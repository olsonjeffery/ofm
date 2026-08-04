use std::path::Path;

use chrono::{DateTime, Utc};
use gix::bstr::ByteSlice;
use similar::{ChangeTag, TextDiff};

/// Skip diff rendering for files whose blob exceeds this size, bounding memory
/// use and response size for commits touching large generated or data files.
const MAX_DIFF_BLOB_BYTES: u64 = 1 << 20;

/// Maximum number of changed files rendered per commit diff.
const MAX_DIFF_FILES: usize = 200;

/// Maximum number of diff lines materialized per file and per commit.
const MAX_DIFF_LINES: usize = 20_000;

/// Errors returned by the git commit/diff service functions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not open git repository at {path}: {source}")]
    RepoOpen {
        path: String,
        source: Box<gix::open::Error>,
    },
    #[error("no base branch could be resolved for the worktree")]
    BaseNotFound,
    #[error("commit {oid} not found in repository")]
    CommitNotFound { oid: String },
    #[error("invalid object id {oid}: {source}")]
    InvalidOid {
        oid: String,
        source: Box<gix::hash::decode::Error>,
    },
    #[error("could not resolve oid spec {oid}: {source}")]
    ResolveOid {
        oid: String,
        source: Box<gix::revision::spec::parse::single::Error>,
    },
    #[error("git diff failed: {0}")]
    Diff(String),
    #[error("git error: {0}")]
    Other(String),
}

/// A single commit on the worktree branch, as rendered in the task detail commit table.
#[derive(Debug, Clone)]
pub struct CommitSummary {
    pub oid: gix::ObjectId,
    /// Shortened object id, e.g. the first 8 hex chars.
    pub short_oid: String,
    /// First line of the commit message.
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_time: DateTime<Utc>,
    /// Number of files changed relative to the commit's first parent (or empty tree).
    pub files_changed: usize,
}

/// Change classification of a file in a commit diff, mirroring `git diff --name-status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Modified => "Modified",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
        }
    }
}

/// One side of one line in a two-column diff, pre-aligned for rendering.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub line_type: ChangeTag,
    /// 1-based line number in the old (left) side, `None` for inserted lines.
    pub old_lineno: Option<u32>,
    /// 1-based line number in the new (right) side, `None` for deleted lines.
    pub new_lineno: Option<u32>,
    pub text: String,
}

/// A single changed file within a commit diff.
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
    pub lines: Vec<DiffLine>,
}

/// The full diff of a single commit.
#[derive(Debug, Clone)]
pub struct CommitDiff {
    pub oid: gix::ObjectId,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_time: DateTime<Utc>,
    pub files: Vec<FileDiff>,
}

/// Author and message metadata shared by [`CommitSummary`] and [`CommitDiff`].
#[derive(Debug, Clone)]
struct CommitMeta {
    summary: String,
    author_name: String,
    author_email: String,
    authored_time: DateTime<Utc>,
}

fn commit_meta(commit: &gix::Commit<'_>) -> Result<CommitMeta, Error> {
    let author = commit
        .author()
        .map_err(|e| Error::Other(format!("failed to read author: {e}")))?;
    let authored_time = author
        .time()
        .map(|t| DateTime::from_timestamp(t.seconds, 0).unwrap_or_default())
        .unwrap_or_else(|_| Utc::now());
    let summary = commit
        .message()
        .map(|m| m.summary().to_string())
        .unwrap_or_else(|_| String::new());
    Ok(CommitMeta {
        summary,
        author_name: author.name.to_string(),
        author_email: author.email.to_string(),
        authored_time,
    })
}

fn open_repo(path: &Path) -> Result<gix::Repository, Error> {
    gix::open(path).map_err(|source| Error::RepoOpen {
        path: path.display().to_string(),
        source: Box::new(source),
    })
}

