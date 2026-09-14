//! Storage traits shared by every crate that touches files, so the parsers, the library
//! and the ingest pipeline compile unchanged for the host (std) and the device (SD card
//! over SPI).
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Errors from storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    /// Path does not exist.
    NotFound,
    /// Underlying device or filesystem failure (message for the log).
    Io(String),
    /// Not enough room on the volume.
    Full,
    /// The name or path is not valid on this filesystem.
    InvalidPath,
    /// Read past the end of a file.
    Eof,
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotFound => write!(f, "not found"),
            FsError::Io(m) => write!(f, "io: {m}"),
            FsError::Full => write!(f, "storage full"),
            FsError::InvalidPath => write!(f, "invalid path"),
            FsError::Eof => write!(f, "unexpected end of file"),
        }
    }
}

/// Result alias.
pub type FsResult<T> = Result<T, FsError>;

/// Random-access reads. Implemented by open files, by byte slices (tests), and by the
/// SD driver on the device. Parsers are written against this so a 5 MB EPUB is never
/// loaded into RAM.
pub trait ReadAt {
    /// Total length in bytes.
    fn len(&self) -> u64;
    /// True when the length is zero.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Read up to `buf.len()` bytes at `offset`; returns the count (0 at end).
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize>;
    /// Fill `buf` exactly or fail with `Eof`.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<()> {
        let mut done = 0;
        while done < buf.len() {
            let n = self.read_at(offset + done as u64, &mut buf[done..])?;
            if n == 0 {
                return Err(FsError::Eof);
            }
            done += n;
        }
        Ok(())
    }
    /// Read a whole range into a new vector (bounded by the caller, so use with care).
    fn read_range(&self, offset: u64, len: usize) -> FsResult<Vec<u8>> {
        let mut v = alloc::vec![0u8; len];
        self.read_exact_at(offset, &mut v)?;
        Ok(v)
    }
}

impl ReadAt for [u8] {
    fn len(&self) -> u64 {
        <[u8]>::len(self) as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        let off = offset as usize;
        if off >= <[u8]>::len(self) {
            return Ok(0);
        }
        let n = buf.len().min(<[u8]>::len(self) - off);
        buf[..n].copy_from_slice(&self[off..off + n]);
        Ok(n)
    }
}

impl ReadAt for Vec<u8> {
    fn len(&self) -> u64 {
        Vec::len(self) as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        self.as_slice().read_at(offset, buf)
    }
}

impl<T: ReadAt + ?Sized> ReadAt for &T {
    fn len(&self) -> u64 {
        (**self).len()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        (**self).read_at(offset, buf)
    }
}

/// A sub-range view of a reader (an entry inside a ZIP, a stream inside a PDF).
#[derive(Clone, Copy, Debug)]
pub struct Slice<R> {
    inner: R,
    start: u64,
    len: u64,
}

impl<R: ReadAt> Slice<R> {
    /// View `len` bytes of `inner` starting at `start`.
    pub fn new(inner: R, start: u64, len: u64) -> Self {
        Slice { inner, start, len }
    }
}

impl<R: ReadAt> ReadAt for Slice<R> {
    fn len(&self) -> u64 {
        self.len
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let n = buf.len().min((self.len - offset) as usize);
        self.inner.read_at(self.start + offset, &mut buf[..n])
    }
}

/// Sequential reading in chunks over a `ReadAt`, for streaming parsers.
pub struct Cursor<R> {
    inner: R,
    pos: u64,
}

impl<R: ReadAt> Cursor<R> {
    /// Start at offset zero.
    pub fn new(inner: R) -> Self {
        Cursor { inner, pos: 0 }
    }
    /// Current offset.
    pub fn position(&self) -> u64 {
        self.pos
    }
    /// Jump to an offset.
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }
    /// Read the next chunk.
    pub fn read(&mut self, buf: &mut [u8]) -> FsResult<usize> {
        let n = self.inner.read_at(self.pos, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
    /// Remaining bytes.
    pub fn remaining(&self) -> u64 {
        self.inner.len().saturating_sub(self.pos)
    }
    /// The underlying reader.
    pub fn inner(&self) -> &R {
        &self.inner
    }
}

/// One directory entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// Name within its directory.
    pub name: String,
    /// True for a directory.
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub size: u64,
}

