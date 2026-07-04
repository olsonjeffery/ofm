use crate::agents;

const PR_TEMPLATE: &str = include_str!("../../templates/pr.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestStatus {
    NoPr,
    ExistingPr { url: String },
}

pub fn build_pull_request_prompt(task_doc_content: &str, pr_status: &PullRequestStatus) -> String {
    let (context_line, create_or_verify_block) = match pr_status {
        PullRequestStatus::NoPr => (String::new(), build_create_block()),
        PullRequestStatus::ExistingPr { url } => {
            (format!("- Existing PR URL: {url}"), build_verify_block(url))
        }
    };

    let mut prompt = agents::build_prompt(PR_TEMPLATE, task_doc_content);
    prompt = prompt.replace("{{prContextLine}}", &context_line);
    prompt = prompt.replace("{{prCreateOrVerifyBlock}}", &create_or_verify_block);
    prompt
}

fn build_create_block() -> String {
    r#"### 1. Create PR

No PR exists yet for this task's branch. Create the PR:

1. Create the pull request:
   ```bash
   gh pr create --title "<descriptive-title>" --body "<description>"
   ```
2. Proceed to step 2 to monitor CI.
"#
    .to_string()
}

fn build_verify_block(url: &str) -> String {
    format!(
        r#"### 1. Verify Existing PR

A PR already exists at {url}. Skip creation and proceed directly to CI monitoring.
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_sections_present() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(prompt.contains("Create PR"));
        assert!(prompt.contains("Monitor CI Status"));
        assert!(prompt.contains("Handle CI Results"));
        assert!(prompt.contains("Check for Merge Conflicts"));
        assert!(prompt.contains("Important Constraints"));
    }

    #[test]
    fn test_no_pr_status_present() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(prompt.contains("No PR exists yet"));
        assert!(prompt.contains("gh pr create"));
        assert!(!prompt.contains("Existing PR URL"));
    }

    #[test]
    fn test_existing_pr_status_present() {
        let url = "https://github.com/owner/repo/pull/42";
        let prompt = build_pull_request_prompt(
            "",
            &PullRequestStatus::ExistingPr {
                url: url.to_string(),
            },
        );
        assert!(prompt.contains("Verify Existing PR"));
        assert!(prompt.contains(url));
        assert!(prompt.contains("Skip creation"));
    }

    #[test]
    fn test_completion_script_is_rust_cli() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(prompt.contains("omprint agent complete-pr"));
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_no_tsx_references() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_placeholder_substitution() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(!prompt.contains("{{taskDocPath}}"));
        assert!(!prompt.contains("{{taskId}}"));
        assert!(!prompt.contains("{{prContextLine}}"));
        assert!(!prompt.contains("{{prCreateOrVerifyBlock}}"));
        assert!(prompt.contains("storage/projects/{project_id}/tasks/task-{task_id}.md"));
        assert!(prompt.contains("{task_id}"));
    }

    #[test]
    fn test_bounded_iteration_mentioned() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(prompt.contains("max 20"));
        assert!(prompt.contains("max 10"));
        assert!(prompt.contains("max 3"));
    }

    #[test]
    fn test_never_merge_constraint() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(prompt.contains("Do NOT merge"));
    }

    #[test]
    fn test_empty_content_not_appended() {
        let prompt = build_pull_request_prompt("", &PullRequestStatus::NoPr);
        assert!(!prompt.contains("## Task Documentation"));
    }

    #[test]
    fn test_content_appended() {
        let content = "Some task content here";
        let prompt = build_pull_request_prompt(content, &PullRequestStatus::NoPr);
        assert!(prompt.contains("## Task Documentation"));
        assert!(prompt.contains(content));
    }
}