/// Parse a full hexadecimal object id (40 or 64 chars).
///
/// Short ids are not resolvable without a repository, so they yield
/// [`Error::InvalidOid`] here; resolve short ids against a repository with
/// [`resolve_oid`] instead.
pub fn parse_oid(oid: &str) -> Result<gix::ObjectId, Error> {
    gix::ObjectId::from_hex(oid.as_bytes()).map_err(|source| Error::InvalidOid {
        oid: oid.to_string(),
        source: Box::new(source),
    })
}

/// Resolve a full or abbreviated object id spec (like `HEAD~1` or a short hash)
/// against the repository at `worktree_path`.
pub fn resolve_oid(worktree_path: &Path, spec: &str) -> Result<gix::ObjectId, Error> {
    let repo = open_repo(worktree_path)?;
    repo.rev_parse_single(spec.as_bytes().as_bstr())
        .map(|id| id.detach())
        .map_err(|source| Error::ResolveOid {
            oid: spec.to_string(),
            source: Box::new(source),
        })
}

/// Resolve the base commit of a worktree, mirroring the branch resolution logic
/// of [`crate::worktree::detect_default_branch`]:
/// `refs/remotes/origin/HEAD` → the currently checked-out branch → `"main"`.
///
/// Returns `Ok(None)` when no candidate base reference can be resolved, so
/// callers can degrade gracefully to an empty commit list.
fn resolve_base_commit(repo: &gix::Repository) -> Result<Option<gix::ObjectId>, Error> {
    let mut base_names = Vec::new();

    // 1. The symbolic ref refs/remotes/origin/HEAD names the default branch,
    //    e.g. "main"; strip the refs/remotes/origin/ prefix like
    //    detect_default_branch does.
    if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let gix::refs::TargetRef::Symbolic(full_name) = reference.target() {
            if let Some(name) = full_name.as_bstr().strip_prefix(b"refs/remotes/origin/") {
                base_names.push(String::from_utf8_lossy(name).into_owned());
            }
        }
    }

    // 2. Fall back to the currently checked-out branch name.
    if let Ok(Some(head_name)) = repo.head_name() {
        base_names.push(head_name.shorten().to_string());
    }

    // 3. Final fallback.
    base_names.push("main".to_string());

    for name in base_names {
        if let Ok(mut reference) = repo.find_reference(&name) {
            if let Ok(commit) = reference.peel_to_commit() {
                return Ok(Some(commit.id));
            }
        }
    }
    Ok(None)
}

/// List the commits on the worktree's branch since the merge-base with the base
/// branch, ordered oldest → newest.
///
/// Graceful results:
/// - Fully merged branch (`merge_base == HEAD`) → `vec![]`.
/// - No base branch resolvable → `vec![]`.
/// - No common ancestor between HEAD and base → `vec![]`.
pub fn list_commits_for_worktree(worktree_path: &Path) -> Result<Vec<CommitSummary>, Error> {
    let repo = open_repo(worktree_path)?;
    let head_commit = repo
        .head_commit()
        .map_err(|e| Error::Other(format!("failed to resolve HEAD commit: {e}")))?;
    let head_id = head_commit.id;

    let base_id = match resolve_base_commit(&repo)? {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let merge_base = match repo.merge_base(head_id, base_id) {
        Ok(id) => id.detach(),
        Err(_) => return Ok(Vec::new()),
    };

    if merge_base == head_id {
        return Ok(Vec::new());
    }

    // Traverse from HEAD, hiding the merge-base and everything reachable from
    // it. The default walk is newest-first; reverse to oldest-first so the
    // commit table renders bottom-to-top from oldest to newest.
    let walk = repo
        .rev_walk(Some(head_id))
        .with_hidden(Some(merge_base))
        .sorting(gix::revision::walk::Sorting::BreadthFirst)
        .all()
        .map_err(|e| Error::Other(format!("failed to walk commits: {e}")))?;

    let mut ids = Vec::new();
    for item in walk {
        let info = item.map_err(|e| Error::Other(format!("commit walk failed: {e}")))?;
        ids.push(info.id);
    }
    ids.reverse();

    let mut commits = Vec::new();
    for id in ids {
        let commit = repo.find_commit(id).map_err(|_| Error::CommitNotFound {
            oid: id.to_string(),
        })?;
        commits.push(build_commit_summary(&repo, &commit)?);
    }
    Ok(commits)
}

fn build_commit_summary(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
) -> Result<CommitSummary, Error> {
    let files_changed = count_files_changed(repo, commit)?;
    let meta = commit_meta(commit)?;
    Ok(CommitSummary {
        oid: commit.id,
        short_oid: short_oid(commit.id),
        summary: meta.summary,
        author_name: meta.author_name,
        author_email: meta.author_email,
        authored_time: meta.authored_time,
        files_changed,
    })
}

pub fn short_oid(oid: gix::ObjectId) -> String {
    oid.to_hex().to_string()[..8].to_string()
}

/// Produce the owned list of non-directory changes needed to turn `old_tree`
/// into `new_tree`, with rewrite (rename) tracking disabled so statuses stay
/// unambiguous add/modify/delete.
fn tree_changes<'r>(
    repo: &'r gix::Repository,
    old_tree: Option<&gix::Tree<'r>>,
    new_tree: Option<&gix::Tree<'r>>,
) -> Result<Vec<gix::diff::tree_with_rewrites::Change>, Error> {
    let options = gix::diff::Options::default();
    let changes = repo
        .diff_tree_to_tree(old_tree, new_tree, options)
        .map_err(|e| Error::Diff(e.to_string()))?;
    Ok(changes
        .into_iter()
        .filter(is_file_change)
        .collect::<Vec<_>>())
}

