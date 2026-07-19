use crate::agents;

const REFINEMENT_TEMPLATE: &str = include_str!("../../templates/refinement.md");

pub fn build_refinement_prompt(task_id: i64, task_doc_path: &str) -> String {
    let output = agents::build_prompt(REFINEMENT_TEMPLATE, task_doc_path);
    output.replace("{{taskId}}", &task_id.to_string())
}
