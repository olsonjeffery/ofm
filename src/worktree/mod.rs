use std::path::{Path, PathBuf};

use tokio::process::Command;

#[derive(serde::Serialize)]
pub struct CreateWorktreeResult {
    pub worktree_path: PathBuf,
    pub branch: String,
}

pub fn sanitize_title(title: &str) -> String {
    let sanitized = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let mut s = sanitized.trim_matches('-').to_string();
    if s.is_empty() {
        return "task".into();
    }
    s.truncate(30);
    s
}

pub fn valid_branch_name(name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if name.is_empty() {
        return Err(format!("invalid branch name: {name}").into());
    }
    if name.contains("..") {
        return Err(format!("invalid branch name: {name}").into());
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(format!("invalid branch name: {name}").into());
    }
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '/' && c != '-' {
            return Err(format!("invalid branch name: {name}").into());
        }
    }
    Ok(())
}

pub fn get_worktree_path(footprint: &str, project_id: i64, task_id: i64) -> PathBuf {
    PathBuf::from(footprint).join(format!("worktrees/project-{project_id}/task-{task_id}/"))
}

pub async fn detect_default_branch(
    repo_path: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let branch = stdout.trim().strip_prefix("refs/remotes/origin/");
        if let Some(b) = branch {
            return Ok(b.to_string());
        }
    }

    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let branch = stdout.trim();
        if !branch.is_empty() && branch != "HEAD" {
            return Ok(branch.to_string());
        }
    }

    Ok("main".into())
}

pub async fn create_worktree(
    repo_path: &str,
    footprint: &str,
    project_id: i64,
    task_id: i64,
    title: &str,
    base_branch: Option<&str>,
) -> Result<CreateWorktreeResult, Box<dyn std::error::Error + Send + Sync>> {
    let worktree_path = get_worktree_path(footprint, project_id, task_id);
    if let Some(parent) = worktree_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            format!(
                "failed to create worktree parent dir {}: {e}",
                parent.display()
            )
        })?;
    }
    let sanitized = sanitize_title(title);
    let branch = format!("task/{task_id}-{sanitized}");
    valid_branch_name(&branch)?;

    let base = match base_branch {
        Some(b) => b.to_string(),
        None => detect_default_branch(repo_path)
            .await
            .map_err(|e| format!("failed to detect default branch in repo {repo_path}: {e}"))?,
    };
    valid_branch_name(&base)?;

    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch,
            &worktree_path.to_string_lossy(),
            &base,
        ])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await
        .map_err(|e| format!("failed to spawn git worktree add in repo {repo_path}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {stderr}").into());
    }

    setup_worktree_environment(repo_path, &worktree_path).await;

    Ok(CreateWorktreeResult {
        worktree_path,
        branch,
    })
}

/// Full create-time environment setup, shared by `create_worktree` and
/// `recreate_worktree` so recreated worktrees get parity with fresh ones:
/// env-file symlinks, gitignored dirs, and a background dependency copy.
async fn setup_worktree_environment(repo_path: &str, worktree_path: &Path) {
    symlink_env_files(repo_path, worktree_path).await;
    create_gitignored_dirs(worktree_path).await;
    copy_dependencies_background(repo_path, worktree_path).await;
}

/// Re-creates a task worktree whose directory was deleted from the
/// filesystem but whose `worktrees` DB row (branch + repo) still exists.
///
/// Idempotent: returns success without touching git if the directory is
/// already present. Otherwise it prunes stale git worktree registrations
/// (the deleted directory leaves one behind), re-attaches the stored branch
/// if it still exists (preserving its HEAD), and only if the branch was
/// deleted too, recreates it from `detect_default_branch` — the "default
/// clone checkout commit" — via `git worktree add -b`. Setup runs last so a
/// recreated worktree has full parity with a freshly created one.
pub async fn recreate_worktree(
    repo_path: &str,
    footprint: &str,
    project_id: i64,
    task_id: i64,
    branch: &str,
    title: &str,
) -> Result<CreateWorktreeResult, Box<dyn std::error::Error + Send + Sync>> {
    let worktree_path = get_worktree_path(footprint, project_id, task_id);
    if worktree_path.exists() {
        return Ok(CreateWorktreeResult {
            worktree_path,
            branch: branch.to_string(),
        });
    }
    if let Some(parent) = worktree_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            format!(
                "failed to create worktree parent dir {}: {e}",
                parent.display()
            )
        })?;
    }
    let branch = if branch.trim().is_empty() {
        format!("task/{task_id}-{}", sanitize_title(title))
    } else {
        branch.to_string()
    };
    valid_branch_name(&branch)?;

    // The directory was deleted out from under git, so a stale registration
    // remains; prune it so `git worktree add` below does not refuse with
    // "already registered worktree".
    let prune_output = Command::new("git")
        .args(["worktree", "prune"])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await
        .map_err(|e| format!("failed to spawn git worktree prune in repo {repo_path}: {e}"))?;
    if !prune_output.status.success() {
        let stderr = String::from_utf8_lossy(&prune_output.stderr);
        return Err(format!("git worktree prune failed: {stderr}").into());
    }

    // 1) Branch still exists → re-attach it, preserving its HEAD.
    let output = Command::new("git")
        .args(["worktree", "add", &worktree_path.to_string_lossy(), &branch])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await
        .map_err(|e| format!("failed to spawn git worktree add in repo {repo_path}: {e}"))?;

    if !output.status.success() {
        // 2) Branch deleted too → recreate it from the default branch tip.
        let base = detect_default_branch(repo_path)
            .await
            .map_err(|e| format!("failed to detect default branch in repo {repo_path}: {e}"))?;
        valid_branch_name(&base)?;
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_path.to_string_lossy(),
                &base,
            ])
            .env("GIT_DISABLE_HOOKS", "1")
            .current_dir(repo_path)
            .output()
            .await
            .map_err(|e| format!("failed to spawn git worktree add in repo {repo_path}: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git worktree add failed: {stderr}").into());
        }
    }

    setup_worktree_environment(repo_path, &worktree_path).await;

    Ok(CreateWorktreeResult {
        worktree_path,
        branch,
    })
}

