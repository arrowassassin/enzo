//! A minimal ZIP reader over `ReadAt`: central directory, stored and deflated entries,
//! ZIP64 not required for EPUB/CBZ sizes but tolerated where offsets fit in 32 bits.
//! Nothing is loaded whole; entries are streamed through [`Inflater`].

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::{ReadAt, Slice};

use crate::inflate::{ByteStream, Framing, InflatedRead, Inflater};
use crate::DocError;

/// Largest central directory we will hold in RAM.
#[cfg(target_os = "none")]
const CD_LIMIT: u64 = 256 * 1024;
/// Largest central directory we will hold in RAM.
#[cfg(not(target_os = "none"))]
const CD_LIMIT: u64 = 8 * 1024 * 1024;
/// First, cheap tail read when looking for the end-of-central-directory record: almost
/// every archive has no comment, so the record sits in the last 22 bytes.
const EOCD_TAIL_SMALL: u64 = 1024 + 22;
/// Full scan: the comment can be up to 64 KB.
const EOCD_TAIL_FULL: u64 = 65_535 + 22;
/// Deflated entries up to this size are loaded whole by [`Zip::entry_reader`]: below it
/// the bytes cost less than a second inflate state plus window (~70 KB) would, and
/// decoding from memory is faster.
const SMALL_ENTRY: u64 = 64 * 1024;

/// One entry from the central directory.
#[derive(Clone, Debug)]
pub struct Entry {
    /// Path inside the archive.
    pub name: String,
    /// 0 = stored, 8 = deflate.
    pub method: u16,
    /// Compressed size.
    pub csize: u64,
    /// Uncompressed size.
    pub usize_: u64,
    /// Offset of the local header.
    local_off: u64,
    /// CRC-32 of the uncompressed data.
    pub crc: u32,
}

