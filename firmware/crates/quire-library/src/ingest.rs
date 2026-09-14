//! Turning a source file into a book cache and a complete index entry.

use quire_fs::Fs;

use crate::cache::CacheSink;
use crate::index::IngestState;
use crate::{book_dir, BookId, LibError, LibResult, Library};

/// Ingest one pending book. On failure the entry is marked Failed with a message and the
/// partial cache is removed; the library index is left for the caller to save.
pub fn ingest_book<F: Fs>(fs: &F, lib: &mut Library, id: BookId, progress: &mut dyn FnMut(u32, u32)) -> LibResult<()> {
    let (path, format) = {
        let e = lib.get(id).ok_or(LibError::NotFound)?;
        (e.path.clone(), e.format)
    };
    let dir = book_dir(id);
    let result = (|| -> LibResult<crate::cache::CacheSummary> {
        let file = fs.open(&path)?;
        let mut sink = CacheSink::new(fs, &dir)?;
        let mut reporter = ProgressSink { inner: &mut sink, cb: progress };
        quire_doc::ingest(format, &file, &path, &mut reporter)?;
        sink.finish()
    })();
    match result {
        Ok(summary) => {
            if let Some(e) = lib.get_mut(id) {
                if !summary.meta.title.trim().is_empty() {
                    e.title = summary.meta.title.trim().into();
                }
                e.authors = summary.meta.authors.clone();
                e.series = summary.meta.series.clone().map(|s| (s, summary.meta.series_index.unwrap_or(0)));
                e.year = summary.meta.year;
                if !summary.meta.language.is_empty() {
                    e.language = summary.meta.language.clone();
                }
                e.subjects = summary.meta.subjects.clone();
                e.sections = summary.sections.len() as u16;
                e.chars = summary.chars();
                e.has_cover = summary.has_cover;
                e.ingest = IngestState::Ready;
                e.error = None;
                if e.loc.chars > e.chars {
                    e.loc = Default::default();
                }
            }
            Ok(())
        }
        Err(err) => {
            let _ = crate::cache::clear_dir(fs, &dir, 0);
            let _ = fs.remove(&dir);
            if let Some(e) = lib.get_mut(id) {
                e.ingest = IngestState::Failed;
                e.error = Some(alloc::format!("{err}"));
            }
            Err(err)
        }
    }
}

/// Remove a book's cache and its index entry.
pub fn forget_book<F: Fs>(fs: &F, lib: &mut Library, id: BookId) {
    let dir = book_dir(id);
    if fs.exists(&dir) {
        let _ = crate::cache::clear_dir(fs, &dir, 0);
        let _ = fs.remove(&dir);
    }
    lib.remove(id);
}

/// Bytes used by a book's cache.
pub fn cache_size<F: Fs>(fs: &F, id: BookId) -> u64 {
    fn walk<F: Fs>(fs: &F, dir: &str, depth: u32) -> u64 {
        if depth > 4 {
            return 0;
        }
        fs.read_dir(dir)
            .map(|es| es.iter().map(|e| if e.is_dir { walk(fs, &quire_fs::join(dir, &e.name), depth + 1) } else { e.size }).sum())
            .unwrap_or(0)
    }
    walk(fs, &book_dir(id), 0)
}

/// A sink wrapper forwarding progress to a callback.
struct ProgressSink<'a, S: quire_doc::Sink> {
    inner: &'a mut S,
    cb: &'a mut dyn FnMut(u32, u32),
}

impl<S: quire_doc::Sink> quire_doc::Sink for ProgressSink<'_, S> {
    fn begin_chapter(&mut self, index: u16, title: Option<&str>) -> Result<(), quire_doc::DocError> {
        self.inner.begin_chapter(index, title)
    }
    fn chapter_bytes(&mut self, data: &[u8]) -> Result<(), quire_doc::DocError> {
        self.inner.chapter_bytes(data)
    }
    fn end_chapter(&mut self, chars: u32) -> Result<(), quire_doc::DocError> {
        self.inner.end_chapter(chars)
    }
    fn image(&mut self, bitmap: &quire_gfx::Bitmap) -> Result<u16, quire_doc::DocError> {
        self.inner.image(bitmap)
    }
    fn cover(&mut self, full: &quire_gfx::Bitmap, thumb: &quire_gfx::Bitmap) -> Result<(), quire_doc::DocError> {
        self.inner.cover(full, thumb)
    }
    fn metadata(&mut self, meta: &quire_doc::Metadata) -> Result<(), quire_doc::DocError> {
        self.inner.metadata(meta)
    }
    fn toc(&mut self, toc: &[quire_doc::TocEntry]) -> Result<(), quire_doc::DocError> {
        self.inner.toc(toc)
    }
    fn progress(&mut self, done: u32, total: u32) {
        (self.cb)(done, total);
        self.inner.progress(done, total);
    }
}
