//! Git integration for Enzo — a thin, JSON-friendly wrapper over `libgit2`.
//!
//! Provides the operations a VS Code-style source-control panel needs:
//! status, diffs, staging, commit, push/pull, branch and worktree management,
//! and log. Everything returns plain serializable structs so the daemon can
//! forward them over ATP as `git.*` results.
//!
//! # Example
//! ```no_run
//! use enzo_git::Repo;
//! let repo = Repo::open(".").unwrap();
//! let status = repo.status().unwrap();
//! for entry in &status {
//!     println!("{} {}", entry.state, entry.path);
//! }
//! ```

mod diff;
mod repo;
mod types;

pub use diff::{DiffHunk, FileDiff};
pub use repo::Repo;
pub use types::{BranchInfo, CommitInfo, RepoInfo, StatusEntry, WorktreeInfo};