async fn symlink_env_files(repo_path: &str, worktree_path: &Path) {
    let env_files = [
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ];

    for filename in &env_files {
        let src = Path::new(repo_path).join(filename);
        let dst = worktree_path.join(filename);

        if !src.exists() || dst.exists() {
            continue;
        }

        #[cfg(unix)]
        {
            if let Err(e) = tokio::fs::symlink(&src, &dst).await {
                tracing::warn!("failed to symlink {}: {e}", filename);
            }
        }

        #[cfg(not(unix))]
        {
            tracing::warn!(
                "symlink not supported on this platform, skipping {}",
                filename
            );
        }
    }
}

async fn create_gitignored_dirs(project_path: &Path) {
    let dirs = ["log", "tmp", "storage"];

    for dir in &dirs {
        let path = project_path.join(dir);
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            tracing::warn!("failed to create dir {}: {e}", dir);
        }
    }
}

async fn copy_dependencies_background(repo_path: &str, project_path: &Path) {
    let dirs = ["node_modules", ".venv"];

    for &dir in &dirs {
        let src = Path::new(repo_path).join(dir);
        let dst = project_path.join(dir);

        let src_s = src.to_string_lossy().to_string();
        let dst_s = dst.to_string_lossy().to_string();
        let dir_s = dir.to_string();
        tokio::spawn(async move {
            if !Path::new(&src_s).exists() {
                return;
            }
            let result = Command::new("cp")
                .args(["-a", &src_s, &dst_s])
                .output()
                .await;
            if let Err(e) = result {
                tracing::warn!("failed to copy {dir_s} to worktree: {e}");
            }
        });
    }
}

