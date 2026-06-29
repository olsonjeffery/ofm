use std::path::{Path, PathBuf};

use tokio::process::Command;
use uuid::Uuid;

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

/// XOR-fold a UUID's 128 bits into a 32-bit integer for worktree path/branch naming.
pub fn uuid_to_u32(uuid: &Uuid) -> u32 {
    let v = uuid.as_u128();
    (v as u32) ^ ((v >> 32) as u32) ^ ((v >> 64) as u32) ^ ((v >> 96) as u32)
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

pub fn get_worktree_path(repo_path: &str, project_id: u32, task_id: u32) -> PathBuf {
    let worktrees_root = format!("{}-worktrees", repo_path.trim_end_matches('/'));
    PathBuf::from(worktrees_root).join(format!("project-{project_id}/task-{task_id}/"))
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
    project_id: u32,
    task_id: u32,
    title: &str,
    base_branch: Option<&str>,
) -> Result<CreateWorktreeResult, Box<dyn std::error::Error + Send + Sync>> {
    let worktree_path = get_worktree_path(repo_path, project_id, task_id);
    if let Some(parent) = worktree_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let sanitized = sanitize_title(title);
    let branch = format!("task/{task_id}-{sanitized}");
    valid_branch_name(&branch)?;

    let base = match base_branch {
        Some(b) => b.to_string(),
        None => detect_default_branch(repo_path).await?,
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
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {stderr}").into());
    }

    symlink_env_files(repo_path, &worktree_path).await;
    create_gitignored_dirs(&worktree_path).await;
    copy_dependencies_background(repo_path, &worktree_path).await;

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
    project_id: u32,
    task_id: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worktree_path = get_worktree_path(repo_path, project_id, task_id);

    if !worktree_path.exists() {
        return Ok(());
    }

    let branch_output = Command::new("git")
        .args(["branch", "--show-current"])
        .env("GIT_DISABLE_HOOKS", "1")
        .current_dir(&worktree_path)
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
        let path = get_worktree_path("/repo", 1, 42);
        assert_eq!(path, PathBuf::from("/repo-worktrees/project-1/task-42/"));
    }

    #[test]
    fn test_get_worktree_path_trailing_slash() {
        let path = get_worktree_path("/repo/", 2, 99);
        assert_eq!(path, PathBuf::from("/repo-worktrees/project-2/task-99/"));
    }

    #[test]
    fn test_get_worktree_path_large_ids() {
        let path = get_worktree_path("/home/projects/my-app", 9999, 888888);
        assert_eq!(
            path,
            PathBuf::from("/home/projects/my-app-worktrees/project-9999/task-888888/")
        );
    }

    #[test]
    fn test_sanitize_title_mixed_case() {
        assert_eq!(sanitize_title("ABC def GHI"), "abc-def-ghi");
    }
}