/// An opened archive.
pub struct Zip<R: ReadAt> {
    src: R,
    /// Entries in central-directory order.
    pub entries: Vec<Entry>,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl<R: ReadAt> Zip<R> {
    /// Parse the central directory.
    pub fn open(src: R) -> Result<Self, DocError> {
        let len = src.len();
        if len < 22 {
            return Err(DocError::Malformed("zip too small"));
        }
        // Find the end-of-central-directory record: try the last 1 KB first, then the
        // full 64 KB + 22 the comment could push it back to.
        let mut found: Option<(Vec<u8>, usize)> = None;
        for want in [EOCD_TAIL_SMALL, EOCD_TAIL_FULL] {
            let tail_len = len.min(want) as usize;
            let tail = src.read_range(len - tail_len as u64, tail_len)?;
            // The record is 22 bytes, so the signature can start no later than len - 22.
            if let Some(i) = tail[..tail_len - 18].windows(4).rposition(|w| w == b"PK\x05\x06") {
                found = Some((tail, i));
                break;
            }
            if tail_len as u64 == len {
                break;
            }
        }
        let (tail, e) = found.ok_or(DocError::Malformed("zip: no end of central directory"))?;
        let count = u16le(&tail, e + 10) as usize;
        let cd_size = u32le(&tail, e + 12) as u64;
        let cd_off = u32le(&tail, e + 16) as u64;
        if cd_off + cd_size > len {
            return Err(DocError::Malformed("zip: central directory out of range"));
        }
        if cd_size > CD_LIMIT {
            return Err(DocError::TooLarge("zip central directory"));
        }
        let cd = src.read_range(cd_off, cd_size as usize)?;
        let mut entries = Vec::with_capacity(count.min(4096));
        let mut p = 0usize;
        while p + 46 <= cd.len() && &cd[p..p + 4] == b"PK\x01\x02" {
            let method = u16le(&cd, p + 10);
            let crc = u32le(&cd, p + 16);
            let csize = u32le(&cd, p + 20) as u64;
            let usize_ = u32le(&cd, p + 24) as u64;
            let nlen = u16le(&cd, p + 28) as usize;
            let xlen = u16le(&cd, p + 30) as usize;
            let clen = u16le(&cd, p + 32) as usize;
            let local_off = u32le(&cd, p + 42) as u64;
            let name_end = p + 46 + nlen;
            if name_end > cd.len() {
                break;
            }
            let name = String::from_utf8_lossy(&cd[p + 46..name_end]).into_owned();
            entries.push(Entry { name, method, csize, usize_, local_off, crc });
            p = name_end + xlen + clen;
        }
        Ok(Zip { src, entries })
    }

    /// Find an entry by exact path (case-sensitive), then case-insensitively.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let name = name.trim_start_matches('/');
        self.entries.iter().find(|e| e.name == name).or_else(|| self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
    }

    /// Offset of an entry's data, from its local header (whose name/extra lengths may
    /// differ from the central directory's). Refuses encrypted entries.
    fn data_offset(&self, entry: &Entry) -> Result<u64, DocError> {
        let mut hdr = [0u8; 30];
        self.src.read_exact_at(entry.local_off, &mut hdr)?;
        if &hdr[..4] != b"PK\x03\x04" {
            return Err(DocError::Malformed("zip: bad local header"));
        }
        let nlen = u16le(&hdr, 26) as u64;
        let xlen = u16le(&hdr, 28) as u64;
        let flags = u16le(&hdr, 6);
        if flags & 1 != 0 {
            return Err(DocError::Drm);
        }
        match entry.method {
            0 | 8 => Ok(entry.local_off + 30 + nlen + xlen),
            _ => Err(DocError::Unsupported("zip compression method")),
        }
    }

    /// Open an entry as a byte stream.
    pub fn stream(&self, entry: &Entry) -> Result<ByteStream<&R>, DocError> {
        let data = self.data_offset(entry)?;
        match entry.method {
            0 => Ok(ByteStream::Stored { src: Slice::new(&self.src, data, entry.csize), pos: 0, buf: alloc::vec![0; 8192] }),
            _ => Ok(ByteStream::Deflated(Inflater::new(Slice::new(&self.src, data, entry.csize), Framing::Raw))),
        }
    }

    /// Open an entry as a random-access reader without loading it: a plain slice of the
    /// archive for stored entries, a windowed [`InflatedRead`] for deflated ones. This is
    /// what the image decoders consume, so a 3 MB cover costs its decode buffers, not
    /// 3 MB of heap. Deflated entries under [`SMALL_ENTRY`] are simply inflated into
    /// memory, which is cheaper than a second inflate state.
    pub fn entry_reader(&self, entry: &Entry) -> Result<EntryReader<&R>, DocError> {
        let data = self.data_offset(entry)?;
        let raw = Slice::new(&self.src, data, entry.csize);
        match entry.method {
            0 => Ok(EntryReader::Stored(raw)),
            _ if entry.usize_ <= SMALL_ENTRY => Ok(EntryReader::Loaded(Inflater::new(raw, Framing::Raw).read_all(SMALL_ENTRY as usize)?)),
            _ => Ok(EntryReader::Deflated(InflatedRead::new(raw, Framing::Raw, entry.usize_))),
        }
    }

    /// Read an entry fully, bounded.
    pub fn read(&self, entry: &Entry, limit: usize) -> Result<Vec<u8>, DocError> {
        self.stream(entry)?.read_all(limit)
    }

    /// Read a named entry fully, bounded.
    pub fn read_name(&self, name: &str, limit: usize) -> Result<Vec<u8>, DocError> {
        let e = self.find(name).ok_or(DocError::Malformed("zip: missing entry"))?.clone();
        self.read(&e, limit)
    }

    /// The reader.
    pub fn source(&self) -> &R {
        &self.src
    }
}

/// A random-access view of one entry's uncompressed bytes (see [`Zip::entry_reader`]).
pub enum EntryReader<R: ReadAt + Clone> {
    /// Stored entry: a slice of the archive.
    Stored(Slice<R>),
    /// Small deflated entry, inflated up front.
    Loaded(Vec<u8>),
    /// Large deflated entry: inflated on demand.
    Deflated(InflatedRead<Slice<R>>),
}

impl<R: ReadAt + Clone> ReadAt for EntryReader<R> {
    fn len(&self) -> u64 {
        match self {
            EntryReader::Stored(s) => s.len(),
            EntryReader::Loaded(v) => v.len() as u64,
            EntryReader::Deflated(d) => d.len(),
        }
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> quire_fs::FsResult<usize> {
        match self {
            EntryReader::Stored(s) => s.read_at(offset, buf),
            EntryReader::Loaded(v) => v.read_at(offset, buf),
            EntryReader::Deflated(d) => d.read_at(offset, buf),
        }
    }
}

#[cfg(test)]
pub(crate) mod testzip {
    //! A tiny writer for tests: stored or deflated entries, correct CRCs.
    use super::*;