pub async fn remove_worktree(
    repo_path: &str,
    worktree_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !worktree_path.exists() {
        return Ok(());
    }

    let branch_output = Command::new("git")
        .args(["branch", "--show-current"])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(worktree_path)
        .output()
        .await?;

    let branch = if branch_output.status.success() {
        let stdout = String::from_utf8_lossy(&branch_output.stdout);
        let b = stdout.trim().to_string();
        if b.is_empty() {
            None
        } else {
            Some(b)
        }
    } else {
        None
    };

    let remove_output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(repo_path)
        .output()
        .await?;

    if !remove_output.status.success() {
        let stderr = String::from_utf8_lossy(&remove_output.stderr);
        return Err(format!("git worktree remove failed: {stderr}").into());
    }

    if let Some(b) = branch {
        let _ = Command::new("git")
            .args(["branch", "-D", &b])
            .env("GIT_DISABLE_HOOKS", "1")
            .current_dir(repo_path)
            .output()
            .await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title_empty() {
        assert_eq!(sanitize_title(""), "task");
    }

    #[test]
    fn test_sanitize_title_all_special() {
        assert_eq!(sanitize_title("!!! @@@ ###"), "task");
    }

    #[test]
    fn test_sanitize_title_leading_trailing_dashes() {
        assert_eq!(sanitize_title("--hello-world--"), "hello-world");
    }

    #[test]
    fn test_sanitize_title_special_chars() {
        assert_eq!(sanitize_title("Hello World! @Test"), "hello-world---test");
    }

    #[test]
    fn test_sanitize_title_truncation() {
        let long = "a".repeat(50);
        let result = sanitize_title(&long);
        assert_eq!(result.len(), 30);
        assert_eq!(result, "a".repeat(30));
    }

    #[test]
    fn test_sanitize_title_all_numeric() {
        assert_eq!(sanitize_title("12345"), "12345");
    }

    #[test]
    fn test_valid_branch_name_valid() {
        assert!(valid_branch_name("task/42-foo-bar").is_ok());
        assert!(valid_branch_name("main").is_ok());
        assert!(valid_branch_name("feature/my-feature_v2").is_ok());
    }

    #[test]
    fn test_valid_branch_name_leading_dash() {
        assert!(valid_branch_name("-branch").is_err());
    }

    #[test]
    fn test_valid_branch_name_double_dot() {
        assert!(valid_branch_name("foo..bar").is_err());
    }

    #[test]
    fn test_valid_branch_name_special_chars() {
        assert!(valid_branch_name("foo bar").is_err());
        assert!(valid_branch_name("foo:bar").is_err());
    }

    #[test]
    fn test_valid_branch_name_underscore_dot_slash() {
        assert!(valid_branch_name("feature/my_feature.v2").is_ok());
    }

    #[test]
    fn test_valid_branch_name_empty() {
        assert!(valid_branch_name("").is_err());
    }

    #[test]
    fn test_get_worktree_path() {
        let path = get_worktree_path("/footprint", 1, 42);
        assert_eq!(
            path,
            PathBuf::from("/footprint/worktrees/project-1/task-42/")
        );
    }

    #[test]
    fn test_get_worktree_path_trailing_slash() {
        let path = get_worktree_path("/footprint/", 2, 99);
        assert_eq!(
            path,
            PathBuf::from("/footprint//worktrees/project-2/task-99/")
        );
    }

    #[test]
    fn test_sanitize_title_mixed_case() {
        assert_eq!(sanitize_title("ABC def GHI"), "abc-def-ghi");
    }
}

#[cfg(test)]
mod git_tests {
    use super::*;
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

    fn footprint_path(tmp: &TempDir) -> String {
        tmp.path()
            .join("ofm-footprint")
            .to_string_lossy()
            .to_string()
    }

    async fn git_branch_in_worktree(worktree_path: &Path) -> String {
        let output = tokio::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(worktree_path)
            .output()
            .await
            .expect("git branch --show-current failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[tokio::test]
    async fn test_recreate_worktree_reattaches_existing_branch() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(tmp.path()).await;

        let created = create_worktree(
            &repo_path(&tmp),
            &footprint_path(&tmp),
            1,
            42,
            "feature",
            None,
        )
        .await
        .expect("create_worktree failed");

        std::fs::remove_dir_all(&created.worktree_path).expect("remove worktree dir failed");

        let recreated = recreate_worktree(
            &repo_path(&tmp),
            &footprint_path(&tmp),
            1,
            42,
            &created.branch,
            "feature",
        )
        .await
        .expect("recreate_worktree failed");

        assert!(
            recreated.worktree_path.exists(),
            "worktree directory should exist again"
        );
        assert_eq!(
            recreated.branch, created.branch,
            "branch name should be preserved"
        );

        let worktree_list = tokio::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(tmp.path())
            .output()
            .await
            .expect("git worktree list failed");
        let stdout = String::from_utf8_lossy(&worktree_list.stdout);
        assert!(
            stdout.contains("task-42"),
            "git worktree list should contain the recreated path: {stdout}"
        );

        assert_eq!(
            git_branch_in_worktree(&recreated.worktree_path).await,
            created.branch,
            "worktree should be on the recreated branch"
        );
    }

    #[tokio::test]
    async fn test_recreate_worktree_recreates_missing_branch_from_default() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(tmp.path()).await;

        let created = create_worktree(&repo_path(&tmp), &footprint_path(&tmp), 1, 99, "lost", None)
            .await
            .expect("create_worktree failed");

        // Both the directory AND the branch are gone.
        std::fs::remove_dir_all(&created.worktree_path).expect("remove worktree dir failed");
        let prune = tokio::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(tmp.path())
            .output()
            .await
            .expect("git worktree prune failed");
        assert!(prune.status.success(), "git worktree prune failed");
        let del = tokio::process::Command::new("git")
            .args(["branch", "-D", &created.branch])
            .current_dir(tmp.path())
            .output()
            .await
            .expect("git branch -D failed");
        assert!(del.status.success(), "git branch -D failed");

        let recreated = recreate_worktree(
            &repo_path(&tmp),
            &footprint_path(&tmp),
            1,
            99,
            &created.branch,
            "lost",
        )
        .await
        .expect("recreate_worktree from default branch failed");

        assert!(
            recreated.worktree_path.exists(),
            "worktree directory should be recreated"
        );

        let branch_list = tokio::process::Command::new("git")
            .args(["branch", "--list", &created.branch])
            .current_dir(tmp.path())
            .output()
            .await
            .expect("git branch --list failed");
        let stdout = String::from_utf8_lossy(&branch_list.stdout);
        assert!(
            stdout.contains(&created.branch),
            "branch should be recreated from the default branch tip, got: {stdout}"
        );

        assert_eq!(
            git_branch_in_worktree(&recreated.worktree_path).await,
            created.branch,
            "worktree should be on the recreated branch"
        );
    }

    #[tokio::test]
    async fn test_recreate_worktree_idempotent_when_dir_exists() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(tmp.path()).await;

        let created = create_worktree(&repo_path(&tmp), &footprint_path(&tmp), 1, 7, "keep", None)
            .await
            .expect("create_worktree failed");

        let result = recreate_worktree(
            &repo_path(&tmp),
            &footprint_path(&tmp),
            1,
            7,
            &created.branch,
            "keep",
        )
        .await
        .expect("recreate_worktree should be a no-op when the directory exists");

        assert!(result.worktree_path.exists());
        assert_eq!(result.branch, created.branch);
    }
}
