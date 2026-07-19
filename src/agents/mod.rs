pub mod implementation;
pub mod planning;
pub mod pull_request;
pub mod refinement;
pub mod review;

/// Builds a prompt from a template string by substituting standard placeholders
/// (`{{taskDocPath}}` and `{{taskId}}`). The agent is instructed to read the
/// task doc file itself — content is NOT inlined.
pub(crate) fn build_prompt(template: &str, task_doc_path: &str) -> String {
    template.replace("{{taskDocPath}}", task_doc_path)
}
