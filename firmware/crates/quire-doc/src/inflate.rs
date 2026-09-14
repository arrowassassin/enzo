//! Streaming inflate over a `ReadAt` source in bounded chunks. Used by ZIP entries
//! (raw deflate), PDF `FlateDecode` streams and PNG image data (zlib).
//!
//! The miniz state is ~43 KB, so it always lives on the heap ([`Inflater`] boxes it);
//! the device task stack is far too small to hold it. [`InflatedRead`] turns the
//! pull-style inflater into a `ReadAt` so the image decoders can consume a deflated
//! ZIP entry without ever holding the whole entry in RAM.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};
use quire_fs::{ReadAt, Slice};

use crate::DocError;

/// Input chunk size read from storage.
const IN_CHUNK: usize = 4096;
/// Output chunk size handed to the consumer.
const OUT_CHUNK: usize = 8192;
/// Bytes of already-produced output that [`InflatedRead`] keeps for backward reads.
pub const WINDOW: usize = 8192;

/// Which framing the compressed data has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// Raw deflate (ZIP entries).
    Raw,
    /// zlib header + adler (PDF FlateDecode, PNG IDAT).
    Zlib,
}

impl Framing {
    fn format(self) -> DataFormat {
        match self {
            Framing::Raw => DataFormat::Raw,
            Framing::Zlib => DataFormat::Zlib,
        }
    }
}

/// A pull-style inflater: call [`Inflater::next_chunk`] until it returns an empty slice.
pub struct Inflater<R: ReadAt> {
    src: R,
    pos: u64,
    state: Box<InflateState>,
    inbuf: Vec<u8>,
    in_len: usize,
    in_pos: usize,
    outbuf: Vec<u8>,
    done: bool,
    /// Total bytes produced so far.
    pub produced: u64,
}

impl<R: ReadAt> Inflater<R> {
    /// Start inflating `src` from its beginning.
    pub fn new(src: R, framing: Framing) -> Self {
        Inflater {
            src,
            pos: 0,
            state: InflateState::new_boxed(framing.format()),
            inbuf: vec![0; IN_CHUNK],
            in_len: 0,
            in_pos: 0,
            outbuf: vec![0; OUT_CHUNK],
            done: false,
            produced: 0,
        }
    }

    /// Restart from the beginning of `src`, reusing every buffer and the boxed state.
    pub fn reset(&mut self, src: R, framing: Framing) {
        self.src = src;
        self.pos = 0;
        self.state.reset(framing.format());
        self.in_len = 0;
        self.in_pos = 0;
        self.done = false;
        self.produced = 0;
    }

    /// The compressed source.
    pub fn source(&self) -> &R {
        &self.src
    }

    /// Produce the next chunk of decompressed bytes. Empty when finished.
    pub fn next_chunk(&mut self) -> Result<&[u8], DocError> {
        if self.done {
            return Ok(&[]);
        }
        let mut out_len = 0usize;
        while out_len == 0 {
            if self.in_pos >= self.in_len {
                self.in_len = self.src.read_at(self.pos, &mut self.inbuf)?;
                self.in_pos = 0;
                self.pos += self.in_len as u64;
            }
            let flush = if self.in_len == 0 { MZFlush::Finish } else { MZFlush::None };
            let r = inflate(&mut self.state, &self.inbuf[self.in_pos..self.in_len], &mut self.outbuf, flush);
            self.in_pos += r.bytes_consumed;
            out_len = r.bytes_written;
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.done = true;
                    break;
                }
                Ok(_) => {
                    if self.in_len == 0 && out_len == 0 {
                        // Input exhausted without a stream end: treat as finished (truncated
                        // streams are common in the wild and everything readable was read).
                        self.done = true;
                        break;
                    }
                }
                Err(_) => return Err(DocError::Malformed("deflate stream")),
            }
        }
        self.produced += out_len as u64;
        Ok(&self.outbuf[..out_len])
    }

    /// Inflate everything into a vector, refusing to exceed `limit` bytes.
    ///
    /// The vector is pre-sized to `min(limit, 4 × compressed size)` so a typical text
    /// entry (3–4× ratio) grows at most once, and the limit is checked before the
    /// vector is ever grown past it.
    pub fn read_all(mut self, limit: usize) -> Result<Vec<u8>, DocError> {
        let src_len = self.src.len();
        let guess = src_len.saturating_mul(4).min(limit as u64) as usize;
        let mut out = Vec::with_capacity(guess);
        loop {
            let chunk = self.next_chunk()?;
            if chunk.is_empty() {
                break;
            }
            if out.len() + chunk.len() > limit {
                return Err(DocError::TooLarge("inflated stream"));
            }
            out.extend_from_slice(chunk);
        }
        Ok(out)
    }
}

/// Inflate a sub-range of a reader.
pub fn inflate_range<R: ReadAt>(src: R, start: u64, len: u64, framing: Framing) -> Inflater<Slice<R>> {
    Inflater::new(Slice::new(src, start, len), framing)
}

