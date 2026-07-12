use crate::agents;

const REFINEMENT_TEMPLATE: &str = include_str!("../../templates/refinement.md");

pub fn build_refinement_prompt(task_doc_content: &str) -> String {
    agents::build_prompt(REFINEMENT_TEMPLATE, task_doc_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_sections_present() {
        let prompt = build_refinement_prompt("");
        assert!(prompt.contains("Review Findings"));
        assert!(prompt.contains("## Instructions"));
        assert!(prompt.contains("task documentation"));
    }

    #[test]
    fn test_task_doc_path_referenced() {
        let prompt = build_refinement_prompt("");
        assert!(!prompt.contains("{{taskDocPath}}"));
        assert!(prompt.contains("storage/projects/{project_id}/tasks/task-{task_id}.md"));
    }

    #[test]
    fn test_empty_content_not_appended() {
        let prompt = build_refinement_prompt("");
        assert!(!prompt.contains("## Task Documentation"));
    }

    #[test]
    fn test_content_not_appended() {
        let content = "Some review findings here";
        let prompt = build_refinement_prompt(content);
        assert!(!prompt.contains("## Task Documentation"));
        assert!(!prompt.contains(content));
    }
}
