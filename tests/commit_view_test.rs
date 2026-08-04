use std::path::Path;

use ofm::services::commits::{self, FileStatus};
use ofm::worktree::create_worktree;
use similar::ChangeTag;
use tempfile::TempDir;

async fn git(dir: &Path, args: &[&str]) {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .expect("git spawn failed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

async fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .expect("git spawn failed");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo_path(tmp: &TempDir) -> String {
    tmp.path().to_string_lossy().to_string()
}

fn footprint_path(tmp: &TempDir) -> String {
    tmp.path()
        .join("ofm-footprint")
        .to_string_lossy()
        .to_string()
}

/// Init a bare repo with a local user configured.
async fn init_repo(dir: &Path) {
    git(dir, &["init", "--initial-branch=main"]).await;
    git(dir, &["config", "user.email", "test@test.com"]).await;
    git(dir, &["config", "user.name", "Test"]).await;
}

/// Initialize a repo that looks like a fresh clone: `main` has a base commit,
/// and `refs/remotes/origin/HEAD` points at the origin-tracking default branch,
/// so `detect_default_branch` / `resolve_base_commit` find "main".
async fn init_origin_like_repo(dir: &Path) {
    init_repo(dir).await;
    std::fs::write(dir.join("file1.txt"), "one\ntwo\n").unwrap();
    git(dir, &["add", "file1.txt"]).await;
    git(dir, &["commit", "-m", "base commit"]).await;

    git(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]).await;
    git(
        dir,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .await;
}

/// Make a worktree for the fixture repo and return its path.
async fn make_worktree(tmp: &TempDir) -> (String, String) {
    let result = create_worktree(
        &repo_path(tmp),
        &footprint_path(tmp),
        1,
        42,
        "feature",
        None,
    )
    .await
    .expect("create_worktree failed");
    (
        result.worktree_path.to_string_lossy().to_string(),
        result.branch,
    )
}

/// Seed a feature branch with three commits:
/// c1 adds file2.txt, c2 modifies file1.txt and deletes file2.txt,
/// c3 modifies file1.txt again.
async fn seed_feature_commits(worktree: &Path) -> Vec<String> {
    let mut shas = Vec::new();

    std::fs::write(worktree.join("file2.txt"), "alpha\n").unwrap();
    git(worktree, &["add", "file2.txt"]).await;
    git(worktree, &["commit", "-m", "add file2"]).await;
    shas.push(git_stdout(worktree, &["rev-parse", "HEAD"]).await);

    std::fs::write(worktree.join("file1.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::remove_file(worktree.join("file2.txt")).unwrap();
    git(worktree, &["add", "-A"]).await;
    git(worktree, &["commit", "-m", "modify file1, delete file2"]).await;
    shas.push(git_stdout(worktree, &["rev-parse", "HEAD"]).await);

    std::fs::write(worktree.join("file1.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    git(worktree, &["add", "-A"]).await;
    git(worktree, &["commit", "-m", "extend file1"]).await;
    shas.push(git_stdout(worktree, &["rev-parse", "HEAD"]).await);

    shas
}

#[tokio::test]
async fn test_list_commits_oldest_first_with_counts() {
    let tmp = TempDir::new().unwrap();
    let repo = repo_path(&tmp);
    init_origin_like_repo(Path::new(&repo)).await;
    let (worktree, _branch) = make_worktree(&tmp).await;
    let shas = seed_feature_commits(Path::new(&worktree)).await;

    let commits = commits::list_commits_for_worktree(Path::new(&worktree))
        .expect("list_commits_for_worktree failed");

    assert_eq!(commits.len(), 3, "only feature commits should be listed");
    assert_eq!(commits[0].summary, "add file2");
    assert_eq!(commits[1].summary, "modify file1, delete file2");
    assert_eq!(commits[2].summary, "extend file1");
    assert_eq!(commits[0].oid.to_string(), shas[0]);
    assert_eq!(commits[2].oid.to_string(), shas[2]);
    assert_eq!(commits[0].author_name, "Test");
    assert_eq!(commits[0].files_changed, 1);
    assert_eq!(commits[1].files_changed, 2);
    assert_eq!(commits[2].files_changed, 1);

    let times: Vec<_> = commits.iter().map(|c| c.authored_time).collect();
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "commits should be ordered oldest to newest"
    );
}

#[tokio::test]
async fn test_commit_diff_two_column_lines() {
    let tmp = TempDir::new().unwrap();
    let repo = repo_path(&tmp);
    init_origin_like_repo(Path::new(&repo)).await;
    let (worktree, _branch) = make_worktree(&tmp).await;
    let shas = seed_feature_commits(Path::new(&worktree)).await;

    let newest = commits::parse_oid(&shas[2]).unwrap();
    let diff = commits::commit_diff(Path::new(&worktree), &newest).expect("commit_diff failed");

    assert_eq!(diff.summary, "extend file1");
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "file1.txt");
    assert_eq!(diff.files[0].status, FileStatus::Modified);
    assert_eq!(diff.files[0].additions, 1);
    assert_eq!(diff.files[0].deletions, 0);

    let insert_lines: Vec<_> = diff.files[0]
        .lines
        .iter()
        .filter(|l| l.line_type == ChangeTag::Insert)
        .collect();
    assert_eq!(insert_lines.len(), 1);
    assert_eq!(insert_lines[0].text, "four\n");
    assert_eq!(insert_lines[0].old_lineno, None);
    assert_eq!(insert_lines[0].new_lineno, Some(4));

    let context_lines: Vec<_> = diff.files[0]
        .lines
        .iter()
        .filter(|l| l.line_type == ChangeTag::Equal)
        .collect();
    assert!(
        context_lines
            .iter()
            .all(|l| l.old_lineno.is_some() && l.new_lineno.is_some()),
        "context lines should carry both line numbers"
    );
    assert!(context_lines.iter().any(|l| l.text == "one\n"));

    // The middle commit deletes file2.txt and modifies file1.txt.
    let middle = commits::parse_oid(&shas[1]).unwrap();
    let middle_diff =
        commits::commit_diff(Path::new(&worktree), &middle).expect("commit_diff failed");
    assert_eq!(middle_diff.files.len(), 2);
    let deleted = middle_diff
        .files
        .iter()
        .find(|f| f.path == "file2.txt")
        .expect("file2.txt should be in the diff");
    assert_eq!(deleted.status, FileStatus::Deleted);
    assert!(deleted
        .lines
        .iter()
        .any(|l| l.line_type == ChangeTag::Delete && l.text == "alpha\n"));
}

#[tokio::test]
async fn test_commit_diff_root_commit_vs_empty_tree() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path()).await;
    std::fs::write(tmp.path().join("root.txt"), "root content\n").unwrap();
    git(tmp.path(), &["add", "root.txt"]).await;
    git(tmp.path(), &["commit", "-m", "root commit"]).await;
    let root_sha = git_stdout(tmp.path(), &["rev-parse", "HEAD"]).await;

    let oid = commits::parse_oid(&root_sha).unwrap();
    let diff = commits::commit_diff(tmp.path(), &oid).expect("root commit diff failed");

    assert_eq!(diff.summary, "root commit");
    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, "root.txt");
    assert_eq!(diff.files[0].status, FileStatus::Added);
    assert_eq!(diff.files[0].additions, 1);
    assert!(diff.files[0]
        .lines
        .iter()
        .any(|l| l.line_type == ChangeTag::Insert && l.text == "root content\n"));
}

#[tokio::test]
async fn test_commit_diff_skips_oversized_files() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path()).await;
    std::fs::write(tmp.path().join("small.txt"), "small\n").unwrap();
    let big = "x".repeat(1024 * 1024 + 1);
    std::fs::write(tmp.path().join("big.txt"), &big).unwrap();
    git(tmp.path(), &["add", "-A"]).await;
    git(tmp.path(), &["commit", "-m", "big file"]).await;
    let sha = git_stdout(tmp.path(), &["rev-parse", "HEAD"]).await;

    let oid = commits::parse_oid(&sha).unwrap();
    let diff = commits::commit_diff(tmp.path(), &oid).expect("commit_diff failed");

    assert_eq!(diff.files.len(), 1, "oversized blob should be skipped");
    assert_eq!(diff.files[0].path, "small.txt");
}

