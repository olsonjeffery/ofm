use crate::cli::{AgentAction, Command};

pub async fn handle_command(cmd: Command) -> Result<(), String> {
    let Command::Agent { action } = cmd;
    let (endpoint, task_id) = match action {
        AgentAction::CompletePlan { task_id } => ("complete-plan", task_id),
        AgentAction::CompleteWorkflow { task_id } => ("complete-workflow", task_id),
        AgentAction::BlockWorkflow { task_id } => ("block-workflow", task_id),
        AgentAction::CompletePr { task_id } => ("complete-pr", task_id),
    };
    let base_url = std::env::var("OMPRINT_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3183".into());
    let url = format!("{}/api/tasks/{}/{}", base_url, task_id, endpoint);
    let client = reqwest::Client::new();
    let resp = client.post(&url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("{}: {}", status, body))
    }
}