/// A writable file.
pub trait WriteFile {
    /// Append bytes.
    fn write_all(&mut self, data: &[u8]) -> FsResult<()>;
    /// Flush to the medium.
    fn flush(&mut self) -> FsResult<()>;
}

/// The small filesystem surface Quire needs. Paths are `/`-separated, absolute within
/// the volume, and never contain `..`.
pub trait Fs {
    /// An open file for reading.
    type File: ReadAt;
    /// An open file for writing.
    type Writer: WriteFile;
    /// Open for reading.
    fn open(&self, path: &str) -> FsResult<Self::File>;
    /// Create or truncate for writing.
    fn create(&self, path: &str) -> FsResult<Self::Writer>;
    /// Whether a path exists.
    fn exists(&self, path: &str) -> bool;
    /// List a directory.
    fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>>;
    /// Create a directory and its parents.
    fn mkdir_all(&self, path: &str) -> FsResult<()>;
    /// Remove a file.
    fn remove(&self, path: &str) -> FsResult<()>;
    /// Rename a file (used for atomic temp-then-rename writes).
    fn rename(&self, from: &str, to: &str) -> FsResult<()>;
    /// Free bytes on the volume, if known.
    fn free_bytes(&self) -> Option<u64>;

    /// Read a whole small file.
    fn read_to_vec(&self, path: &str) -> FsResult<Vec<u8>> {
        let f = self.open(path)?;
        let len = f.len() as usize;
        f.read_range(0, len)
    }
    /// Write a whole file atomically: to `path.tmp`, then rename over `path`.
    fn write_atomic(&self, path: &str, data: &[u8]) -> FsResult<()> {
        let tmp = alloc::format!("{path}.tmp");
        {
            let mut w = self.create(&tmp)?;
            w.write_all(data)?;
            w.flush()?;
        }
        if self.exists(path) {
            let _ = self.remove(path);
        }
        self.rename(&tmp, path)
    }
}

/// Join two path components.
pub fn join(a: &str, b: &str) -> String {
    let mut s = String::from(a.trim_end_matches('/'));
    s.push('/');
    s.push_str(b.trim_start_matches('/'));
    s
}

/// The file name part of a path.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The lower-cased extension, without the dot.
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// The directory part of a path ("" for a bare name).
pub fn parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Resolve `href` relative to the directory of `base` (for EPUB manifests), normalising
/// `.` and `..` segments and stripping any fragment.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = percent_decode(href);
    let mut parts: Vec<&str> = if href.starts_with('/') { Vec::new() } else { parent(base).split('/').filter(|s| !s.is_empty()).collect() };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Decode `%XX` escapes (EPUB hrefs are URL-encoded).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(a), Some(c)) = (h(b[i + 1]), h(b[i + 2])) {
                out.push((a * 16 + c) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| String::from(s))
}