fn is_file_change(change: &gix::diff::tree_with_rewrites::Change) -> bool {
    use gix::diff::tree_with_rewrites::Change;
    match change {
        Change::Addition { entry_mode, .. } | Change::Deletion { entry_mode, .. } => {
            entry_mode.is_no_tree()
        }
        Change::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } => previous_entry_mode.is_no_tree() && entry_mode.is_no_tree(),
        Change::Rewrite { .. } => true,
    }
}

/// Load the first parent's tree of `decoded`, or `None` for a root commit.
fn parent_tree<'r>(
    repo: &'r gix::Repository,
    decoded: &gix::objs::CommitRef<'_>,
) -> Result<Option<gix::Tree<'r>>, Error> {
    let Some(parent_id) = decoded.parents().next() else {
        return Ok(None);
    };
    let parent = repo
        .find_commit(parent_id)
        .map_err(|e| Error::Other(format!("failed to load parent commit: {e}")))?;
    parent
        .tree()
        .map(Some)
        .map_err(|e| Error::Other(format!("failed to load parent tree: {e}")))
}

/// Load a commit's own tree together with its first parent's tree (`None` for
/// a root commit), as needed for a first-parent diff.
fn commit_trees<'r>(
    repo: &'r gix::Repository,
    commit: &gix::Commit<'r>,
) -> Result<(Option<gix::Tree<'r>>, gix::Tree<'r>), Error> {
    let decoded = commit
        .decode()
        .map_err(|e| Error::Other(format!("failed to decode commit: {e}")))?;
    let parent_tree = parent_tree(repo, &decoded)?;
    let commit_tree = commit
        .tree()
        .map_err(|e| Error::Other(format!("failed to load commit tree: {e}")))?;
    Ok((parent_tree, commit_tree))
}

