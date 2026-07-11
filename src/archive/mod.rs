pub mod paths;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct ArchiveRoot {
    root: PathBuf,
}

impl ArchiveRoot {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn from_config() -> Self {
        let root = paths::get_archive_root();
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_project_archive(
        &self,
        project_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project_id = paths::sanitize_id(project_id)?;
        let tasks_path = self.root.join("projects").join(project_id).join("tasks");
        std::fs::create_dir_all(&tasks_path)?;
        #[cfg(unix)]
        {
            let _ = std::fs::set_permissions(&tasks_path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    pub fn read_task_doc(&self, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(std::fs::read_to_string(path)?)
    }

    pub fn write_task_doc(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn delete_task_doc(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let project_id = paths::sanitize_id(project_id)?;
        let task_id = paths::sanitize_id(task_id)?;
        let path = self
            .root
            .join("projects")
            .join(project_id)
            .join("tasks")
            .join(format!("task-{task_id}.md"));
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(&path)?;
        Ok(true)
    }

    pub fn delete_task_archive(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project_id = paths::sanitize_id(project_id)?;
        let task_id = paths::sanitize_id(task_id)?;
        let proj_root = self.root.join("projects").join(project_id);
        let doc_path = proj_root.join("tasks").join(format!("task-{task_id}.md"));
        let task_dir = proj_root.join("tasks").join(format!("task-{task_id}"));
        let recording_file = proj_root
            .join("recordings")
            .join(format!("task-{task_id}.webm"));

        if doc_path.exists() {
            std::fs::remove_file(&doc_path)?;
        }
        if task_dir.exists() {
            std::fs::remove_dir_all(&task_dir)?;
        }
        if recording_file.exists() {
            std::fs::remove_file(&recording_file)?;
        }
        Ok(())
    }

    pub fn get_dev_server_port(task_id: &str) -> Result<u16, Box<dyn std::error::Error>> {
        let id: u32 = task_id.parse()?;
        Ok(3100 + (id % 900) as u16)
    }

    pub fn task_doc_path(&self, project_id: &str, task_id: &str) -> PathBuf {
        self.root
            .join("projects")
            .join(project_id)
            .join("tasks")
            .join(format!("task-{task_id}.md"))
    }

    pub fn build_context_prompt(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let task_doc_path = self.task_doc_path(project_id, task_id);
        let port = Self::get_dev_server_port(task_id)?;

        let mut sections: Vec<String> = Vec::new();

        // Task Plan File section
        let task_plan = format!(
            "\
## Task Plan File

The canonical task plan — also known as the specification for this task — is stored at:
`{}`

**At the start of this conversation, before answering the user's first message, you MUST read this file in full using the Read tool.** It contains the requirements, constraints, and prior decisions you need to do this work correctly. Do not skip this step even if the user's first message looks unrelated to the plan.

When the user refers to the \"task plan\", \"task doc\", \"task spec\", \"specifications\", or asks you to read or update the task documentation, this is the file — read or edit it directly with the Read/Edit tool. Do NOT search for it elsewhere; the path above is authoritative.

Note: any `.bottega/tasks/*.md` files inside the repo itself are legacy from before task docs were moved to a central archive. Ignore them — the path above is the only source of truth.",
            task_doc_path.display()
        );
        sections.push(task_plan);

        // Input Files section
        let tasks_root = self.root.join("projects").join(project_id).join("tasks");
        let input_dir = tasks_root
            .join(format!("task-{task_id}"))
            .join("input_files");
        if input_dir.exists() && input_dir.is_dir() {
            let mut entries: Vec<String> = Vec::new();
            for entry in std::fs::read_dir(&input_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                entries.push(format!("- `{}`", name));
            }
            entries.sort();
            let input_section = format!(
                "\
## Input Files

The following input files are available in `{}`:
{}",
                input_dir.display(),
                entries.join("\n")
            );
            sections.push(input_section);
        }

        // AGENTS.md section
        let agents_md = "\
## AGENTS.md Guidance

When you open or explore a directory, check if it contains an `AGENTS.md` file.
If it does, read it in full before proceeding. These files contain project-specific
instructions, conventions, preferences, and setup notes that the project maintainer
has curated for AI agents working in this codebase. They take precedence over
generic instructions where they overlap.

This is especially important in the worktree root directory, where an `AGENTS.md`
may define agent-level instructions for this specific task or project.";
        sections.push(agents_md.to_string());

        // Testing Configuration section
        let testing = format!(
            "\
## Testing Configuration

- **Task ID:** {task_id}
- **Dev Server Port:** {port}

When running Playwright MCP tests, start the project's dev server on port {port}:
1. Check project files (README, package.json, Procfile) for the start command
2. Start server with your assigned port (e.g., `OFM_PORT={port} cargo run` or `OFM_PORT={port} bin/dev`)
3. Run Playwright tests against `http://localhost:{port}`
4. Stop the server when testing is complete: `lsof -ti:{port} | xargs kill -9 2>/dev/null || true`

### Test Execution Best Practices

When running the project's test suite:

1. **Run targeted tests first**: Only run test files related to your changes. This gives fast feedback.
2. **Full suite = background**: When running the complete test suite, ALWAYS use `run_in_background: true` on the Bash tool. Full suites can take 5-15 minutes and will exceed the default timeout.
3. **Wait for backgrounded tests before re-launching**: If a test command gets backgrounded (you receive a task ID), wait for it to complete using TaskOutput with `block: true`. Do NOT start another test run while one is still running — parallel suites compete for resources and take even longer. Only re-launch if the previous run completed and failed.
4. **Use fail-fast flags**: If the test framework supports it, use a fail-fast option to exit on first failure.
5. **Set generous timeouts**: If not using `run_in_background`, set `timeout: 600000` (10 minutes) for full test suites.",
        );
        sections.push(testing);

        Ok(sections.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read_task_doc() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let path = dir.path().join("test-doc.md");

        root.write_task_doc(&path, "hello world").unwrap();
        let content = root.read_task_doc(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_read_non_existent_doc() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let path = dir.path().join("nonexistent.md");

        let content = root.read_task_doc(&path).unwrap();
        assert_eq!(content, "");
    }

    #[test]
    fn test_overwrite_task_doc() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let path = dir.path().join("overwrite.md");

        root.write_task_doc(&path, "first version").unwrap();
        root.write_task_doc(&path, "second version").unwrap();
        let content = root.read_task_doc(&path).unwrap();
        assert_eq!(content, "second version");
    }

    #[test]
    fn test_delete_task_doc_exists() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let path = dir.path().join("projects").join("p1").join("tasks");
        std::fs::create_dir_all(&path).unwrap();
        let doc_path = path.join("task-1.md");
        std::fs::write(&doc_path, "content").unwrap();

        let deleted = root.delete_task_doc("p1", "1").unwrap();
        assert!(deleted);
        assert!(!doc_path.exists());
    }

    #[test]
    fn test_delete_task_doc_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());

        let deleted = root.delete_task_doc("p1", "999").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_delete_task_archive() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let tasks_path = dir.path().join("projects").join("p1").join("tasks");
        std::fs::create_dir_all(&tasks_path).unwrap();
        let doc_path = tasks_path.join("task-5.md");
        std::fs::write(&doc_path, "content").unwrap();
        let task_subdir = tasks_path.join("task-5");
        std::fs::create_dir_all(&task_subdir).unwrap();
        std::fs::write(task_subdir.join("input.txt"), "data").unwrap();

        root.delete_task_archive("p1", "5").unwrap();
        assert!(!doc_path.exists());
        assert!(!task_subdir.exists());
    }

    #[test]
    fn test_ensure_project_archive() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());

        root.ensure_project_archive("test-proj").unwrap();
        let expected = dir.path().join("projects").join("test-proj").join("tasks");
        assert!(expected.exists());
        assert!(expected.is_dir());
    }

