pub mod guards;
pub mod recovery;
pub mod state_machine;

use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentType, RunStatus};
use crate::server::error::ServerError;
use crate::services::tasks;

pub const MAX_WORKFLOW_RUNS: i32 = 25;

pub enum NextAction {
    StartAgent(AgentType),
    Stop,
    Terminal,
}

pub async fn completion_handler(
    client: &Client,
    conversation_id: Uuid,
) -> Result<NextAction, ServerError> {
    let run = tasks::get_agent_run_by_conversation(client, &conversation_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    if run.status != RunStatus::Running {
        return Ok(NextAction::Terminal);
    }

    tasks::mark_agent_run_completed(client, &run.id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let task = tasks::get_task(client, &run.task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    if task.workflow_run_count >= MAX_WORKFLOW_RUNS {
        tasks::mark_task_blocked(client, &run.task_id)
            .await
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        return Ok(NextAction::Stop);
    }

    Ok(state_machine::next_agent(&task, &run.agent_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::AgentType;
    use crate::omp::session;
    use tempfile::TempDir;

    async fn make_client() -> (hiqlite::Client, Uuid, TempDir) {
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
        let project_id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, $3, $4)",
                hiqlite::params!(
                    project_id.to_string(),
                    user_id.to_string(),
                    "test-proj",
                    "/tmp/repo"
                ),
            )
            .await
            .unwrap();
        let task_id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO tasks (id, project_id, user_id, title) VALUES ($1, $2, $3, $4)",
                hiqlite::params!(
                    task_id.to_string(),
                    project_id.to_string(),
                    user_id.to_string(),
                    "test-task"
                ),
            )
            .await
            .unwrap();

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

        let action = completion_handler(&client, result.conversation_id)
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

        let action = completion_handler(&client, result.conversation_id)
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

        let action = completion_handler(&client, result.conversation_id)
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
                hiqlite::params!(task_id.to_string()),
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

        let action = completion_handler(&client, result.conversation_id)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));

        let task = tasks::get_task(&client, &task_id).await.unwrap();
        assert!(task.workflow_blocked);
    }
}
