pub mod guards;
pub mod recovery;
pub mod state_machine;

use std::collections::HashMap;
use std::sync::Arc;

use hiqlite::Client;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::db::schema::{AgentType, RunStatus};
use crate::providers::registry;
use crate::providers::LlmProvider;
use crate::server::error::ServerError;
use crate::services::tasks;

pub const MAX_WORKFLOW_RUNS: i32 = 25;

pub enum NextAction {
    StartAgent(AgentType),
    Stop,
    Terminal,
}

pub fn internal_err(e: impl std::fmt::Display) -> ServerError {
    ServerError::Internal(e.to_string())
}

pub async fn completion_handler(
    client: &Client,
    conversation_id: Uuid,
    active_sessions: &Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>>,
) -> Result<NextAction, ServerError> {
    let run = tasks::get_agent_run_by_conversation(client, &conversation_id)
        .await
        .map_err(internal_err)?;

    if run.status != RunStatus::Running {
        return Ok(NextAction::Terminal);
    }

    tasks::mark_agent_run_completed(client, &run.id)
        .await
        .map_err(internal_err)?;

    if let Some(mut provider) = active_sessions
        .lock()
        .await
        .remove(&conversation_id.to_string())
    {
        if let Err(e) = provider.shutdown().await {
            tracing::warn!("Error shutting down provider for conversation {conversation_id}: {e}");
        }
    }

    let task = tasks::get_task(client, run.task_id)
        .await
        .map_err(internal_err)?;

    if task.workflow_run_count >= MAX_WORKFLOW_RUNS {
        tasks::mark_task_blocked(client, run.task_id)
            .await
            .map_err(internal_err)?;
        return Ok(NextAction::Stop);
    }

    let config_statuses =
        registry::resolve_agent_config_statuses(client, task.user_id, task.project_id).await;
    Ok(state_machine::next_agent(
        &task,
        &run.agent_type,
        &config_statuses,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::AgentType;
    use crate::services::session;
    use tempfile::TempDir;

    fn empty_sessions() -> Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

    async fn make_client() -> (hiqlite::Client, i64, TempDir) {
        let tmp = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-123".into(),
            secret_api: "test-api-secret-123".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();

        let user_id = db::ensure_default_user(&client).await.unwrap();

        let project_id: i64 = {
            let mut rows = client
                .query_raw(
                    "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM projects",
                    hiqlite::params!(),
                )
                .await
                .unwrap();
            let id = rows
                .first_mut()
                .map(|r| r.get::<i64>("next_id"))
                .unwrap_or(1);
            client
                .execute(
                    "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, $3, $4)",
                    hiqlite::params!(id, user_id.to_string(), "test-proj", "/tmp/repo"),
                )
                .await
                .unwrap();
            id
        };

        let task_id: i64 = {
            let mut rows = client
                .query_raw(
                    "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM tasks",
                    hiqlite::params!(),
                )
                .await
                .unwrap();
            let id = rows
                .first_mut()
                .map(|r| r.get::<i64>("next_id"))
                .unwrap_or(1);
            client
                .execute(
                    "INSERT INTO tasks (id, project_id, user_id, title) VALUES ($1, $2, $3, $4)",
                    hiqlite::params!(id, project_id, user_id.to_string(), "test-task"),
                )
                .await
                .unwrap();
            id
        };

        (client, task_id, tmp)
    }

    #[tokio::test]
    async fn test_completion_handler_running_to_completed() {
        let (client, task_id, _tmp) = make_client().await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        // Seed a review config so the phase-skip check passes
        let now = chrono::Utc::now().naive_utc().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                hiqlite::params!(
                    uuid::Uuid::new_v4().to_string(),
                    "review",
                    "opencode",
                    "test.json",
                    "global",
                    "gpt-4",
                    "balanced",
                    &now,
                    &now,
                ),
            )
            .await
            .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        let run = tasks::get_agent_run_by_conversation(&client, &result.conversation_id)
            .await
            .unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[tokio::test]
    async fn test_completion_handler_failed_no_chain() {
        let (client, task_id, _tmp) = make_client().await;

        let result =
            session::start_session(&client, task_id, "model", "balanced", AgentType::Review)
                .await
                .unwrap();

        tasks::mark_agent_run_failed(
            &client,
            &tasks::get_agent_run_by_conversation(&client, &result.conversation_id)
                .await
                .unwrap()
                .id,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Terminal));
    }

    #[tokio::test]
    async fn test_completion_handler_planning_stops() {
        let (client, task_id, _tmp) = make_client().await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Planification,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));
    }

    #[tokio::test]
    async fn test_completion_handler_iteration_cap_auto_blocks() {
        let (client, task_id, _tmp) = make_client().await;

        client
            .execute(
                "UPDATE tasks SET workflow_run_count = 25 WHERE id = $1",
                hiqlite::params!(task_id),
            )
            .await
            .unwrap();

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));

        let task = tasks::get_task(&client, task_id).await.unwrap();
        assert!(task.workflow_blocked);
    }
}
