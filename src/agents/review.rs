use crate::agents;

const REVIEW_TEMPLATE: &str = include_str!("../../templates/review.md");

pub fn build_review_prompt(task_id: i64, task_doc_path: &str) -> String {
    let output = agents::build_prompt(REVIEW_TEMPLATE, task_doc_path);
    output.replace("{{taskId}}", &task_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_sections_present() {
        let prompt = build_review_prompt(1, "");
        assert!(prompt.contains("Read Task Documentation"));
        assert!(prompt.contains("Verify Checked Items"));
        assert!(prompt.contains("Run Unit Tests"));
        assert!(prompt.contains("Manual Testing"));
        assert!(prompt.contains("Evaluate Completion Status"));
        assert!(prompt.contains("Update Task Documentation"));
    }

    #[test]
    fn test_early_return_logic_present() {
        let prompt = build_review_prompt(1, "");
        assert!(prompt.contains("IN_PROGRESS"));
        assert!(prompt.contains("do NOT proceed"));
    }

    #[test]
    fn test_verdict_criteria_present() {
        let prompt = build_review_prompt(1, "");
        assert!(prompt.contains("READY"));
        assert!(prompt.contains("NEEDS_WORK"));
        assert!(prompt.contains("BLOCKED"));
    }

    #[test]
    fn test_completion_scripts_are_rust_cli() {
        let prompt = build_review_prompt(1, "");
        assert!(prompt.contains("ofm agent complete-workflow"));
        assert!(prompt.contains("ofm agent block-workflow"));
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_no_tsx_references() {
        let prompt = build_review_prompt(1, "");
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_playwright_testing_present() {
        let prompt = build_review_prompt(1, "");
        assert!(prompt.contains("Playwright MCP"));
    }

    #[test]
    fn test_placeholder_substitution() {
        let prompt = build_review_prompt(1, "");
        assert!(!prompt.contains("{{taskDocPath}}"));
        assert!(!prompt.contains("{{taskId}}"));
    }

    #[test]
    fn test_empty_content_not_appended() {
        let prompt = build_review_prompt(1, "");
        assert!(!prompt.contains("## Task Documentation"));
    }
}
