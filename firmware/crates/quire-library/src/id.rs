//! Stable book identifiers: a content hash, so a moved or renamed file keeps its cache.

use core::fmt;
use quire_fs::ReadAt;
use serde::{Deserialize, Serialize};

/// A 64-bit book id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
pub struct BookId(pub u64);

impl fmt::Display for BookId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}
impl fmt::Debug for BookId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BookId({:016x})", self.0)
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a over bytes.
pub fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

impl BookId {
    /// Parse the 16-hex-digit form.
    pub fn parse(s: &str) -> Option<BookId> {
        if s.len() != 16 {
            return None;
        }
        u64::from_str_radix(s, 16).ok().map(BookId)
    }

    /// Identify a file by its size and the first and last 4 KB of content.
    pub fn of_file<R: ReadAt>(file: &R) -> Result<BookId, quire_fs::FsError> {
        let len = file.len();
        let mut h = fnv1a(FNV_OFFSET, &len.to_le_bytes());
        let mut buf = [0u8; 4096];
        let n = file.read_at(0, &mut buf)?;
        h = fnv1a(h, &buf[..n]);
        if len > 4096 {
            let start = len.saturating_sub(4096);
            let n = file.read_at(start, &mut buf)?;
            h = fnv1a(h, &buf[..n]);
        }
        if h == 0 {
            h = 1;
        }
        Ok(BookId(h))
    }

    /// An id for something that is not a file (a downloaded article, a note).
    pub fn of_name(name: &str) -> BookId {
        BookId(fnv1a(FNV_OFFSET ^ 0x5eed, name.as_bytes()).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_content_based() {
        let a = BookId::of_file(&&b"hello world"[..]).unwrap();
        let b = BookId::of_file(&&b"hello world"[..]).unwrap();
        let c = BookId::of_file(&&b"hello worlD"[..]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(BookId::parse(&alloc::format!("{a}")), Some(a));
        assert_eq!(BookId::parse("zz"), None);
    }
}
