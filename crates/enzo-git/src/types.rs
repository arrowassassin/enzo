//! Serializable data types returned by [`crate::Repo`] operations.

use serde::{Deserialize, Serialize};

/// One entry in `git status` — a path with its working-tree / index state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusEntry {
    /// Path relative to the repository root.
    pub path: String,
    /// Compact state string, e.g. `"M"`, `"A"`, `"D"`, `"??"`, `"R"`.
    pub state: String,
    /// `true` if the change is staged (present in the index).
    pub staged: bool,
    /// `true` for an untracked file.
    pub untracked: bool,
}

/// Summary of the repository's current position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoInfo {
    /// Absolute path to the working directory.
    pub workdir: String,
    /// Current branch name (or detached HEAD short id).
    pub head: String,
    /// `true` if HEAD is detached.
    pub detached: bool,
    /// Commits ahead of the upstream branch.
    pub ahead: usize,
    /// Commits behind the upstream branch.
    pub behind: usize,
    /// `true` if the working tree has uncommitted changes.
    pub dirty: bool,
}

/// One branch (local or remote).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchInfo {
    /// Branch short name.
    pub name: String,
    /// `true` if this is the current HEAD branch.
    pub is_head: bool,
    /// `true` if this is a remote-tracking branch.
    pub is_remote: bool,
    /// Upstream branch name, if configured.
    pub upstream: Option<String>,
}

/// One commit in the log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitInfo {
    /// Full 40-char commit id.
    pub id: String,
    /// First 8 chars of the id.
    pub short_id: String,
    /// Commit summary (first line of the message).
    pub summary: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Commit time as a Unix timestamp (seconds).
    pub time: i64,
}

/// One linked worktree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Worktree name.
    pub name: String,
    /// Absolute path to the worktree.
    pub path: String,
    /// `true` if the worktree is locked.
    pub locked: bool,
}