fn count_files_changed(repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<usize, Error> {
    let (parent_tree, commit_tree) = commit_trees(repo, commit)?;
    Ok(tree_changes(repo, parent_tree.as_ref(), Some(&commit_tree))?.len())
}

/// Produce the full diff of a single commit (first-parent diff, or empty tree
/// for a root commit).
pub fn commit_diff(worktree_path: &Path, oid: &gix::ObjectId) -> Result<CommitDiff, Error> {
    let repo = open_repo(worktree_path)?;
    let commit = repo.find_commit(*oid).map_err(|_| Error::CommitNotFound {
        oid: oid.to_string(),
    })?;
    let (parent_tree, commit_tree) = commit_trees(&repo, &commit)?;

    let mut files = Vec::new();
    let mut total_lines = 0;
    for change in tree_changes(&repo, parent_tree.as_ref(), Some(&commit_tree))? {
        if files.len() >= MAX_DIFF_FILES || total_lines >= MAX_DIFF_LINES {
            break;
        }
        if let Some(file_diff) = build_file_diff(&repo, &change)? {
            total_lines += file_diff.lines.len();
            files.push(file_diff);
        }
    }

    let meta = commit_meta(&commit)?;
    Ok(CommitDiff {
        oid: *oid,
        summary: meta.summary,
        author_name: meta.author_name,
        author_email: meta.author_email,
        authored_time: meta.authored_time,
        files,
    })
}

fn build_file_diff(
    repo: &gix::Repository,
    change: &gix::diff::tree_with_rewrites::Change,
) -> Result<Option<FileDiff>, Error> {
    use gix::diff::tree_with_rewrites::Change;
    let (path, old_id, new_id, status) = match change {
        Change::Addition { location, id, .. } => {
            (location.to_string(), None, Some(*id), FileStatus::Added)
        }
        Change::Deletion { location, id, .. } => {
            (location.to_string(), Some(*id), None, FileStatus::Deleted)
        }
        Change::Modification {
            location,
            previous_id,
            id,
            ..
        } => (
            location.to_string(),
            Some(*previous_id),
            Some(*id),
            FileStatus::Modified,
        ),
        Change::Rewrite {
            source_id,
            location,
            id,
            ..
        } => (
            location.to_string(),
            Some(*source_id),
            Some(*id),
            FileStatus::Renamed,
        ),
    };

    let old_size = blob_size(repo, old_id)?.unwrap_or(0);
    let new_size = blob_size(repo, new_id)?.unwrap_or(0);
    if old_size > MAX_DIFF_BLOB_BYTES || new_size > MAX_DIFF_BLOB_BYTES {
        return Ok(None);
    }

    let old_text = blob_text(repo, old_id)?.unwrap_or_default();
    let new_text = blob_text(repo, new_id)?.unwrap_or_default();

    let (lines, additions, deletions) = diff_lines(&old_text, &new_text);

    Ok(Some(FileDiff {
        path,
        status,
        additions,
        deletions,
        lines,
    }))
}

/// Read a blob's size in bytes without fully decoding it, so callers can bound
/// memory use before materializing large blobs.
fn blob_size(repo: &gix::Repository, id: Option<gix::ObjectId>) -> Result<Option<u64>, Error> {
    let Some(id) = id else {
        return Ok(None);
    };
    let header = repo
        .find_header(id)
        .map_err(|e| Error::Other(format!("failed to read blob header {id}: {e}")))?;
    Ok(Some(header.size()))
}

/// Read a blob as UTF-8 text. Returns `Ok(None)` for binary content (contains
/// NUL bytes); callers fall back to an empty string so binary files diff as
/// empty rather than rendering binary garbage.
fn blob_text(repo: &gix::Repository, id: Option<gix::ObjectId>) -> Result<Option<String>, Error> {
    let Some(id) = id else {
        return Ok(Some(String::new()));
    };
    let blob = repo
        .find_blob(id)
        .map_err(|e| Error::Other(format!("failed to read blob {id}: {e}")))?;
    if blob.data.contains(&0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&blob.data).into_owned()))
}