/// A byte source that is either stored or inflated, exposing `next_chunk` uniformly.
pub enum ByteStream<R: ReadAt> {
    /// Stored bytes, read in chunks.
    Stored {
        /// The entry's bytes.
        src: Slice<R>,
        /// Read position.
        pos: u64,
        /// Chunk buffer.
        buf: Vec<u8>,
    },
    /// Inflated bytes.
    Deflated(Inflater<Slice<R>>),
}

impl<R: ReadAt> ByteStream<R> {
    /// Next chunk; empty at the end.
    pub fn next_chunk(&mut self) -> Result<&[u8], DocError> {
        match self {
            ByteStream::Stored { src, pos, buf } => {
                let n = src.read_at(*pos, buf)?;
                *pos += n as u64;
                Ok(&buf[..n])
            }
            ByteStream::Deflated(i) => i.next_chunk(),
        }
    }
    /// Read everything, bounded.
    pub fn read_all(mut self, limit: usize) -> Result<Vec<u8>, DocError> {
        if let ByteStream::Deflated(i) = self {
            return i.read_all(limit);
        }
        // Stored: the claimed length may lie (hostile archives), so read until the
        // underlying slice runs dry rather than trusting it for one big read.
        let claimed = match &self {
            ByteStream::Stored { src, .. } => src.len(),
            ByteStream::Deflated(_) => 0,
        };
        let mut out = Vec::with_capacity(claimed.min(limit as u64) as usize);
        loop {
            let c = self.next_chunk()?;
            if c.is_empty() {
                break;
            }
            if out.len() + c.len() > limit {
                return Err(DocError::TooLarge("stream"));
            }
            out.extend_from_slice(c);
        }
        Ok(out)
    }
}

/// Mutable half of [`InflatedRead`].
struct InflatedInner<R: ReadAt> {
    inf: Inflater<R>,
    /// Output bytes `[start, start + buf.len())` of the inflated stream.
    buf: Vec<u8>,
    start: u64,
    /// True once the inflater ran dry (no further forward reads will yield bytes).
    ended: bool,
    /// How many times the stream was rewound to serve a backward read.
    restarts: u32,
}

/// A forward-only inflated stream presented as a random-access `ReadAt`.
///
/// Offsets that increase monotonically are served straight from the inflater; a
/// sliding window of the last [`WINDOW`] bytes serves small backward reads (a decoder
/// re-reading a header). A backward read beyond the window restarts the inflater from
/// the beginning of the entry — correct, just slower — so decoders that seek freely
/// (bottom-up BMP) still work.
///
/// `len()` is the uncompressed size the caller claims (a ZIP central directory entry);
/// reads past the real end return 0 bytes regardless.
pub struct InflatedRead<R: ReadAt + Clone> {
    inner: RefCell<InflatedInner<R>>,
    src: R,
    framing: Framing,
    len: u64,
}

impl<R: ReadAt + Clone> InflatedRead<R> {
    /// Wrap a compressed source whose inflated length is `len`.
    pub fn new(src: R, framing: Framing, len: u64) -> Self {
        let inf = Inflater::new(src.clone(), framing);
        InflatedRead {
            inner: RefCell::new(InflatedInner { inf, buf: Vec::with_capacity(2 * WINDOW), start: 0, ended: false, restarts: 0 }),
            src,
            framing,
            len,
        }
    }

    /// Number of times a backward read forced a restart (diagnostics and tests).
    pub fn restarts(&self) -> u32 {
        self.inner.borrow().restarts
    }
}

