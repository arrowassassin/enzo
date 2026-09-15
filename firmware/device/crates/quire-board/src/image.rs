//! The ESP-IDF application image format, verified the way the bootloader verifies it
//! (`esp_image_format.c`): a 24-byte header (magic 0xE9, segment count, chip id, the
//! `hash_appended` flag), `segment_count` segments of an 8-byte header plus word-aligned
//! data, padding up to a 16-byte boundary whose last byte is the XOR checksum of all
//! segment data seeded with 0xEF, and, when `hash_appended` is set, the SHA-256 of
//! everything before it. The app descriptor (`esp_app_desc_t`, magic 0xABCD5432) is the
//! first thing in the first segment, at image offset 32.
//!
//! [`Verifier`] is fed the image in chunks of any size, so the same code checks a file as
//! it streams to flash and a slot as it is read back. Pure: no HAL, no allocation.

use sha2::{Digest, Sha256};

/// First byte of an image.
pub const MAGIC: u8 = 0xE9;
/// Header length.
pub const HEADER_LEN: usize = 24;
/// Segment header length.
pub const SEGMENT_HEADER_LEN: usize = 8;
/// Chip id of the ESP32-C3 in the header.
pub const CHIP_ESP32C3: u16 = 5;
/// Most segments an image may have.
pub const MAX_SEGMENTS: u8 = 16;
/// Seed of the XOR checksum.
pub const CHECKSUM_SEED: u8 = 0xEF;
/// Where the app descriptor starts in the image.
pub const APP_DESC_OFFSET: usize = HEADER_LEN + SEGMENT_HEADER_LEN;
/// Bytes of the app descriptor the screens use (magic through the build date).
pub const APP_DESC_LEN: usize = 112;
/// Bytes from the image start through the app descriptor fields we read.
pub const HEAD_LEN: usize = APP_DESC_OFFSET + APP_DESC_LEN;
const APP_DESC_MAGIC: u32 = 0xABCD_5432;

/// Why an image was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// The first byte is not 0xE9.
    BadMagic,
    /// Built for another chip.
    WrongChip(u16),
    /// Zero or too many segments.
    SegmentCount(u8),
    /// A segment's length is not a multiple of four.
    SegmentLength,
    /// The image runs past the space it must fit in.
    TooLarge,
    /// The data ended before the image did.
    Truncated,
    /// The XOR checksum byte does not match.
    Checksum,
    /// The appended SHA-256 does not match.
    Hash,
}

impl core::fmt::Display for ImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ImageError::BadMagic => write!(f, "not a firmware image"),
            ImageError::WrongChip(c) => write!(f, "built for another chip ({c})"),
            ImageError::SegmentCount(n) => write!(f, "bad segment count ({n})"),
            ImageError::SegmentLength => write!(f, "bad segment length"),
            ImageError::TooLarge => write!(f, "image too large"),
            ImageError::Truncated => write!(f, "image truncated"),
            ImageError::Checksum => write!(f, "checksum mismatch"),
            ImageError::Hash => write!(f, "SHA-256 mismatch"),
        }
    }
}

/// What a verified image looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// Bytes from the image start through the appended hash (or the checksum block).
    pub len: u32,
    /// Segments walked.
    pub segments: u8,
    /// Whether a SHA-256 was appended and checked.
    pub hash_checked: bool,
    /// The first [`HEAD_LEN`] bytes, for [`app_desc`].
    pub head: [u8; HEAD_LEN],
}

impl ImageInfo {
    /// The app descriptor, when the image carries one.
    pub fn desc(&self) -> Option<AppDesc<'_>> {
        app_desc(&self.head)
    }
}

/// The fields of `esp_app_desc_t` the screens show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDesc<'a> {
    /// `CARGO_PKG_VERSION` of the build.
    pub version: &'a str,
    /// Package name.
    pub project: &'a str,
    /// Build time, `HH:MM:SS`.
    pub time: &'a str,
    /// Build date, `YYYY-MM-DD`.
    pub date: &'a str,
}

fn cstr(b: &[u8]) -> &str {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    core::str::from_utf8(&b[..n]).unwrap_or("")
}