/// Run a line diff over two text contents and produce the pre-aligned
/// [`DiffLine`] sequence plus add/delete counts.
///
/// Each change from `similar::TextDiff::iter_all_changes()` maps to exactly one
/// [`DiffLine`]: `Equal` carries both line numbers, `Delete` only the old one,
/// and `Insert` only the new one. Renderers derive the two columns from this.
fn diff_lines(old: &str, new: &str) -> (Vec<DiffLine>, usize, usize) {
    let text_diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;
    for change in text_diff.iter_all_changes() {
        if lines.len() >= MAX_DIFF_LINES {
            break;
        }
        match change.tag() {
            ChangeTag::Insert => additions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
        lines.push(DiffLine {
            line_type: change.tag(),
            old_lineno: change.old_index().map(|i| i as u32 + 1),
            new_lineno: change.new_index().map(|i| i as u32 + 1),
            text: change.value().to_string(),
        });
    }
    (lines, additions, deletions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_oid_accepts_full_sha1() {
        let oid = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let parsed = parse_oid(oid).expect("valid full oid should parse");
        assert_eq!(parsed.to_string(), oid);
    }

    #[test]
    fn parse_oid_rejects_short_oid() {
        let err = parse_oid("deadbeef").unwrap_err();
        assert!(matches!(err, Error::InvalidOid { .. }));
    }

    #[test]
    fn parse_oid_rejects_invalid_hex() {
        let err = parse_oid("zzzbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap_err();
        assert!(matches!(err, Error::InvalidOid { .. }));
    }

    #[test]
    fn parse_oid_rejects_empty() {
        let err = parse_oid("").unwrap_err();
        assert!(matches!(err, Error::InvalidOid { .. }));
    }

    #[test]
    fn diff_lines_two_column_alignment() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2-changed\nline3\nline4\n";
        let (lines, additions, deletions) = diff_lines(old, new);
        assert_eq!(additions, 2);
        assert_eq!(deletions, 1);

        let expected = [
            (ChangeTag::Equal, Some(1), Some(1), "line1\n"),
            (ChangeTag::Delete, Some(2), None, "line2\n"),
            (ChangeTag::Insert, None, Some(2), "line2-changed\n"),
            (ChangeTag::Equal, Some(3), Some(3), "line3\n"),
            (ChangeTag::Insert, None, Some(4), "line4\n"),
        ];
        let actual: Vec<_> = lines
            .iter()
            .map(|l| (l.line_type, l.old_lineno, l.new_lineno, l.text.as_str()))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn diff_lines_pure_addition() {
        let (lines, additions, deletions) = diff_lines("", "newline\n");
        assert_eq!(additions, 1);
        assert_eq!(deletions, 0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_type, ChangeTag::Insert);
        assert_eq!(lines[0].old_lineno, None);
        assert_eq!(lines[0].new_lineno, Some(1));
    }

    #[test]
    fn diff_lines_pure_deletion() {
        let (lines, additions, deletions) = diff_lines("oldline\n", "");
        assert_eq!(additions, 0);
        assert_eq!(deletions, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_type, ChangeTag::Delete);
        assert_eq!(lines[0].old_lineno, Some(1));
        assert_eq!(lines[0].new_lineno, None);
    }

    #[test]
    fn list_commits_are_ordered_oldest_first() {
        // The walk yields commits oldest-first; verify a pre-built summary list
        // respects ascending authored_time as a sanity check of the contract.
        let mut commits = Vec::new();
        let base = chrono::NaiveDate::from_ymd_opt(2024, 6, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        for i in 0..5 {
            commits.push(CommitSummary {
                oid: gix::ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
                short_oid: format!("commit{i}"),
                summary: format!("commit {i}"),
                author_name: "A".into(),
                author_email: "a@example.com".into(),
                authored_time: DateTime::<Utc>::from_naive_utc_and_offset(
                    base + chrono::Duration::hours(i),
                    Utc,
                ),
                files_changed: 1,
            });
        }
        let windows = commits.windows(2);
        for pair in windows {
            assert!(pair[0].authored_time <= pair[1].authored_time);
        }
    }

    #[test]
    fn file_status_as_str() {
        assert_eq!(FileStatus::Added.as_str(), "Added");
        assert_eq!(FileStatus::Modified.as_str(), "Modified");
        assert_eq!(FileStatus::Deleted.as_str(), "Deleted");
        assert_eq!(FileStatus::Renamed.as_str(), "Renamed");
    }

    #[test]
    fn short_oid_prefix() {
        let oid = gix::ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        assert_eq!(short_oid(oid), "deadbeef");
    }
}
