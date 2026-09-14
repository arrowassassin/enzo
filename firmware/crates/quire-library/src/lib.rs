//! Quire's library: everything that lives under `/.quire/` on the card.
//!
//! - [`Library`]: the book index (`library.bin`), scanning of source folders, ingest of
//!   new books into the chapter cache, positions, collections.
//! - [`cache`]: the per-book cache layout (`/.quire/books/<id>/`) and the ingest sink.
//! - [`Book`]: an open book — chapters, images, TOC, page indexes per typography profile.
//! - [`marks`]: bookmarks, highlights and notes.
//! - [`stats`]: the append-only session log, the daily index and every number the
//!   Analytics screens show.
//!
//! All state is small and bounded so a library of thousands of books costs a few tens of
//! kilobytes of RAM; the heavy data stays on the card.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod book;
pub mod cache;
pub mod id;
pub mod index;
pub mod ingest;
pub mod marks;
pub mod pages;
pub mod scan;
pub mod stats;
pub mod time;

pub use book::Book;
pub use id::BookId;
pub use index::{BookEntry, Collection, IngestState, Library, Loc, Status};
pub use ingest::{forget_book, ingest_book};
pub use scan::scan;
pub use stats::{Session, SessionTracker, Stats};

use alloc::string::String;

/// Root of everything Quire writes on the card.
pub const ROOT: &str = "/.quire";
/// Where book caches live.
pub const BOOKS_DIR: &str = "/.quire/books";
/// The library index file.
pub const INDEX_FILE: &str = "/.quire/library.bin";
/// Statistics directory.
pub const STATS_DIR: &str = "/.quire/stats";
/// Default folders scanned for books, in order.
pub const DEFAULT_SOURCES: &[&str] = &["/Books", "/books", "/"];

/// Errors from library operations.
#[derive(Debug)]
pub enum LibError {
    /// Card IO.
    Fs(quire_fs::FsError),
    /// A document could not be ingested.
    Doc(quire_doc::DocError),
    /// A cache or index file is corrupt.
    Corrupt(&'static str),
    /// No such book.
    NotFound,
}

impl From<quire_fs::FsError> for LibError {
    fn from(e: quire_fs::FsError) -> Self {
        LibError::Fs(e)
    }
}
impl From<quire_doc::DocError> for LibError {
    fn from(e: quire_doc::DocError) -> Self {
        match e {
            quire_doc::DocError::Fs(f) => LibError::Fs(f),
            other => LibError::Doc(other),
        }
    }
}
impl core::fmt::Display for LibError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LibError::Fs(e) => write!(f, "card: {e:?}"),
            LibError::Doc(e) => write!(f, "document: {e}"),
            LibError::Corrupt(what) => write!(f, "corrupt {what}"),
            LibError::NotFound => write!(f, "not found"),
        }
    }
}

/// Result alias.
pub type LibResult<T> = Result<T, LibError>;

/// Path of a book's cache directory.
pub fn book_dir(id: BookId) -> String {
    alloc::format!("{BOOKS_DIR}/{id}")
}