/// Read the app descriptor from the start of an image (at least [`HEAD_LEN`] bytes).
pub fn app_desc(image: &[u8]) -> Option<AppDesc<'_>> {
    if image.len() < HEAD_LEN || image[0] != MAGIC {
        return None;
    }
    let d = &image[APP_DESC_OFFSET..HEAD_LEN];
    if u32::from_le_bytes([d[0], d[1], d[2], d[3]]) != APP_DESC_MAGIC {
        return None;
    }
    Some(AppDesc { version: cstr(&d[16..48]), project: cstr(&d[48..80]), time: cstr(&d[80..96]), date: cstr(&d[96..112]) })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Header,
    SegmentHeader,
    SegmentData,
    Padding,
    Hash,
    Done,
}

/// Streaming verifier: feed bytes with [`Verifier::update`], then [`Verifier::finish`].
pub struct Verifier {
    phase: Phase,
    /// Bytes consumed so far.
    pos: u32,
    /// Space the image must fit in.
    capacity: u32,
    /// Small scratch for headers, the checksum block and the hash.
    scratch: [u8; 32],
    scratch_len: usize,
    /// Bytes still expected in the current phase (segment data, padding, hash).
    remaining: u32,
    segments_total: u8,
    segments_done: u8,
    hash_appended: bool,
    xor: u8,
    sha: Sha256,
    head: [u8; HEAD_LEN],
}

impl Verifier {
    /// A verifier for an image that must fit in `capacity` bytes.
    pub fn new(capacity: u32) -> Verifier {
        Verifier {
            phase: Phase::Header,
            pos: 0,
            capacity,
            scratch: [0; 32],
            scratch_len: 0,
            remaining: 0,
            segments_total: 0,
            segments_done: 0,
            hash_appended: false,
            xor: CHECKSUM_SEED,
            sha: Sha256::new(),
            head: [0; HEAD_LEN],
        }
    }

    /// Whether the whole image has been seen (further bytes are ignored).
    pub fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Bytes consumed so far.
    pub fn position(&self) -> u32 {
        self.pos
    }

    /// Feed the next bytes. Errors are final.
    pub fn update(&mut self, mut data: &[u8]) -> Result<(), ImageError> {
        while !data.is_empty() {
            match self.phase {
                Phase::Done => return Ok(()),
                Phase::Header => {
                    let n = self.fill(data, HEADER_LEN);
                    data = &data[n..];
                    if self.scratch_len == HEADER_LEN {
                        self.take_header()?;
                    }
                }
                Phase::SegmentHeader => {
                    let n = self.fill(data, SEGMENT_HEADER_LEN);
                    data = &data[n..];
                    if self.scratch_len == SEGMENT_HEADER_LEN {
                        self.take_segment_header()?;
                    }
                }
                Phase::SegmentData => {
                    let n = data.len().min(self.remaining as usize);
                    let (chunk, rest) = data.split_at(n);
                    for &b in chunk {
                        self.xor ^= b;
                    }
                    self.consume(chunk);
                    data = rest;
                    self.remaining -= n as u32;
                    if self.remaining == 0 {
                        self.segments_done += 1;
                        self.phase = if self.segments_done == self.segments_total {
                            // Pad so the checksum byte ends a 16-byte block.
                            self.remaining = ((self.pos + 1 + 15) & !15) - self.pos;
                            Phase::Padding
                        } else {
                            self.scratch_len = 0;
                            Phase::SegmentHeader
                        };
                    }
                }
                Phase::Padding => {
                    let n = data.len().min(self.remaining as usize);
                    let (chunk, rest) = data.split_at(n);
                    let stored = chunk[chunk.len() - 1];
                    self.consume(chunk);
                    data = rest;
                    self.remaining -= n as u32;
                    if self.remaining == 0 {
                        if stored != self.xor {
                            return Err(ImageError::Checksum);
                        }
                        if self.hash_appended {
                            self.scratch_len = 0;
                            self.phase = Phase::Hash;
                        } else {
                            self.phase = Phase::Done;
                        }
                    }
                }
                Phase::Hash => {
                    let n = self.fill_raw(data, 32);
                    data = &data[n..];
                    if self.scratch_len == 32 {
                        let sha = core::mem::replace(&mut self.sha, Sha256::new());
                        if sha.finalize().as_slice() != &self.scratch[..32] {
                            return Err(ImageError::Hash);
                        }
                        self.phase = Phase::Done;
                    }
                }
            }
        }
        Ok(())
    }

