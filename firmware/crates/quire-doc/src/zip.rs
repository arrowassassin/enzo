//! A minimal ZIP reader over `ReadAt`: central directory, stored and deflated entries,
//! ZIP64 not required for EPUB/CBZ sizes but tolerated where offsets fit in 32 bits.
//! Nothing is loaded whole; entries are streamed through [`Inflater`].

use alloc::string::String;
use alloc::vec::Vec;
use quire_fs::{ReadAt, Slice};

use crate::inflate::{ByteStream, Framing, Inflater};
use crate::DocError;

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
        // Find the end-of-central-directory record in the last 64 KB + 22.
        let tail_len = len.min(65_557) as usize;
        let tail = src.read_range(len - tail_len as u64, tail_len)?;
        let mut eocd = None;
        let mut i = tail_len.saturating_sub(22);
        loop {
            if &tail[i..i + 4] == b"PK\x05\x06" {
                eocd = Some(i);
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        let e = eocd.ok_or(DocError::Malformed("zip: no end of central directory"))?;
        let count = u16le(&tail, e + 10) as usize;
        let cd_size = u32le(&tail, e + 12) as u64;
        let cd_off = u32le(&tail, e + 16) as u64;
        if cd_off + cd_size > len || cd_size > 8 * 1024 * 1024 {
            return Err(DocError::Malformed("zip: central directory out of range"));
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

    /// Open an entry as a byte stream.
    pub fn stream(&self, entry: &Entry) -> Result<ByteStream<&R>, DocError> {
        // Local header: 30 bytes + name + extra (the central directory's lengths may differ).
        let mut hdr = [0u8; 30];
        self.src.read_exact_at(entry.local_off, &mut hdr)?;
        if &hdr[..4] != b"PK\x03\x04" {
            return Err(DocError::Malformed("zip: bad local header"));
        }
        let nlen = u16le(&hdr, 26) as u64;
        let xlen = u16le(&hdr, 28) as u64;
        let data = entry.local_off + 30 + nlen + xlen;
        let flags = u16le(&hdr, 6);
        if flags & 1 != 0 {
            return Err(DocError::Drm);
        }
        match entry.method {
            0 => Ok(ByteStream::Stored { src: Slice::new(&self.src, data, entry.csize), pos: 0, buf: alloc::vec![0; 8192] }),
            8 => Ok(ByteStream::Deflated(Inflater::new(Slice::new(&self.src, data, entry.csize), Framing::Raw))),
            _ => Err(DocError::Unsupported("zip compression method")),
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
            let (method, payload) = if deflate { (8u16, miniz_oxide::deflate::compress_to_vec(content, 6)) } else { (0u16, content.to_vec()) };
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
    fn real_epub_directory_parses() {
        let data = include_bytes!("../fixtures/wasteland.epub");
        let z = Zip::open(&data[..]).unwrap();
        assert!(z.entries.len() > 5);
        assert_eq!(z.read_name("mimetype", 64).unwrap(), b"application/epub+zip");
        let c = z.read_name("META-INF/container.xml", 8192).unwrap();
        assert!(core::str::from_utf8(&c).unwrap().contains("rootfile"));
    }
}
