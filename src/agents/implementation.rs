use crate::agents;

const IMPLEMENTATION_TEMPLATE: &str = include_str!("../../templates/implementation.md");

pub fn build_implementation_prompt(task_doc_path: &str) -> String {
    agents::build_prompt(IMPLEMENTATION_TEMPLATE, task_doc_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_sections_present() {
        let prompt = build_implementation_prompt("");
        assert!(prompt.contains("## Instructions"));
        assert!(prompt.contains("Review Findings"));
        assert!(prompt.contains("To-Do List"));
        assert!(prompt.contains("Workflow Completion"));
    }

    #[test]
    fn test_constraints_enforced() {
        let prompt = build_implementation_prompt("");
        assert!(prompt.contains("Do NOT ask any questions"));
        assert!(prompt.contains("Task tool is disallowed"));
    }

    #[test]
    fn test_no_completion_script() {
        let prompt = build_implementation_prompt("");
        assert!(!prompt.contains("ofm agent complete-"));
        assert!(!prompt.contains("ofm agent block-"));
    }

    #[test]
    fn test_no_tsx_references() {
        let prompt = build_implementation_prompt("");
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_placeholder_substitution() {
        let prompt = build_implementation_prompt("");
        assert!(!prompt.contains("{{taskDocPath}}"));
        assert!(!prompt.contains("{{taskId}}"));
    }

    #[test]
    fn test_task_doc_content_not_appended() {
        let content = "Some task content here";
        let prompt = build_implementation_prompt(content);
        assert!(
            !prompt.contains("## Task Documentation"),
            "doc content should NOT be inlined"
        );
    }

    #[test]
    fn test_empty_content_not_appended() {
        let prompt = build_implementation_prompt("");
        assert!(!prompt.contains("## Task Documentation"));
    }
}
