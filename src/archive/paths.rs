use std::path::PathBuf;

fn expand_tilde(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{stripped}");
        }
    }
    path.to_string()
}

pub fn sanitize_id(input: &str) -> Result<&str, Box<dyn std::error::Error>> {
    if input.contains('/') || input.contains('\\') || input == ".." || input.starts_with('.') {
        return Err(format!("invalid id: {input}").into());
    }
    Ok(input)
}

pub fn get_archive_root() -> PathBuf {
    let raw = std::env::var("OMPRINT_ARCHIVE_ROOT").unwrap_or("~/.omprint".into());
    PathBuf::from(expand_tilde(&raw))
}

pub fn get_project_archive_path(project_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let project_id = sanitize_id(project_id)?;
    Ok(get_archive_root().join("projects").join(project_id))
}

pub fn get_archive_tasks_folder_path(project_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(get_project_archive_path(project_id)?.join("tasks"))
}

pub fn get_task_doc_path(project_id: &str, task_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(get_archive_tasks_folder_path(project_id)?.join(format!("task-{task_id}.md")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_get_archive_root_env_set() {
        let previous = env::var("OMPRINT_ARCHIVE_ROOT").ok();
        env::set_var("OMPRINT_ARCHIVE_ROOT", "/custom/archive");
        let result = get_archive_root();
        assert_eq!(result, PathBuf::from("/custom/archive"));
        if let Some(val) = previous {
            env::set_var("OMPRINT_ARCHIVE_ROOT", val);
        } else {
            env::remove_var("OMPRINT_ARCHIVE_ROOT");
        }
    }

    #[test]
    fn test_get_archive_root_env_unset() {
        let previous = env::var("OMPRINT_ARCHIVE_ROOT").ok();
        env::remove_var("OMPRINT_ARCHIVE_ROOT");
        let home = env::var("HOME").unwrap();
        let result = get_archive_root();
        assert_eq!(result, PathBuf::from(format!("{}/.omprint", home)));
        if let Some(val) = previous {
            env::set_var("OMPRINT_ARCHIVE_ROOT", val);
        }
    }

    #[test]
    fn test_get_task_doc_path() {
        let previous = env::var("OMPRINT_ARCHIVE_ROOT").ok();
        env::set_var("OMPRINT_ARCHIVE_ROOT", "/base");
        let result = get_task_doc_path("42", "7").unwrap();
        assert_eq!(
            result,
            PathBuf::from("/base/projects/42/tasks/task-7.md")
        );
        if let Some(val) = previous {
            env::set_var("OMPRINT_ARCHIVE_ROOT", val);
        } else {
            env::remove_var("OMPRINT_ARCHIVE_ROOT");
        }
    }

    #[test]
    fn test_get_project_archive_path() {
        let previous = env::var("OMPRINT_ARCHIVE_ROOT").ok();
        env::set_var("OMPRINT_ARCHIVE_ROOT", "/base");
        let result = get_project_archive_path("proj-1").unwrap();
        assert_eq!(result, PathBuf::from("/base/projects/proj-1"));
        if let Some(val) = previous {
            env::set_var("OMPRINT_ARCHIVE_ROOT", val);
        } else {
            env::remove_var("OMPRINT_ARCHIVE_ROOT");
        }
    }

    #[test]
    fn test_get_archive_tasks_folder_path() {
        let previous = env::var("OMPRINT_ARCHIVE_ROOT").ok();
        env::set_var("OMPRINT_ARCHIVE_ROOT", "/base");
        let result = get_archive_tasks_folder_path("p1").unwrap();
        assert_eq!(result, PathBuf::from("/base/projects/p1/tasks"));
        if let Some(val) = previous {
            env::set_var("OMPRINT_ARCHIVE_ROOT", val);
        } else {
            env::remove_var("OMPRINT_ARCHIVE_ROOT");
        }
    }

    #[test]
    fn test_sanitize_id_rejects_path_traversal() {
        assert!(sanitize_id("../etc").is_err());
        assert!(sanitize_id("foo/bar").is_err());
        assert!(sanitize_id("..").is_err());
        assert!(sanitize_id(".hidden").is_err());
        assert!(sanitize_id("normal-id").is_ok());
        assert!(sanitize_id("task-42").is_ok());
    }

    #[test]
    fn test_expand_tilde() {
        let home = env::var("HOME").unwrap();
        assert_eq!(
            expand_tilde("~/.omprint"),
            format!("{}/.omprint", home)
        );
        assert_eq!(expand_tilde("/absolute/path"), "/absolute/path");
        assert_eq!(expand_tilde("relative/path"), "relative/path");
    }
}
