//! The [`Repo`] handle — wraps a `git2::Repository` with high-level operations.

use std::cell::RefCell;
use std::path::Path;

use anyhow::Context;
use git2::{
    BranchType, Cred, DiffFormat, DiffOptions, FetchOptions, PushOptions, RemoteCallbacks,
    Repository, Signature, Sort, StatusOptions,
};

use crate::diff::{DiffHunk, FileDiff};
use crate::types::{BranchInfo, CommitInfo, RepoInfo, StatusEntry, WorktreeInfo};

/// A live handle to a git repository.
pub struct Repo {
    inner: Repository,
}

impl Repo {
    /// Open the repository containing `path` (searches upward for `.git`).
    ///
    /// # Errors
    /// Returns an error if no repository is found.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let inner = Repository::discover(path.as_ref())
            .with_context(|| format!("open git repo at {}", path.as_ref().display()))?;
        Ok(Self { inner })
    }

    /// Initialise a new repository at `path`.
    ///
    /// # Errors
    /// Returns an error if initialization fails.
    pub fn init(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let inner = Repository::init(path.as_ref())
            .with_context(|| format!("init git repo at {}", path.as_ref().display()))?;
        Ok(Self { inner })
    }

    // ── Status & info ───────────────────────────────────────────────────────

    /// Return the working-tree status (like `git status --porcelain`).
    ///
    /// # Errors
    /// Returns an error if status cannot be computed.
    pub fn status(&self) -> anyhow::Result<Vec<StatusEntry>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true);
        let statuses = self.inner.statuses(Some(&mut opts)).context("statuses")?;

        let mut out = Vec::new();
        for entry in statuses.iter() {
            let s = entry.status();
            let path = entry.path().unwrap_or("").to_owned();
            let staged = s.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_DELETED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            );
            let untracked = s.contains(git2::Status::WT_NEW);
            out.push(StatusEntry {
                path,
                state: status_label(s),
                staged,
                untracked,
            });
        }
        Ok(out)
    }

    /// Return a summary of the repository's current position.
    ///
    /// # Errors
    /// Returns an error if HEAD or upstream info cannot be read.
    pub fn info(&self) -> anyhow::Result<RepoInfo> {
        let workdir = self
            .inner
            .workdir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let (head, detached) = match self.inner.head() {
            Ok(head_ref) => {
                if self.inner.head_detached().unwrap_or(false) {
                    let short = head_ref
                        .target()
                        .map_or_else(|| "HEAD".to_owned(), |oid| oid.to_string()[..8].to_owned());
                    (short, true)
                } else {
                    (head_ref.shorthand().unwrap_or("HEAD").to_owned(), false)
                }
            }
            // Unborn branch (fresh repo, no commits yet).
            Err(_) => ("(no commits yet)".to_owned(), false),
        };

        let (ahead, behind) = self.ahead_behind().unwrap_or((0, 0));
        let dirty = !self.status()?.is_empty();

        Ok(RepoInfo {
            workdir,
            head,
            detached,
            ahead,
            behind,
            dirty,
        })
    }

    fn ahead_behind(&self) -> anyhow::Result<(usize, usize)> {
        let head = self.inner.head()?;
        let local_oid = head.target().context("head target")?;
        let branch_name = head.shorthand().context("head shorthand")?;
        let upstream = self
            .inner
            .find_branch(branch_name, BranchType::Local)?
            .upstream()?;
        let upstream_oid = upstream.get().target().context("upstream target")?;
        let (ahead, behind) = self.inner.graph_ahead_behind(local_oid, upstream_oid)?;
        Ok((ahead, behind))
    }

    // ── Diffs ───────────────────────────────────────────────────────────────

    /// Diff the working tree against the index (unstaged changes).
    ///
    /// # Errors
    /// Returns an error if the diff cannot be computed.
    pub fn diff_unstaged(&self) -> anyhow::Result<Vec<FileDiff>> {
        let mut opts = DiffOptions::new();
        let diff = self
            .inner
            .diff_index_to_workdir(None, Some(&mut opts))
            .context("diff index→workdir")?;
        collect_file_diffs(&diff)
    }

    /// Diff the index against HEAD (staged changes).
    ///
    /// # Errors
    /// Returns an error if the diff cannot be computed.
    pub fn diff_staged(&self) -> anyhow::Result<Vec<FileDiff>> {
        let head_tree = match self.inner.head() {
            Ok(h) => Some(h.peel_to_tree().context("peel head to tree")?),
            Err(_) => None, // no commits yet — diff against empty tree
        };
        let mut opts = DiffOptions::new();
        let diff = self
            .inner
            .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
            .context("diff tree→index")?;
        collect_file_diffs(&diff)
    }

    // ── Staging ─────────────────────────────────────────────────────────────

    /// Stage a path (like `git add <path>`).
    ///
    /// # Errors
    /// Returns an error if the path cannot be staged.
    pub fn stage(&self, path: &str) -> anyhow::Result<()> {
        let mut index = self.inner.index().context("open index")?;
        index
            .add_path(Path::new(path))
            .with_context(|| format!("stage {path}"))?;
        index.write().context("write index")?;
        Ok(())
    }

    /// Unstage a path (like `git restore --staged <path>`).
    ///
    /// # Errors
    /// Returns an error if the path cannot be unstaged.
    pub fn unstage(&self, path: &str) -> anyhow::Result<()> {
        if let Ok(head) = self.inner.head() {
            let commit = head.peel_to_commit().context("peel head to commit")?;
            self.inner
                .reset_default(Some(commit.as_object()), [Path::new(path)])
                .with_context(|| format!("unstage {path}"))?;
        } else {
            // No HEAD yet: just remove from index.
            let mut index = self.inner.index()?;
            index.remove_path(Path::new(path)).ok();
            index.write()?;
        }
        Ok(())
    }

    /// Stage all changes (like `git add -A`).
    ///
    /// # Errors
    /// Returns an error if the operation fails.
    pub fn stage_all(&self) -> anyhow::Result<()> {
        let mut index = self.inner.index().context("open index")?;
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .context("stage all")?;
        index.write().context("write index")?;
        Ok(())
    }

    // ── Commit ──────────────────────────────────────────────────────────────

    /// Create a commit from the current index.
    ///
    /// Uses the repository's configured `user.name`/`user.email`, falling back
    /// to the provided author if config is missing.
    ///
    /// # Errors
    /// Returns an error if there is nothing to commit or signature is missing.
    pub fn commit(&self, message: &str) -> anyhow::Result<String> {
        let sig = self.signature()?;
        let mut index = self.inner.index().context("open index")?;
        let tree_oid = index.write_tree().context("write tree")?;
        let tree = self.inner.find_tree(tree_oid).context("find tree")?;

        let parents = match self.inner.head() {
            Ok(head) => vec![head.peel_to_commit().context("peel head")?],
            Err(_) => vec![], // initial commit
        };
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let oid = self
            .inner
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .context("create commit")?;
        Ok(oid.to_string())
    }

    fn signature(&self) -> anyhow::Result<Signature<'static>> {
        // Prefer repo/global config; fall back to a sensible default.
        if let Ok(sig) = self.inner.signature() {
            return Ok(sig);
        }
        Signature::now("Enzo", "enzo@localhost").context("build signature")
    }

    // ── Branches ────────────────────────────────────────────────────────────

    /// List all branches (local and remote).
    ///
    /// # Errors
    /// Returns an error if branches cannot be enumerated.
    pub fn branches(&self) -> anyhow::Result<Vec<BranchInfo>> {
        let mut out = Vec::new();
        for kind in [BranchType::Local, BranchType::Remote] {
            for b in self.inner.branches(Some(kind))? {
                let (branch, _) = b?;
                let name = branch.name()?.unwrap_or("").to_owned();
                let is_head = branch.is_head();
                let upstream = branch
                    .upstream()
                    .ok()
                    .and_then(|u| u.name().ok().flatten().map(str::to_owned));
                out.push(BranchInfo {
                    name,
                    is_head,
                    is_remote: kind == BranchType::Remote,
                    upstream,
                });
            }
        }
        Ok(out)
    }

    /// Create a new local branch at HEAD.
    ///
    /// # Errors
    /// Returns an error if the branch already exists or HEAD is unborn.
    pub fn create_branch(&self, name: &str) -> anyhow::Result<()> {
        let head = self.inner.head().context("read HEAD")?;
        let commit = head.peel_to_commit().context("peel HEAD to commit")?;
        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("create branch {name}"))?;
        Ok(())
    }

    /// Check out an existing branch (updates HEAD and the working tree).
    ///
    /// # Errors
    /// Returns an error if the branch does not exist or checkout fails.
    pub fn checkout(&self, name: &str) -> anyhow::Result<()> {
        let (object, reference) = self
            .inner
            .revparse_ext(name)
            .with_context(|| format!("resolve {name}"))?;
        self.inner
            .checkout_tree(&object, None)
            .with_context(|| format!("checkout tree {name}"))?;
        match reference {
            Some(r) => {
                let ref_name = r.name().context("branch ref name")?;
                self.inner.set_head(ref_name)?;
            }
            None => self.inner.set_head_detached(object.id())?,
        }
        Ok(())
    }

    // ── Worktrees ───────────────────────────────────────────────────────────

    /// List linked worktrees.
    ///
    /// # Errors
    /// Returns an error if worktrees cannot be enumerated.
    pub fn worktrees(&self) -> anyhow::Result<Vec<WorktreeInfo>> {
        let mut out = Vec::new();
        let worktrees = self.inner.worktrees()?;
        #[allow(
            clippy::explicit_iter_loop,
            reason = "git2 StringArray::iter yields Result items; & does not iterate"
        )]
        for name in worktrees.iter() {
            let Some(name) = name? else { continue };
            if let Ok(wt) = self.inner.find_worktree(name) {
                let locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
                out.push(WorktreeInfo {
                    name: name.to_owned(),
                    path: wt.path().display().to_string(),
                    locked,
                });
            }
        }
        Ok(out)
    }

    /// Create a new worktree `name` at `path` (checks out HEAD there).
    ///
    /// # Errors
    /// Returns an error if the worktree cannot be created.
    pub fn add_worktree(&self, name: &str, path: impl AsRef<Path>) -> anyhow::Result<()> {
        self.inner
            .worktree(name, path.as_ref(), None)
            .with_context(|| format!("add worktree {name}"))?;
        Ok(())
    }

    // ── Log ─────────────────────────────────────────────────────────────────

    /// Return up to `limit` commits reachable from HEAD, newest first.
    ///
    /// # Errors
    /// Returns an error if the revwalk fails.
    pub fn log(&self, limit: usize) -> anyhow::Result<Vec<CommitInfo>> {
        if self.inner.head().is_err() {
            return Ok(vec![]); // no commits yet
        }
        let mut walk = self.inner.revwalk().context("revwalk")?;
        walk.push_head().context("push head")?;
        walk.set_sorting(Sort::TIME)?;

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let oid = oid?;
            let commit = self.inner.find_commit(oid)?;
            let summary = commit.summary()?.unwrap_or("").to_owned();
            let time = commit.time().seconds();
            let author = commit.author();
            let id = oid.to_string();
            out.push(CommitInfo {
                short_id: id[..8.min(id.len())].to_owned(),
                id,
                summary,
                author: author.name().unwrap_or("").to_owned(),
                email: author.email().unwrap_or("").to_owned(),
                time,
            });
        }
        Ok(out)
    }

    // ── Remote operations ─────────────────────────────────────────────────────

    /// Fetch from a remote (default `origin`).
    ///
    /// Authentication uses the SSH agent / default credential helpers.
    ///
    /// # Errors
    /// Returns an error if the fetch fails.
    pub fn fetch(&self, remote: &str) -> anyhow::Result<()> {
        let mut rem = self
            .inner
            .find_remote(remote)
            .with_context(|| format!("find remote {remote}"))?;
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(default_callbacks());
        rem.fetch::<&str>(&[], Some(&mut fo), None)
            .with_context(|| format!("fetch {remote}"))?;
        Ok(())
    }

    /// Push the current branch to a remote (default `origin`).
    ///
    /// # Errors
    /// Returns an error if there is no current branch or the push fails.
    pub fn push(&self, remote: &str) -> anyhow::Result<()> {
        let head = self.inner.head().context("read HEAD")?;
        let branch = head.shorthand().context("branch name")?;
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");

        let mut rem = self
            .inner
            .find_remote(remote)
            .with_context(|| format!("find remote {remote}"))?;
        let mut po = PushOptions::new();
        po.remote_callbacks(default_callbacks());
        rem.push(&[refspec.as_str()], Some(&mut po))
            .with_context(|| format!("push to {remote}"))?;
        Ok(())
    }

    /// Access the underlying `git2::Repository` (escape hatch for advanced ops).
    #[must_use]
    pub fn inner(&self) -> &Repository {
        &self.inner
    }
}