#[cfg(feature = "std")]
pub mod host {
    //! A `std` filesystem rooted at a directory, for tests and the simulator.
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};

    /// A filesystem rooted at a directory.
    #[derive(Clone, Debug)]
    pub struct HostFs {
        root: PathBuf,
    }

    impl HostFs {
        /// Root at a directory (created if missing).
        pub fn new(root: impl AsRef<Path>) -> Self {
            let _ = std::fs::create_dir_all(root.as_ref());
            HostFs { root: root.as_ref().to_path_buf() }
        }
        fn full(&self, p: &str) -> PathBuf {
            self.root.join(p.trim_start_matches('/'))
        }
    }

    /// An open host file.
    pub struct HostFile {
        f: std::cell::RefCell<std::fs::File>,
        len: u64,
    }

    impl ReadAt for HostFile {
        fn len(&self) -> u64 {
            self.len
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
            let mut f = self.f.borrow_mut();
            f.seek(SeekFrom::Start(offset)).map_err(|e| FsError::Io(e.to_string()))?;
            f.read(buf).map_err(|e| FsError::Io(e.to_string()))
        }
    }

    /// A host file open for writing.
    pub struct HostWriter(std::io::BufWriter<std::fs::File>);
    impl WriteFile for HostWriter {
        fn write_all(&mut self, data: &[u8]) -> FsResult<()> {
            self.0.write_all(data).map_err(|e| FsError::Io(e.to_string()))
        }
        fn flush(&mut self) -> FsResult<()> {
            self.0.flush().map_err(|e| FsError::Io(e.to_string()))
        }
    }

    fn map(e: std::io::Error) -> FsError {
        match e.kind() {
            std::io::ErrorKind::NotFound => FsError::NotFound,
            _ => FsError::Io(e.to_string()),
        }
    }

    impl Fs for HostFs {
        type File = HostFile;
        type Writer = HostWriter;
        fn open(&self, path: &str) -> FsResult<HostFile> {
            let f = std::fs::File::open(self.full(path)).map_err(map)?;
            let len = f.metadata().map_err(map)?.len();
            Ok(HostFile { f: std::cell::RefCell::new(f), len })
        }
        fn create(&self, path: &str) -> FsResult<HostWriter> {
            let p = self.full(path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(map)?;
            }
            Ok(HostWriter(std::io::BufWriter::new(std::fs::File::create(p).map_err(map)?)))
        }
        fn exists(&self, path: &str) -> bool {
            self.full(path).exists()
        }
        fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>> {
            let mut out = Vec::new();
            for e in std::fs::read_dir(self.full(path)).map_err(map)? {
                let e = e.map_err(map)?;
                let md = e.metadata().map_err(map)?;
                out.push(DirEntry { name: e.file_name().to_string_lossy().into_owned(), is_dir: md.is_dir(), size: md.len() });
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(out)
        }
        fn mkdir_all(&self, path: &str) -> FsResult<()> {
            std::fs::create_dir_all(self.full(path)).map_err(map)
        }
        fn remove(&self, path: &str) -> FsResult<()> {
            let p = self.full(path);
            if p.is_dir() {
                std::fs::remove_dir_all(p).map_err(map)
            } else {
                std::fs::remove_file(p).map_err(map)
            }
        }
        fn rename(&self, from: &str, to: &str) -> FsResult<()> {
            std::fs::rename(self.full(from), self.full(to)).map_err(map)
        }
        fn free_bytes(&self) -> Option<u64> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_and_cursor() {
        let data: Vec<u8> = (0..100u8).collect();
        let s = Slice::new(&data, 10, 20);
        assert_eq!(s.len(), 20);
        let mut buf = [0u8; 8];
        assert_eq!(s.read_at(15, &mut buf).unwrap(), 5);
        assert_eq!(&buf[..5], &[25, 26, 27, 28, 29]);
        assert_eq!(s.read_at(20, &mut buf).unwrap(), 0);
        let mut c = Cursor::new(&s);
        let mut all = Vec::new();
        loop {
            let n = c.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        assert_eq!(all.len(), 20);
        assert_eq!(all[0], 10);
    }

    #[test]
    fn path_helpers() {
        assert_eq!(resolve("OPS/package.opf", "chapter_001.xhtml"), "OPS/chapter_001.xhtml");
        assert_eq!(resolve("OPS/text/ch1.xhtml", "../images/a%20b.jpg#frag"), "OPS/images/a b.jpg");
        assert_eq!(resolve("a.opf", "/x/y.xhtml"), "x/y.xhtml");
        assert_eq!(extension("Books/Some.Book.EPUB"), "epub");
        assert_eq!(file_name("/a/b/c.txt"), "c.txt");
        assert_eq!(parent("/a/b/c.txt"), "/a/b");
        assert_eq!(join("/books/", "/x.epub"), "/books/x.epub");
    }
}
