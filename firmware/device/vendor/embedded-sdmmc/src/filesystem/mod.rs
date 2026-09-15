//! Generic File System structures
//!
//! Implements generic file system components. These should be applicable to
//! most (if not all) supported filesystems.

/// Maximum file size supported by this library
pub const MAX_FILE_SIZE: u32 = u32::MAX;

mod attributes;
mod cluster;
mod directory;
mod filename;
mod files;
mod handles;
pub(crate) mod longname;
mod timestamp;

pub use self::attributes::Attributes;
pub use self::cluster::ClusterId;
pub use self::directory::{DirEntry, Directory, RawDirectory};
pub use self::filename::{FilenameError, LfnBuffer, ShortFileName, ToShortFileName};
pub use self::files::{File, FileError, Mode, RawFile};
pub use self::handles::{Handle, HandleGenerator};
pub use self::longname::{LFN_UNITS_PER_ENTRY, LongName, MAX_LFN_ENTRIES, MAX_LFN_UNITS, ShortNameBasis, eq_ignore_case};
pub use self::timestamp::{TimeSource, Timestamp};

pub(crate) use self::directory::DirectoryInfo;
pub(crate) use self::files::FileInfo;

// ****************************************************************************
//
// End Of File
//
// ****************************************************************************