    /// Check that the image ended where it should and return what was found.
    pub fn finish(self) -> Result<ImageInfo, ImageError> {
        if self.phase != Phase::Done {
            return Err(ImageError::Truncated);
        }
        Ok(ImageInfo { len: self.pos, segments: self.segments_total, hash_checked: self.hash_appended, head: self.head })
    }

    /// Bytes that are part of the hashed image.
    fn consume(&mut self, chunk: &[u8]) {
        let at = self.pos as usize;
        if at < HEAD_LEN {
            let n = chunk.len().min(HEAD_LEN - at);
            self.head[at..at + n].copy_from_slice(&chunk[..n]);
        }
        self.sha.update(chunk);
        self.pos += chunk.len() as u32;
    }

    /// Gather up to `want` hashed bytes into the scratch buffer; returns how many were taken.
    fn fill(&mut self, data: &[u8], want: usize) -> usize {
        let n = data.len().min(want - self.scratch_len);
        self.scratch[self.scratch_len..self.scratch_len + n].copy_from_slice(&data[..n]);
        self.scratch_len += n;
        self.consume(&data[..n]);
        n
    }

    /// Gather bytes that are not part of the hash (the appended hash itself).
    fn fill_raw(&mut self, data: &[u8], want: usize) -> usize {
        let n = data.len().min(want - self.scratch_len);
        self.scratch[self.scratch_len..self.scratch_len + n].copy_from_slice(&data[..n]);
        self.scratch_len += n;
        self.pos += n as u32;
        n
    }

    fn take_header(&mut self) -> Result<(), ImageError> {
        let h = &self.scratch[..HEADER_LEN];
        if h[0] != MAGIC {
            return Err(ImageError::BadMagic);
        }
        let chip = u16::from_le_bytes([h[12], h[13]]);
        if chip != CHIP_ESP32C3 {
            return Err(ImageError::WrongChip(chip));
        }
        let segments = h[1];
        if segments == 0 || segments > MAX_SEGMENTS {
            return Err(ImageError::SegmentCount(segments));
        }
        self.segments_total = segments;
        self.hash_appended = h[23] == 1;
        self.scratch_len = 0;
        self.phase = Phase::SegmentHeader;
        Ok(())
    }

    fn take_segment_header(&mut self) -> Result<(), ImageError> {
        let s = &self.scratch[..SEGMENT_HEADER_LEN];
        let len = u32::from_le_bytes([s[4], s[5], s[6], s[7]]);
        if !len.is_multiple_of(4) {
            return Err(ImageError::SegmentLength);
        }
        // Data, the checksum block and a hash must all fit.
        if self.pos.checked_add(len).and_then(|e| e.checked_add(16 + 32)).is_none_or(|e| e > self.capacity) {
            return Err(ImageError::TooLarge);
        }
        self.remaining = len;
        self.scratch_len = 0;
        self.phase = if len == 0 {
            // Zero-length segments are legal; skip straight on.
            self.segments_done += 1;
            if self.segments_done == self.segments_total {
                self.remaining = ((self.pos + 1 + 15) & !15) - self.pos;
                Phase::Padding
            } else {
                Phase::SegmentHeader
            }
        } else {
            Phase::SegmentData
        };
        Ok(())
    }
}