    #[test]
    fn test_get_dev_server_port() {
        assert_eq!(ArchiveRoot::get_dev_server_port("7").unwrap(), 3107);
        assert_eq!(ArchiveRoot::get_dev_server_port("88").unwrap(), 3188);
        assert_eq!(ArchiveRoot::get_dev_server_port("907").unwrap(), 3107);
        assert!(ArchiveRoot::get_dev_server_port("abc").is_err());
    }

    #[test]
    fn test_build_context_prompt_contains_sections() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());

        let prompt = root.build_context_prompt("p1", "42").unwrap();
        assert!(prompt.contains("Task Plan File"));
        assert!(prompt.contains("task-42.md"));
        assert!(prompt.contains("Testing Configuration"));
        assert!(prompt.contains("**Task ID:** 42"));
        assert!(prompt.contains("**Dev Server Port:** 3142"));
        assert!(prompt.contains("---"));
    }

    #[test]
    fn test_build_context_prompt_no_input_files_when_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());

        let prompt = root.build_context_prompt("p1", "1").unwrap();
        assert!(!prompt.contains("Input Files"));
    }

    #[test]
    fn test_write_creates_dirs() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = ArchiveRoot::new(dir.path().to_path_buf());
        let nested = dir.path().join("a").join("b").join("c.md");

        root.write_task_doc(&nested, "deep").unwrap();
        assert!(nested.exists());
        assert_eq!(root.read_task_doc(&nested).unwrap(), "deep");
    }
}