impl<R: ReadAt + Clone> ReadAt for InflatedRead<R> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, out: &mut [u8]) -> quire_fs::FsResult<usize> {
        if offset >= self.len || out.is_empty() {
            return Ok(0);
        }
        let mut st = self.inner.borrow_mut();
        let st = &mut *st;
        if offset < st.start {
            // Behind the window: rewind.
            st.inf.reset(self.src.clone(), self.framing);
            st.buf.clear();
            st.start = 0;
            st.ended = false;
            st.restarts += 1;
        }
        while offset >= st.start + st.buf.len() as u64 {
            if st.ended {
                return Ok(0);
            }
            let chunk = st.inf.next_chunk().map_err(|_| quire_fs::FsError::Io(alloc::string::String::from("inflate")))?;
            if chunk.is_empty() {
                st.ended = true;
                return Ok(0);
            }
            // Keep at most ~2 × WINDOW bytes: trim the front before appending so the
            // fresh chunk is always wholly retained.
            if st.buf.len() + chunk.len() > 2 * WINDOW {
                let keep = WINDOW.saturating_sub(chunk.len()).min(st.buf.len());
                let drop = st.buf.len() - keep;
                st.buf.copy_within(drop.., 0);
                st.buf.truncate(keep);
                st.start += drop as u64;
            }
            st.buf.extend_from_slice(chunk);
        }
        let from = (offset - st.start) as usize;
        let n = out.len().min(st.buf.len() - from);
        out[..n].copy_from_slice(&st.buf[from..from + n]);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        (0..50_000u32).map(|i| b"the quick brown fox "[(i % 20) as usize]).collect()
    }

    #[test]
    fn round_trip_raw_and_zlib() {
        let text = sample();
        let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let zl = miniz_oxide::deflate::compress_to_vec_zlib(&text, 6);
        let a = Inflater::new(&raw, Framing::Raw).read_all(1 << 20).unwrap();
        let b = Inflater::new(&zl, Framing::Zlib).read_all(1 << 20).unwrap();
        assert_eq!(a, text);
        assert_eq!(b, text);
        assert!(Inflater::new(&zl, Framing::Zlib).read_all(1000).is_err(), "limit enforced");
    }

    #[test]
    fn corrupt_input_is_an_error_not_a_panic() {
        let junk = vec![0xFFu8; 300];
        assert!(Inflater::new(&junk, Framing::Zlib).read_all(1 << 16).is_err());
    }

    #[test]
    fn boxed_state_is_small_on_the_stack_and_reset_reuses_it() {
        // The inflater itself must be a handful of words: the 43 KB miniz state is boxed.
        assert!(core::mem::size_of::<Inflater<&[u8]>>() < 256, "{}", core::mem::size_of::<Inflater<&[u8]>>());
        let text = sample();
        let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let zl = miniz_oxide::deflate::compress_to_vec_zlib(&text, 6);
        let mut inf = Inflater::new(&raw[..], Framing::Raw);
        let mut out = Vec::new();
        loop {
            let c = inf.next_chunk().unwrap();
            if c.is_empty() {
                break;
            }
            out.extend_from_slice(c);
        }
        assert_eq!(out, text);
        inf.reset(&zl[..], Framing::Zlib);
        assert_eq!(inf.produced, 0);
        let out2 = inf.read_all(1 << 20).unwrap();
        assert_eq!(out2, text);
    }

    #[test]
    fn read_all_presizes_and_refuses_before_growing() {
        let text = sample();
        let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let out = Inflater::new(&raw[..], Framing::Raw).read_all(60_000).unwrap();
        assert_eq!(out.len(), 50_000);
        assert!(out.capacity() <= 60_000 + OUT_CHUNK, "capacity {} stays near the limit", out.capacity());
        assert!(matches!(Inflater::new(&raw[..], Framing::Raw).read_all(49_999), Err(DocError::TooLarge(_))));
    }

    #[test]
    fn inflated_read_serves_forward_reads_and_restarts_on_backward_reads() {
        let text = sample();
        let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let r = InflatedRead::new(&raw[..], Framing::Raw, text.len() as u64);
        assert_eq!(r.len(), 50_000);
        // Forward reads of varying sizes, including ones that straddle chunk boundaries.
        let mut off = 0usize;
        let mut got = Vec::new();
        for (i, size) in [7usize, 100, 8191, 8193, 3, 20_000, 40_000].iter().enumerate() {
            let mut buf = vec![0u8; *size];
            r.read_exact_at(off as u64, &mut buf[..(*size).min(text.len() - off)]).unwrap_or_else(|e| panic!("read {i}: {e}"));
            let n = (*size).min(text.len() - off);
            got.extend_from_slice(&buf[..n]);
            off += n;
            if off >= text.len() {
                break;
            }
        }
        assert_eq!(got, text[..got.len()]);
        assert_eq!(r.restarts(), 0, "forward reads never restart");
        // A small backward read inside the window is served from the window.
        let mut small = [0u8; 16];
        r.read_exact_at((off - 100) as u64, &mut small).unwrap();
        assert_eq!(small, text[off - 100..off - 84]);
        assert_eq!(r.restarts(), 0, "window serves small backward reads");
        // A backward read far beyond the window restarts the stream and is still correct.
        let mut head = [0u8; 32];
        r.read_exact_at(5, &mut head).unwrap();
        assert_eq!(head, text[5..37]);
        assert_eq!(r.restarts(), 1);
        // Past the end: zero bytes, and read_exact_at reports Eof.
        let mut tail = [0u8; 8];
        assert_eq!(r.read_at(50_000, &mut tail).unwrap(), 0);
        assert!(r.read_exact_at(49_996, &mut tail).is_err());
        // Whole stream via read_range matches.
        assert_eq!(r.read_range(0, 50_000).unwrap(), text);
    }

    #[test]
    fn inflated_read_over_a_lying_length_stops_at_the_real_end() {
        let text = sample();
        let raw = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let r = InflatedRead::new(&raw[..], Framing::Raw, u32::MAX as u64);
        let mut buf = vec![0u8; 1000];
        assert_eq!(r.read_at(49_500, &mut buf).unwrap(), 500);
        assert_eq!(r.read_at(60_000, &mut buf).unwrap(), 0);
    }
}