    pub struct Builder {
        data: Vec<u8>,
        cd: Vec<u8>,
        count: u16,
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    impl Builder {
        pub fn new() -> Self {
            Builder { data: Vec::new(), cd: Vec::new(), count: 0 }
        }
        pub fn add(&mut self, name: &str, content: &[u8], deflate: bool) -> &mut Self {
            let (method, payload) =
                if deflate { (8u16, miniz_oxide::deflate::compress_to_vec(content, 6)) } else { (0u16, content.to_vec()) };
            let crc = crc32(content);
            let off = self.data.len() as u32;
            let mut h = Vec::new();
            h.extend_from_slice(b"PK\x03\x04");
            h.extend_from_slice(&20u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&method.to_le_bytes());
            h.extend_from_slice(&[0, 0, 0, 0]);
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            h.extend_from_slice(&(content.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(name.as_bytes());
            self.data.extend_from_slice(&h);
            self.data.extend_from_slice(&payload);
            let mut c = Vec::new();
            c.extend_from_slice(b"PK\x01\x02");
            c.extend_from_slice(&[20, 0, 20, 0]);
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&method.to_le_bytes());
            c.extend_from_slice(&[0, 0, 0, 0]);
            c.extend_from_slice(&crc.to_le_bytes());
            c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            c.extend_from_slice(&(content.len() as u32).to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            c.extend_from_slice(&[0, 0, 0, 0]);
            c.extend_from_slice(&off.to_le_bytes());
            c.extend_from_slice(name.as_bytes());
            self.cd.extend_from_slice(&c);
            self.count += 1;
            self
        }
        pub fn finish(self) -> Vec<u8> {
            let mut out = self.data;
            let cd_off = out.len() as u32;
            out.extend_from_slice(&self.cd);
            out.extend_from_slice(b"PK\x05\x06");
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&self.count.to_le_bytes());
            out.extend_from_slice(&self.count.to_le_bytes());
            out.extend_from_slice(&(self.cd.len() as u32).to_le_bytes());
            out.extend_from_slice(&cd_off.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_and_deflated_entries_round_trip() {
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let mut b = testzip::Builder::new();
        b.add("mimetype", b"application/epub+zip", false).add("OPS/ch1.xhtml", b"<p>Hello</p>", true).add("big.bin", &big, true);
        let bytes = b.finish();
        let z = Zip::open(&bytes).unwrap();
        assert_eq!(z.entries.len(), 3);
        assert_eq!(z.read_name("mimetype", 100).unwrap(), b"application/epub+zip");
        assert_eq!(z.read_name("OPS/ch1.xhtml", 100).unwrap(), b"<p>Hello</p>");
        assert_eq!(z.read_name("big.bin", 200_000).unwrap(), big);
        assert!(z.read_name("big.bin", 1000).is_err(), "limit enforced");
        assert!(z.find("ops/CH1.XHTML").is_some(), "case-insensitive fallback");
    }

    #[test]
    fn entry_reader_streams_stored_and_deflated_entries() {
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let mut b = testzip::Builder::new();
        b.add("stored.bin", &big, false).add("deflated.bin", &big, true).add("small.bin", &big[..1000], true);
        let bytes = b.finish();
        let z = Zip::open(&bytes).unwrap();
        let small = z.entry_reader(z.find("small.bin").unwrap()).unwrap();
        assert!(matches!(small, EntryReader::Loaded(_)), "small deflated entries are loaded whole");
        assert_eq!(small.read_range(0, 1000).unwrap(), big[..1000]);
        for name in ["stored.bin", "deflated.bin"] {
            let e = z.find(name).unwrap().clone();
            let r = z.entry_reader(&e).unwrap();
            if name == "deflated.bin" {
                assert!(matches!(r, EntryReader::Deflated(_)), "large deflated entries stream");
            }
            assert_eq!(r.len(), 100_000);
            assert_eq!(r.read_range(0, 100_000).unwrap(), big, "{name}");
            let mut mid = [0u8; 10];
            r.read_exact_at(50_000, &mut mid).unwrap();
            assert_eq!(mid, big[50_000..50_010], "{name}");
            let mut head = [0u8; 10];
            r.read_exact_at(3, &mut head).unwrap();
            assert_eq!(head, big[3..13], "{name} backward read");
        }
    }

    #[test]
    fn eocd_behind_a_long_comment_is_found_by_the_fallback_scan() {
        let mut b = testzip::Builder::new();
        b.add("a.txt", b"hello", false);
        let mut bytes = b.finish();
        // Append a 20 KB comment and record its length in the EOCD.
        let comment = alloc::vec![b'x'; 20_000];
        let n = bytes.len();
        bytes[n - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&comment);
        let z = Zip::open(&bytes).unwrap();
        assert_eq!(z.read_name("a.txt", 16).unwrap(), b"hello");
    }

    #[test]
    fn real_epub_directory_parses() {
        let data = include_bytes!("../fixtures/wasteland.epub");
        let z = Zip::open(&data[..]).unwrap();
        assert!(z.entries.len() > 5);
        assert_eq!(z.read_name("mimetype", 64).unwrap(), b"application/epub+zip");
        let c = z.read_name("META-INF/container.xml", 8192).unwrap();
        assert!(core::str::from_utf8(&c).unwrap().contains("rootfile"));
    }
}