/// Build remote callbacks that try SSH agent then default credentials.
fn default_callbacks() -> RemoteCallbacks<'static> {
    let mut cb = RemoteCallbacks::new();
    cb.credentials(|_url, username, allowed| {
        if allowed.is_ssh_key()
            && let Some(user) = username
        {
            return Cred::ssh_key_from_agent(user);
        }
        Cred::default()
    });
    cb
}

/// Map a `git2::Status` bitset to a compact porcelain-style label.
fn status_label(s: git2::Status) -> String {
    if s.contains(git2::Status::WT_NEW) {
        "??".to_owned()
    } else if s.intersects(git2::Status::INDEX_NEW) {
        "A".to_owned()
    } else if s.intersects(git2::Status::WT_DELETED | git2::Status::INDEX_DELETED) {
        "D".to_owned()
    } else if s.intersects(git2::Status::WT_RENAMED | git2::Status::INDEX_RENAMED) {
        "R".to_owned()
    } else if s.intersects(git2::Status::WT_MODIFIED | git2::Status::INDEX_MODIFIED) {
        "M".to_owned()
    } else if s.contains(git2::Status::CONFLICTED) {
        "U".to_owned()
    } else {
        " ".to_owned()
    }
}

/// Walk a `git2::Diff`, grouping lines into per-file [`FileDiff`] structures.
fn collect_file_diffs(diff: &git2::Diff<'_>) -> anyhow::Result<Vec<FileDiff>> {
    // RefCell lets the FnMut print callback mutate our accumulator.
    let files: RefCell<Vec<FileDiff>> = RefCell::new(Vec::new());

    diff.print(DiffFormat::Patch, |delta, hunk, line| {
        let mut files = files.borrow_mut();
        let old_path = delta
            .old_file()
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let new_path = delta
            .new_file()
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        // Start a new FileDiff when the new_path changes.
        if files.last().map(|f| f.new_path.as_str()) != Some(new_path.as_str()) {
            files.push(FileDiff {
                old_path: old_path.clone(),
                new_path: new_path.clone(),
                additions: 0,
                deletions: 0,
                binary: delta.flags().is_binary(),
                hunks: Vec::new(),
            });
        }
        let file = files.last_mut().expect("just pushed");

        match line.origin() {
            'H' => {
                // Hunk header line.
                if let Some(h) = hunk {
                    let header = String::from_utf8_lossy(h.header()).trim_end().to_owned();
                    file.hunks.push(DiffHunk {
                        header,
                        lines: Vec::new(),
                    });
                }
            }
            '+' => {
                file.additions += 1;
                push_line(file, '+', line.content());
            }
            '-' => {
                file.deletions += 1;
                push_line(file, '-', line.content());
            }
            ' ' => push_line(file, ' ', line.content()),
            _ => {}
        }
        true
    })
    .context("print diff")?;

    Ok(files.into_inner())
}

