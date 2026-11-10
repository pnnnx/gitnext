//! Git plumbing for gitnext (libgit2 via `git2`). Repositories stay 100%
//! git-compatible: GitHub, CI and plain `git` keep working.

use anyhow::{bail, Context, Result};
use git2::{IndexAddOption, Oid, Repository, RepositoryInitOptions, Signature, Status, StatusOptions};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// (path, contents before, contents after)
pub type ChangedFile = (String, Option<String>, Option<String>);

pub struct Repo {
    pub inner: Repository,
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusEntry {
    pub path: String,
    /// Staged change: A M D R T, or None.
    pub staged: Option<char>,
    /// Unstaged change in the working tree: M D T, '?' for untracked.
    pub unstaged: Option<char>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitInfo {
    pub id: String,
    pub short: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    pub message: String,
}

impl Repo {
    /// Find the repository containing `start` (walks up like `git`).
    pub fn discover(start: &Path) -> Result<Self> {
        let inner = Repository::discover(start).context("not a git repository (run `gitnext init`)")?;
        let root = inner
            .workdir()
            .context("bare repositories are not supported")?
            .to_path_buf();
        let root = root.canonicalize().unwrap_or(root);
        Ok(Repo { inner, root })
    }

    /// `git init` (no-op if `path` already is a repository root).
    pub fn init(path: &Path) -> Result<(Self, bool)> {
        if let Ok(r) = Repository::open(path) {
            let root = r.workdir().unwrap_or(path).to_path_buf();
            return Ok((Repo { inner: r, root }, false));
        }
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("main");
        let inner = Repository::init_opts(path, &opts)?;
        let root = path.canonicalize().unwrap_or(path.to_path_buf());
        Ok((Repo { inner, root }, true))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn branch(&self) -> String {
        match self.inner.head() {
            Ok(h) => h.shorthand().unwrap_or("HEAD").to_string(),
            Err(_) => self
                .inner
                .find_reference("HEAD")
                .ok()
                .and_then(|r| r.symbolic_target().ok().flatten().map(|t| t.trim_start_matches("refs/heads/").to_string()))
                .unwrap_or_else(|| "main".into()),
        }
    }

    pub fn status(&self) -> Result<Vec<StatusEntry>> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true).renames_head_to_index(true);
        let statuses = self.inner.statuses(Some(&mut opts))?;
        let mut out = Vec::new();
        for e in statuses.iter() {
            let s = e.status();
            if s.contains(Status::IGNORED) {
                continue;
            }
            let staged = if s.contains(Status::INDEX_NEW) {
                Some('A')
            } else if s.contains(Status::INDEX_MODIFIED) {
                Some('M')
            } else if s.contains(Status::INDEX_DELETED) {
                Some('D')
            } else if s.contains(Status::INDEX_RENAMED) {
                Some('R')
            } else if s.contains(Status::INDEX_TYPECHANGE) {
                Some('T')
            } else {
                None
            };
            let unstaged = if s.contains(Status::CONFLICTED) {
                Some('U')
            } else if s.contains(Status::WT_NEW) {
                Some('?')
            } else if s.contains(Status::WT_MODIFIED) {
                Some('M')
            } else if s.contains(Status::WT_DELETED) {
                Some('D')
            } else if s.contains(Status::WT_TYPECHANGE) {
                Some('T')
            } else {
                None
            };
            if staged.is_none() && unstaged.is_none() {
                continue;
            }
            let path = e
                .head_to_index()
                .and_then(|d| d.new_file().path())
                .or_else(|| e.path().ok().map(Path::new))
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.push(StatusEntry { path, staged, unstaged });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// `git add <paths>` — pathspecs like ".", "src", "*.rs". Also stages deletions.
    pub fn add(&self, specs: &[String]) -> Result<()> {
        let mut index = self.inner.index()?;
        let specs: Vec<String> = if specs.is_empty() { vec![".".into()] } else { specs.to_vec() };
        index.add_all(specs.iter(), IndexAddOption::DEFAULT, None)?;
        index.update_all(specs.iter(), None)?;
        index.write()?;
        Ok(())
    }

    /// `git reset -- <path>` (unstage).
    pub fn unstage(&self, specs: &[String]) -> Result<()> {
        match self.inner.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(c) => self.inner.reset_default(Some(c.as_object()), specs.iter())?,
            None => {
                let mut index = self.inner.index()?;
                for s in specs {
                    let _ = index.remove_path(Path::new(s));
                }
                index.write()?;
            }
        }
        Ok(())
    }

    pub fn has_staged(&self) -> Result<bool> {
        Ok(self.status()?.iter().any(|e| e.staged.is_some()))
    }

    pub fn signature(&self) -> Result<Signature<'static>> {
        match self.inner.signature() {
            Ok(s) => Ok(s.to_owned()),
            Err(_) => bail!(
                "git identity not configured. Run:\n  git config --global user.name \"Your Name\"\n  git config --global user.email you@example.com"
            ),
        }
    }

    /// Commit the index. Returns the new commit id.
    pub fn commit(&self, message: &str) -> Result<Oid> {
        let sig = self.signature()?;
        let mut index = self.inner.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.inner.find_tree(tree_id)?;
        let parent = self.inner.head().ok().and_then(|h| h.peel_to_commit().ok());
        if let Some(p) = &parent {
            if p.tree_id() == tree_id {
                bail!("nothing to commit (stage changes with `gitnext add` or use -a)");
            }
        }
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        Ok(self.inner.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?)
    }

    pub fn log(&self, limit: usize) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        if walk.push_head().is_err() {
            return Ok(vec![]); // no commits yet
        }
        walk.set_sorting(git2::Sort::TIME)?;
        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let c = self.inner.find_commit(oid?)?;
            let id = c.id().to_string();
            out.push(CommitInfo {
                short: id[..7].to_string(),
                id,
                author: c.author().name().unwrap_or("").to_string(),
                time: c.time().seconds(),
                summary: c.summary().ok().flatten().unwrap_or("").to_string(),
                message: c.message().unwrap_or("").to_string(),
            });
        }
        Ok(out)
    }

    /// File contents at HEAD (None if absent / no commits / not UTF-8).
    pub fn read_head(&self, path: &str) -> Option<String> {
        let tree = self.inner.head().ok()?.peel_to_tree().ok()?;
        let entry = tree.get_path(Path::new(path)).ok()?;
        let blob = self.inner.find_blob(entry.id()).ok()?;
        String::from_utf8(blob.content().to_vec()).ok()
    }

    /// File contents in the index (staged version).
    pub fn read_index(&self, path: &str) -> Option<String> {
        let index = self.inner.index().ok()?;
        let entry = index.get_path(Path::new(path), 0)?;
        let blob = self.inner.find_blob(entry.id).ok()?;
        String::from_utf8(blob.content().to_vec()).ok()
    }

    /// File contents at any revision (`HEAD~3`, a branch, a commit id).
    pub fn read_at(&self, rev: &str, path: &str) -> Option<String> {
        let tree = self.inner.revparse_single(rev).ok()?.peel_to_tree().ok()?;
        let entry = tree.get_path(Path::new(path)).ok()?;
        let blob = self.inner.find_blob(entry.id()).ok()?;
        String::from_utf8(blob.content().to_vec()).ok()
    }

    /// Paths that differ between `rev` and the working tree (incl. untracked).
    pub fn changed_since(&self, rev: &str) -> Result<Vec<String>> {
        let tree = self.inner.revparse_single(rev).with_context(|| format!("unknown revision `{rev}`"))?.peel_to_tree()?;
        let mut opts = git2::DiffOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);
        let diff = self.inner.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut opts))?;
        let mut out: Vec<String> = diff
            .deltas()
            .filter_map(|d| d.new_file().path().or_else(|| d.old_file().path()).map(|p| p.to_string_lossy().replace('\\', "/")))
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Paths changed between two revisions (`a..b`).
    pub fn changed_between(&self, a: &str, b: &str) -> Result<Vec<ChangedFile>> {
        let ta = self.inner.revparse_single(a).with_context(|| format!("unknown revision `{a}`"))?.peel_to_tree()?;
        let tb = self.inner.revparse_single(b).with_context(|| format!("unknown revision `{b}`"))?.peel_to_tree()?;
        let diff = self.inner.diff_tree_to_tree(Some(&ta), Some(&tb), None)?;
        let mut out = Vec::new();
        for d in diff.deltas() {
            let path = d.new_file().path().or_else(|| d.old_file().path()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let blob = |id: git2::Oid| -> Option<String> {
                if id.is_zero() {
                    return None;
                }
                self.inner.find_blob(id).ok().and_then(|b| String::from_utf8(b.content().to_vec()).ok())
            };
            out.push((path, blob(d.old_file().id()), blob(d.new_file().id())));
        }
        Ok(out)
    }

    pub fn read_worktree(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(path)).ok()
    }

    /// Stage (or unstage) only the hunks of `path` that touch the given line
    /// ranges — "stage this function". Ranges are 1-based inclusive lines;
    /// staging: old = index version, new = working tree; unstaging: old = HEAD,
    /// new = index (the sides of [`SymChange`]-style before/after ranges).
    /// Returns how many hunks were applied.
    pub fn stage_ranges(&self, path: &str, old_ranges: &[(u32, u32)], new_ranges: &[(u32, u32)], unstage: bool) -> Result<usize> {
        let mut opts = git2::DiffOptions::new();
        opts.pathspec(path).include_untracked(true).recurse_untracked_dirs(true).show_untracked_content(true).context_lines(1).interhunk_lines(0);
        let index = self.inner.index()?;
        let diff = if unstage {
            // index → HEAD: applying it to the index undoes the staged hunks
            let head = self.inner.head().ok().and_then(|h| h.peel_to_tree().ok());
            opts.reverse(true);
            self.inner.diff_tree_to_index(head.as_ref(), Some(&index), Some(&mut opts))?
        } else {
            self.inner.diff_index_to_workdir(Some(&index), Some(&mut opts))?
        };
        let hits = |ranges: &[(u32, u32)], start: u32, lines: u32| {
            if lines == 0 {
                return false; // nothing on this side (pure insertion / deletion)
            }
            let (a, b) = (start, start + lines - 1);
            ranges.iter().any(|&(x, y)| a <= y && x <= b)
        };
        let applied = std::cell::Cell::new(0usize);
        let mut ao = git2::ApplyOptions::new();
        ao.hunk_callback(|h| {
            let Some(h) = h else { return false };
            // the reversed (unstage) diff runs index → HEAD, so its sides are swapped
            let ok = if unstage {
                hits(new_ranges, h.old_start(), h.old_lines()) || hits(old_ranges, h.new_start(), h.new_lines())
            } else {
                hits(old_ranges, h.old_start(), h.old_lines()) || hits(new_ranges, h.new_start(), h.new_lines())
            };
            if ok {
                applied.set(applied.get() + 1);
            }
            ok
        });
        self.inner.apply(&diff, git2::ApplyLocation::Index, Some(&mut ao)).map_err(|e| anyhow::anyhow!("could not stage part of {path}: {}", e.message()))?;
        Ok(applied.get())
    }

    /// Local branches with ahead/behind against their upstream (or the current branch).
    pub fn branches(&self) -> Result<Vec<BranchInfo>> {
        let head = self.inner.head().ok();
        let head_oid = head.as_ref().and_then(|h| h.target());
        let current = head.as_ref().filter(|h| h.is_branch()).and_then(|h| h.shorthand().ok().map(str::to_string));
        let mut out = Vec::new();
        for b in self.inner.branches(Some(git2::BranchType::Local))? {
            let (b, _) = b?;
            let Some(name) = b.name().ok().flatten().map(str::to_string) else { continue };
            let Some(oid) = b.get().target() else { continue };
            let upstream = b.upstream().ok();
            let up_name = upstream.as_ref().and_then(|u| u.name().ok().flatten().map(str::to_string));
            let base = upstream.as_ref().and_then(|u| u.get().target()).or(head_oid);
            let (ahead, behind) = match base {
                Some(base) if base != oid => self.inner.graph_ahead_behind(oid, base).unwrap_or((0, 0)),
                _ => (0, 0),
            };
            let c = self.inner.find_commit(oid)?;
            out.push(BranchInfo {
                current: current.as_deref() == Some(name.as_str()),
                compared_to: up_name.clone().or_else(|| current.clone()).unwrap_or_default(),
                upstream: up_name,
                name,
                ahead,
                behind,
                last_summary: c.summary().ok().flatten().unwrap_or("").to_string(),
                last_author: c.author().name().unwrap_or("").to_string(),
                last_time: c.time().seconds(),
            });
        }
        out.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_time.cmp(&a.last_time)));
        Ok(out)
    }

    /// Undo the last commit but keep its changes staged (`git reset --soft HEAD~1`).
    /// Refuses when the commit is already on the upstream branch.
    pub fn undo_commit(&self) -> Result<CommitInfo> {
        let head = self.inner.head().context("there are no commits yet")?;
        let c = head.peel_to_commit()?;
        let Ok(parent) = c.parent(0) else {
            bail!("this is the first commit of the repository; there is nothing before it to go back to");
        };
        if head.is_branch() {
            if let Some(up) = head.shorthand().ok().and_then(|n| self.inner.find_branch(n, git2::BranchType::Local).ok()).and_then(|b| b.upstream().ok()).and_then(|u| u.get().target()) {
                if up == c.id() || self.inner.graph_descendant_of(up, c.id()).unwrap_or(false) {
                    bail!("the last commit is already pushed; undoing it would rewrite shared history (make a new commit that fixes it instead)");
                }
            }
        }
        let info = self.commit_info(&c);
        self.inner.reset(parent.as_object(), git2::ResetType::Soft, None)?;
        Ok(info)
    }

    /// Recent positions of HEAD (newest first): commits, checkouts, resets, undos.
    pub fn reflog(&self, limit: usize) -> Result<Vec<ReflogEntry>> {
        let log = self.inner.reflog("HEAD")?;
        Ok(log
            .iter()
            .take(limit)
            .map(|e| {
                let id = e.id_new().to_string();
                ReflogEntry { short: id[..7].to_string(), id, message: e.message().ok().flatten().unwrap_or("").to_string(), time: e.committer().when().seconds() }
            })
            .collect())
    }

    /// Move the current branch to `rev`, keeping the working tree and index as they are.
    pub fn reset_soft(&self, rev: &str) -> Result<()> {
        let obj = self.inner.revparse_single(rev).with_context(|| format!("unknown revision `{rev}`"))?;
        self.inner.reset(&obj, git2::ResetType::Soft, None)?;
        Ok(())
    }

    fn commit_info(&self, c: &git2::Commit) -> CommitInfo {
        let id = c.id().to_string();
        CommitInfo {
            short: id[..7].to_string(),
            id,
            author: c.author().name().unwrap_or("").to_string(),
            time: c.time().seconds(),
            summary: c.summary().ok().flatten().unwrap_or("").to_string(),
            message: c.message().unwrap_or("").to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    /// what ahead/behind is measured against (upstream, else the current branch)
    pub compared_to: String,
    pub ahead: usize,
    pub behind: usize,
    pub last_summary: String,
    pub last_author: String,
    pub last_time: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflogEntry {
    pub id: String,
    pub short: String,
    pub message: String,
    pub time: i64,
}
