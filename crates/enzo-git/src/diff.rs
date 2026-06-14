//! Diff types and rendering.

use serde::{Deserialize, Serialize};

/// A unified diff for one file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDiff {
    /// Old path (pre-change).
    pub old_path: String,
    /// New path (post-change).
    pub new_path: String,
    /// Lines added.
    pub additions: usize,
    /// Lines removed.
    pub deletions: usize,
    /// `true` if the file is binary (hunks are empty).
    pub binary: bool,
    /// The hunks comprising the diff.
    pub hunks: Vec<DiffHunk>,
}

/// One hunk of a unified diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffHunk {
    /// The `@@ -a,b +c,d @@` header.
    pub header: String,
    /// Raw hunk lines including leading ` `/`+`/`-` markers.
    pub lines: Vec<String>,
}