/// Verify a whole image held in memory.
pub fn verify(image: &[u8], capacity: u32) -> Result<ImageInfo, ImageError> {
    let mut v = Verifier::new(capacity);
    v.update(image)?;
    v.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// Build an image from segments, with an app descriptor at the start of the first.
    fn image(segments: &[&[u8]], hash: bool) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[MAGIC, segments.len() as u8, 2, 0x2f]);
        v.extend_from_slice(&0x4038_0000u32.to_le_bytes());
        v.extend_from_slice(&[0xEE, 0, 0, 0]);
        v.extend_from_slice(&CHIP_ESP32C3.to_le_bytes());
        v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
        v.push(hash as u8);
        assert_eq!(v.len(), HEADER_LEN);
        let mut xor = CHECKSUM_SEED;
        for (i, s) in segments.iter().enumerate() {
            v.extend_from_slice(&(0x3C00_0020u32 + i as u32 * 0x10000).to_le_bytes());
            v.extend_from_slice(&(s.len() as u32).to_le_bytes());
            v.extend_from_slice(s);
            for &b in *s {
                xor ^= b;
            }
        }
        let padded = (v.len() + 1 + 15) & !15;
        while v.len() < padded - 1 {
            v.push(0);
        }
        v.push(xor);
        if hash {
            let d = Sha256::digest(&v);
            v.extend_from_slice(&d);
        }
        v
    }

    fn desc_segment() -> Vec<u8> {
        let mut d = std::vec![0u8; 256];
        d[0..4].copy_from_slice(&APP_DESC_MAGIC.to_le_bytes());
        d[16..21].copy_from_slice(b"0.4.2");
        d[48..56].copy_from_slice(b"quire-x3");
        d[80..88].copy_from_slice(b"23:51:34");
        d[96..106].copy_from_slice(b"2026-09-13");
        d
    }

    #[test]
    fn verifies_a_hashed_image_in_any_chunking() {
        let seg0 = desc_segment();
        let seg1 = (0..1000u32).map(|i| (i * 7) as u8).collect::<Vec<_>>();
        let img = image(&[&seg0, &seg1, &[]], true);
        let whole = verify(&img, 1 << 20).unwrap();
        assert_eq!(whole.len as usize, img.len());
        assert_eq!(whole.segments, 3);
        assert!(whole.hash_checked);
        let d = whole.desc().unwrap();
        assert_eq!((d.version, d.project, d.time, d.date), ("0.4.2", "quire-x3", "23:51:34", "2026-09-13"));
        for chunk in [1usize, 3, 7, 16, 100, 4096] {
            let mut v = Verifier::new(1 << 20);
            for c in img.chunks(chunk) {
                v.update(c).unwrap();
            }
            assert!(v.done());
            assert_eq!(v.finish().unwrap(), whole, "chunk {chunk}");
        }
    }

    #[test]
    fn unhashed_image_ends_at_the_checksum_block() {
        let img = image(&[&desc_segment()], false);
        let info = verify(&img, 1 << 20).unwrap();
        assert!(!info.hash_checked);
        assert_eq!(info.len as usize, img.len());
        assert_eq!(img.len() % 16, 0);
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let mut img = image(&[&desc_segment()], true);
        let len = img.len();
        img.extend_from_slice(&[0xFF; 100]);
        let info = verify(&img, 1 << 20).unwrap();
        assert_eq!(info.len as usize, len);
    }

    #[test]
    fn rejects_bad_images() {
        let good = image(&[&desc_segment()], true);
        let mut bad = good.clone();
        bad[0] = 0xE8;
        assert_eq!(verify(&bad, 1 << 20), Err(ImageError::BadMagic));
        let mut bad = good.clone();
        bad[12] = 9;
        assert_eq!(verify(&bad, 1 << 20), Err(ImageError::WrongChip(9)));
        let mut bad = good.clone();
        bad[1] = 0;
        assert_eq!(verify(&bad, 1 << 20), Err(ImageError::SegmentCount(0)));
        // A flipped data byte breaks the checksum first.
        let mut bad = good.clone();
        bad[40] ^= 0x10;
        assert_eq!(verify(&bad, 1 << 20), Err(ImageError::Checksum));
        // A flipped padding byte only the hash catches.
        let mut bad = good.clone();
        let pad_at = HEADER_LEN + SEGMENT_HEADER_LEN + 256;
        bad[pad_at] ^= 1;
        assert_eq!(verify(&bad, 1 << 20), Err(ImageError::Hash));
        assert_eq!(verify(&good[..good.len() - 1], 1 << 20), Err(ImageError::Truncated));
        assert_eq!(verify(&good[..10], 1 << 20), Err(ImageError::Truncated));
        assert_eq!(verify(&good, 200), Err(ImageError::TooLarge));
        assert_eq!(verify(&[], 1 << 20), Err(ImageError::Truncated));
        let mut bad = good.clone();
        bad[HEADER_LEN + 4] = 3; // segment length 259
        assert!(matches!(verify(&bad, 1 << 20), Err(ImageError::SegmentLength)));
    }

    #[test]
    fn app_desc_needs_the_magic() {
        let img = image(&[&[0u8; 256]], true);
        assert_eq!(app_desc(&img), None);
        assert_eq!(app_desc(&img[..10]), None);
        let img = image(&[&desc_segment()], true);
        assert_eq!(app_desc(&img).unwrap().version, "0.4.2");
    }
}
