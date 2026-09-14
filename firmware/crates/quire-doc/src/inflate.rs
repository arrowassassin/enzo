//! Streaming inflate over a `ReadAt` source in bounded chunks. Used by ZIP entries
//! (raw deflate), PDF `FlateDecode` streams and PNG image data (zlib).

use alloc::vec;
use alloc::vec::Vec;
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};
use quire_fs::{ReadAt, Slice};

use crate::DocError;

/// Input chunk size read from storage.
const IN_CHUNK: usize = 4096;
/// Output chunk size handed to the consumer.
const OUT_CHUNK: usize = 8192;

/// Which framing the compressed data has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// Raw deflate (ZIP entries).
    Raw,
    /// zlib header + adler (PDF FlateDecode, PNG IDAT).
    Zlib,
}

/// A pull-style inflater: call [`Inflater::next_chunk`] until it returns an empty slice.
pub struct Inflater<R: ReadAt> {
    src: R,
    pos: u64,
    state: InflateState,
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
        let fmt = match framing {
            Framing::Raw => DataFormat::Raw,
            Framing::Zlib => DataFormat::Zlib,
        };
        Inflater {
            src,
            pos: 0,
            state: InflateState::new(fmt),
            inbuf: vec![0; IN_CHUNK],
            in_len: 0,
            in_pos: 0,
            outbuf: vec![0; OUT_CHUNK],
            done: false,
            produced: 0,
        }
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
    pub fn read_all(mut self, limit: usize) -> Result<Vec<u8>, DocError> {
        let mut out = Vec::new();
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
        let mut out = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_raw_and_zlib() {
        let text: Vec<u8> = (0..50_000u32).map(|i| b"the quick brown fox "[(i % 20) as usize]).collect();
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
}
