//! The card as an object-safe handle, so the network tasks (which cannot be generic) share
//! the main loop's filesystem without this crate depending on the board crate.

use alloc::boxed::Box;
use alloc::vec::Vec;
use quire_fs::{DirEntry, Fs, FsResult, ReadAt, WriteFile};

/// A filesystem behind a trait object. Implemented for every [`Fs`].
pub trait CardFs {
    /// Open for reading.
    fn open(&self, path: &str) -> FsResult<Box<dyn ReadAt>>;
    /// Create or truncate.
    fn create(&self, path: &str) -> FsResult<Box<dyn WriteFile>>;
    /// Open for appending.
    fn append(&self, path: &str) -> FsResult<Box<dyn WriteFile>>;
    /// Whether the path exists.
    fn exists(&self, path: &str) -> bool;
    /// List a directory.
    fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>>;
    /// Create a directory and its parents.
    fn mkdir_all(&self, path: &str) -> FsResult<()>;
    /// Remove a file.
    fn remove(&self, path: &str) -> FsResult<()>;
    /// Rename.
    fn rename(&self, from: &str, to: &str) -> FsResult<()>;
    /// Free bytes.
    fn free_bytes(&self) -> Option<u64>;
}

impl<F: Fs> CardFs for F
where
    F::File: 'static,
    F::Writer: 'static,
{
    fn open(&self, path: &str) -> FsResult<Box<dyn ReadAt>> {
        Ok(Box::new(Fs::open(self, path)?))
    }
    fn create(&self, path: &str) -> FsResult<Box<dyn WriteFile>> {
        Ok(Box::new(Fs::create(self, path)?))
    }
    fn append(&self, path: &str) -> FsResult<Box<dyn WriteFile>> {
        Ok(Box::new(Fs::append(self, path)?))
    }
    fn exists(&self, path: &str) -> bool {
        Fs::exists(self, path)
    }
    fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>> {
        Fs::read_dir(self, path)
    }
    fn mkdir_all(&self, path: &str) -> FsResult<()> {
        Fs::mkdir_all(self, path)
    }
    fn remove(&self, path: &str) -> FsResult<()> {
        Fs::remove(self, path)
    }
    fn rename(&self, from: &str, to: &str) -> FsResult<()> {
        Fs::rename(self, from, to)
    }
    fn free_bytes(&self) -> Option<u64> {
        Fs::free_bytes(self)
    }
}

/// A boxed reader.
pub struct DynFile(pub Box<dyn ReadAt>);

impl ReadAt for DynFile {
    fn len(&self) -> u64 {
        self.0.len()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        self.0.read_at(offset, buf)
    }
}

/// A boxed writer.
pub struct DynWriter(pub Box<dyn WriteFile>);

impl WriteFile for DynWriter {
    fn write_all(&mut self, data: &[u8]) -> FsResult<()> {
        self.0.write_all(data)
    }
    fn flush(&mut self) -> FsResult<()> {
        self.0.flush()
    }
}

/// A [`CardFs`] as an [`Fs`], for the library, settings and stats loaders.
#[derive(Clone, Copy)]
pub struct DynFs<'a>(pub &'a dyn CardFs);

impl Fs for DynFs<'_> {
    type File = DynFile;
    type Writer = DynWriter;
    fn open(&self, path: &str) -> FsResult<DynFile> {
        self.0.open(path).map(DynFile)
    }
    fn create(&self, path: &str) -> FsResult<DynWriter> {
        self.0.create(path).map(DynWriter)
    }
    fn append(&self, path: &str) -> FsResult<DynWriter> {
        self.0.append(path).map(DynWriter)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn read_dir(&self, path: &str) -> FsResult<Vec<DirEntry>> {
        self.0.read_dir(path)
    }
    fn mkdir_all(&self, path: &str) -> FsResult<()> {
        self.0.mkdir_all(path)
    }
    fn remove(&self, path: &str) -> FsResult<()> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> FsResult<()> {
        self.0.rename(from, to)
    }
    fn free_bytes(&self) -> Option<u64> {
        self.0.free_bytes()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn dyn_round_trip() {
        let dir = std::env::temp_dir().join(alloc::format!("quire-net-fs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let host = quire_fs::host::HostFs::new(&dir);
        let dynfs = DynFs(&host);
        dynfs.write_atomic("/a.txt", b"hello").unwrap();
        assert_eq!(dynfs.read_to_vec("/a.txt").unwrap(), b"hello");
        assert!(Fs::exists(&dynfs, "/a.txt"));
        Fs::rename(&dynfs, "/a.txt", "/b.txt").unwrap();
        assert!(!Fs::exists(&dynfs, "/a.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