#[tokio::test]
async fn test_fully_merged_branch_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let repo = repo_path(&tmp);
    init_origin_like_repo(Path::new(&repo)).await;
    let (worktree, branch) = make_worktree(&tmp).await;
    seed_feature_commits(Path::new(&worktree)).await;

    let before =
        commits::list_commits_for_worktree(Path::new(&worktree)).expect("list before merge failed");
    assert_eq!(before.len(), 3);

    // Merge the feature branch into main (fast-forward): the worktree branch is
    // now fully merged, so the commit list must be empty.
    git(Path::new(&repo), &["checkout", "main"]).await;
    git(Path::new(&repo), &["merge", "--ff-only", &branch]).await;

    let after =
        commits::list_commits_for_worktree(Path::new(&worktree)).expect("list after merge failed");
    assert!(
        after.is_empty(),
        "fully-merged branch should list no commits"
    );
}

#[tokio::test]
async fn test_local_repo_without_remote_degrades_gracefully() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path()).await;
    std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
    git(tmp.path(), &["add", "a.txt"]).await;
    git(tmp.path(), &["commit", "-m", "initial"]).await;

    // No remote and HEAD is on the only branch: base resolution falls back to
    // the current branch, so the list is empty rather than erroring.
    let commits = commits::list_commits_for_worktree(tmp.path()).expect("should not error");
    assert!(commits.is_empty());
}

#[tokio::test]
async fn test_resolve_oid_supports_short_and_full() {
    let tmp = TempDir::new().unwrap();
    let repo = repo_path(&tmp);
    init_origin_like_repo(Path::new(&repo)).await;
    let (worktree, _branch) = make_worktree(&tmp).await;
    let shas = seed_feature_commits(Path::new(&worktree)).await;

    let short = &shas[0][..8];
    let resolved = commits::resolve_oid(Path::new(&worktree), short).expect("short oid resolves");
    assert_eq!(resolved.to_string(), shas[0]);

    let resolved_full =
        commits::resolve_oid(Path::new(&worktree), &shas[0]).expect("full oid resolves");
    assert_eq!(resolved_full.to_string(), shas[0]);

    assert!(commits::resolve_oid(Path::new(&worktree), "deadbeef").is_err());
}