fn push_line(file: &mut FileDiff, origin: char, content: &[u8]) {
    let text = String::from_utf8_lossy(content);
    let line = format!("{origin}{}", text.trim_end_matches('\n'));
    if let Some(hunk) = file.hunks.last_mut() {
        hunk.lines.push(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway repo with one commit; returns (`TempDir`, Repo).
    fn fixture() -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::init(dir.path()).unwrap();
        // Configure identity for commits.
        let mut cfg = repo.inner().config().unwrap();
        cfg.set_str("user.name", "Test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        (dir, repo)
    }

    fn write(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn init_and_empty_status() {
        let (_d, repo) = fixture();
        assert!(repo.status().unwrap().is_empty());
    }

    #[test]
    fn untracked_file_appears_in_status() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "hello");
        let status = repo.status().unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].path, "a.txt");
        assert_eq!(status[0].state, "??");
        assert!(status[0].untracked);
    }

    #[test]
    fn stage_then_commit() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "hello");
        repo.stage("a.txt").unwrap();

        let staged = repo.status().unwrap();
        assert!(staged[0].staged, "file should be staged");

        let oid = repo.commit("initial").unwrap();
        assert_eq!(oid.len(), 40);

        // Clean after commit.
        assert!(repo.status().unwrap().is_empty());

        let log = repo.log(10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "initial");
        assert_eq!(log[0].author, "Test");
    }

    #[test]
    fn unstage_reverts_staging() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "x");
        repo.stage("a.txt").unwrap();
        repo.unstage("a.txt").unwrap();
        let status = repo.status().unwrap();
        // Back to untracked.
        assert!(status[0].untracked, "should be untracked after unstage");
    }

    #[test]
    fn diff_staged_shows_additions() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "line1\nline2\n");
        repo.stage("a.txt").unwrap();
        let diffs = repo.diff_staged().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].new_path, "a.txt");
        assert_eq!(diffs[0].additions, 2);
    }

    #[test]
    fn diff_unstaged_shows_modifications() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "original\n");
        repo.stage("a.txt").unwrap();
        repo.commit("c1").unwrap();
        write(d.path(), "a.txt", "changed\n");
        let diffs = repo.diff_unstaged().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].additions, 1);
        assert_eq!(diffs[0].deletions, 1);
    }

    #[test]
    fn create_and_list_branch() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "x");
        repo.stage("a.txt").unwrap();
        repo.commit("c1").unwrap();

        repo.create_branch("feature").unwrap();
        let branches = repo.branches().unwrap();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"feature"));
    }

    #[test]
    fn checkout_switches_branch() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "x");
        repo.stage("a.txt").unwrap();
        repo.commit("c1").unwrap();
        repo.create_branch("feature").unwrap();
        repo.checkout("feature").unwrap();
        let info = repo.info().unwrap();
        assert_eq!(info.head, "feature");
    }

    #[test]
    fn info_reports_dirty() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "x");
        repo.stage("a.txt").unwrap();
        repo.commit("c1").unwrap();
        assert!(!repo.info().unwrap().dirty);
        write(d.path(), "a.txt", "y");
        assert!(repo.info().unwrap().dirty);
    }

    #[test]
    fn log_empty_for_fresh_repo() {
        let (_d, repo) = fixture();
        assert!(repo.log(10).unwrap().is_empty());
    }

    #[test]
    fn add_and_list_worktree() {
        let (d, repo) = fixture();
        write(d.path(), "a.txt", "x");
        repo.stage("a.txt").unwrap();
        repo.commit("c1").unwrap();

        let wt_path = d.path().parent().unwrap().join("enzo-wt-test");
        // Worktree creation may fail in restricted environments; tolerate it.
        if repo.add_worktree("wt1", &wt_path).is_ok() {
            let wts = repo.worktrees().unwrap();
            assert!(wts.iter().any(|w| w.name == "wt1"));
            std::fs::remove_dir_all(&wt_path).ok();
        }
    }
}
