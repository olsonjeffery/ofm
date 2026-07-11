const PLANIFICATION_TEMPLATE: &str = include_str!("../../templates/planification.md");

pub fn build_planning_prompt(
    task_doc_content: &str,
    task_doc_path: &str,
    task_id: &str,
    plan_template_path: &str,
) -> String {
    let mut prompt = PLANIFICATION_TEMPLATE
        .replace("{{taskDocPath}}", task_doc_path)
        .replace("{{taskId}}", task_id)
        .replace("{{planTemplatePath}}", plan_template_path);

    if !task_doc_content.is_empty() && task_doc_content.len() <= 4000 {
        prompt.push_str("\n\n## Original Task Document Content\n\n");
        prompt.push_str(task_doc_content);
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_required_sections_present() {
        let prompt = build_planning_prompt("", "path/to/doc.md", "42", "path/to/template.md");
        assert!(prompt.contains("## Primary Goal"));
        assert!(prompt.contains("## Planning Workflow"));
        assert!(prompt.contains("### Step 1: Explore"));
        assert!(prompt.contains("### Step 2: Clarify"));
        assert!(prompt.contains("### Step 3: Write the plan"));
        assert!(prompt.contains("### Step 4: Complete"));
    }

    #[test]
    fn test_original_request_preserved() {
        let content = "Implement user authentication";
        let prompt = build_planning_prompt(content, "path/to/doc.md", "42", "path/to/template.md");
        assert!(prompt.contains(content));
        assert!(prompt.contains("## Original Task Document Content"));
    }

    #[test]
    fn test_planning_constraints_enforced() {
        let prompt = build_planning_prompt("", "path/to/doc.md", "42", "path/to/template.md");
        assert!(prompt.contains("MUST NOT implement code"));
        assert!(prompt.contains("planning agent"));
        assert!(prompt.contains("Do not use Edit, Write, or TodoWrite"));
    }

    #[test]
    fn test_completion_signal_is_rust_cli() {
        let prompt = build_planning_prompt("", "path/to/doc.md", "42", "path/to/template.md");
        assert!(prompt.contains("ofm agent complete-plan"));
        assert!(!prompt.contains("tsx"));
    }

    #[test]
    fn test_placeholder_substitution() {
        let prompt = build_planning_prompt(
            "",
            "/home/user/task-42.md",
            "42",
            "/home/user/plan-template.md",
        );
        assert!(prompt.contains("/home/user/task-42.md"));
        assert!(prompt.contains("/home/user/plan-template.md"));
        assert!(!prompt.contains("{{taskDocPath}}"));
        assert!(!prompt.contains("{{planTemplatePath}}"));
        assert!(!prompt.contains("{{taskId}}"));
    }
}
