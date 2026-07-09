use std::path::Path;

use ofm::worktree::*;
use tempfile::TempDir;

async fn init_test_repo(dir: &Path) {
    let output = tokio::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .arg(dir)
        .output()
        .await
        .expect("git init failed");
    assert!(
        output.status.success(),
        "git init failed: {:?}",
        output.stderr
    );

    tokio::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .await
        .expect("git config email failed");

    tokio::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .await
        .expect("git config name failed");

    tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir)
        .output()
        .await
        .expect("git commit failed");
}

fn repo_path(tmp: &TempDir) -> String {
    tmp.path().to_string_lossy().to_string()
}

#[tokio::test]
async fn test_detect_default_branch() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    let branch = detect_default_branch(&repo_path(&tmp))
        .await
        .expect("detect_default_branch failed");
    assert_eq!(branch, "main");
}

#[tokio::test]
async fn test_create_and_remove_worktree() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    let result = create_worktree(&repo_path(&tmp), 1, 42, "test-feature", None)
        .await
        .expect("create_worktree failed");

    assert!(
        result.worktree_path.exists(),
        "worktree directory should exist"
    );
    assert_eq!(result.branch, "task/42-test-feature");

    let branch_list = tokio::process::Command::new("git")
        .args(["branch", "--list", "task/42-test-feature"])
        .current_dir(tmp.path())
        .output()
        .await
        .expect("git branch --list failed");
    let stdout = String::from_utf8_lossy(&branch_list.stdout);
    assert!(
        stdout.contains("task/42-test-feature"),
        "branch should exist, got: {stdout}"
    );

    for dir in &["log", "tmp", "storage"] {
        let path = result.worktree_path.join(dir);
        assert!(path.exists(), "{dir} directory should exist in worktree");
    }

    remove_worktree(&repo_path(&tmp), 1, 42)
        .await
        .expect("remove_worktree failed");

    assert!(
        !result.worktree_path.exists(),
        "worktree directory should be removed"
    );

    let branch_list = tokio::process::Command::new("git")
        .args(["branch", "--list", "task/42-test-feature"])
        .current_dir(tmp.path())
        .output()
        .await
        .expect("git branch --list failed");
    let stdout = String::from_utf8_lossy(&branch_list.stdout);
    assert!(
        !stdout.contains("task/42-test-feature"),
        "branch should be deleted, got: {stdout}"
    );
}

#[tokio::test]
async fn test_create_worktree_with_base_branch() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    tokio::process::Command::new("git")
        .args(["checkout", "-b", "develop"])
        .current_dir(tmp.path())
        .output()
        .await
        .expect("git checkout -b develop failed");

    tokio::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "develop commit"])
        .current_dir(tmp.path())
        .output()
        .await
        .expect("git commit on develop failed");

    let result = create_worktree(&repo_path(&tmp), 1, 99, "from-develop", Some("develop"))
        .await
        .expect("create_worktree with base_branch failed");

    assert_eq!(result.branch, "task/99-from-develop");

    let rev_parse = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&result.worktree_path)
        .output()
        .await
        .expect("git rev-parse failed");
    let stdout = String::from_utf8_lossy(&rev_parse.stdout);
    assert_eq!(stdout.trim(), "task/99-from-develop");

    remove_worktree(&repo_path(&tmp), 1, 99)
        .await
        .expect("remove_worktree failed");
}

#[tokio::test]
async fn test_symlink_env_files() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    let env_path = tmp.path().join(".env");
    tokio::fs::write(&env_path, "DATABASE_URL=test")
        .await
        .expect("write .env failed");

    let result = create_worktree(&repo_path(&tmp), 1, 7, "env-test", None)
        .await
        .expect("create_worktree failed");

    let symlink_path = result.worktree_path.join(".env");
    assert!(
        symlink_path.exists(),
        ".env symlink should exist in worktree"
    );

    #[cfg(unix)]
    {
        let metadata = tokio::fs::symlink_metadata(&symlink_path)
            .await
            .expect("symlink_metadata failed");
        assert!(metadata.is_symlink(), ".env should be a symlink");
    }

    let content = tokio::fs::read_to_string(&symlink_path)
        .await
        .expect("read .env symlink failed");
    assert_eq!(content, "DATABASE_URL=test");

    remove_worktree(&repo_path(&tmp), 1, 7)
        .await
        .expect("remove_worktree failed");
}

#[tokio::test]
async fn test_remove_nonexistent_worktree() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    let result = remove_worktree(&repo_path(&tmp), 999, 999).await;
    assert!(
        result.is_ok(),
        "removing nonexistent worktree should succeed"
    );
}

#[tokio::test]
async fn test_sanitize_title_and_branch_naming() {
    let tmp = TempDir::new().unwrap();
    init_test_repo(tmp.path()).await;

    let result = create_worktree(&repo_path(&tmp), 1, 1, "My Feature Branch!", None)
        .await
        .expect("create_worktree with special title failed");

    assert_eq!(result.branch, "task/1-my-feature-branch");

    let branch_list = tokio::process::Command::new("git")
        .args(["branch", "--list", "task/1-my-feature-branch"])
        .current_dir(tmp.path())
        .output()
        .await
        .expect("git branch --list failed");
    let stdout = String::from_utf8_lossy(&branch_list.stdout);
    assert!(
        stdout.contains("task/1-my-feature-branch"),
        "sanitized branch should exist, got: {stdout}"
    );

    remove_worktree(&repo_path(&tmp), 1, 1)
        .await
        .expect("remove_worktree failed");
}
