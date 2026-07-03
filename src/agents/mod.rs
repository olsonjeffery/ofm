pub mod implementation;
pub mod planning;
pub mod review;

/// Builds a prompt from a template string by substituting standard placeholders
/// (`{{taskDocPath}}` and `{{taskId}}`) and optionally appending the task
/// documentation content under a `## Task Documentation` heading.
pub(crate) fn build_prompt(template: &str, task_doc_content: &str) -> String {
    let mut prompt = template
        .replace(
            "{{taskDocPath}}",
            "storage/projects/{project_id}/tasks/task-{task_id}.md",
        )
        .replace("{{taskId}}", "{task_id}");

    if !task_doc_content.is_empty() {
        prompt.push_str("\n\n## Task Documentation\n\n");
        prompt.push_str(task_doc_content);
    }

    prompt
}
