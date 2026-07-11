pub mod implementation;
pub mod planning;
pub mod pull_request;
pub mod review;

/// Builds a prompt from a template string by substituting standard placeholders
/// (`{{taskDocPath}}` and `{{taskId}}`). The agent is instructed to read the
/// task doc file itself — content is NOT inlined.
pub(crate) fn build_prompt(template: &str, _task_doc_content: &str) -> String {
    template
        .replace(
            "{{taskDocPath}}",
            "storage/projects/{project_id}/tasks/task-{task_id}.md",
        )
        .replace("{{taskId}}", "{task_id}")
}
