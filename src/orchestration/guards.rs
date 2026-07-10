use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::Task;
use crate::orchestration::MAX_WORKFLOW_RUNS;
use crate::server::error::ServerError;
use crate::services::tasks;

pub async fn one_running_per_task(client: &Client, task_id: Uuid) -> Result<(), ServerError> {
    match tasks::get_running_agent_for_task(client, &task_id).await {
        Ok(Some(_)) => Err(ServerError::Conflict(
            "an agent is already running for this task".into(),
        )),
        Ok(None) => Ok(()),
        Err(e) => Err(ServerError::Internal(e.to_string())),
    }
}

pub fn iteration_cap(task: &Task) -> Result<(), ServerError> {
    if task.workflow_run_count >= MAX_WORKFLOW_RUNS {
        Err(ServerError::Conflict("max iterations reached".into()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::{AgentType, Task};
    use crate::services::session;
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
    async fn test_one_running_per_task_ok() {
        let (client, task_id, _tmp) = make_client().await;
        let result = one_running_per_task(&client, task_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_one_running_per_task_conflict() {
        let (client, task_id, _tmp) = make_client().await;

        session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let result = one_running_per_task(&client, task_id).await;
        assert!(result.is_err());
        match result {
            Err(ServerError::Conflict(msg)) => {
                assert!(msg.contains("already running"));
            }
            _ => panic!("expected Conflict error"),
        }
    }

    #[test]
    fn test_iteration_cap_under_limit() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            title: "test".into(),
            status: "pending".into(),
            workflow_complete: false,
            workflow_blocked: false,
            workflow_run_count: 10,
            planification_complete: false,
            pr_agent_complete: false,
            refinement_complete: false,
            yolo_mode: false,
            created_at: chrono::Utc::now().naive_utc(),
        };
        assert!(iteration_cap(&task).is_ok());
    }

    #[test]
    fn test_iteration_cap_at_limit() {
        let task = Task {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            title: "test".into(),
            status: "pending".into(),
            workflow_complete: false,
            workflow_blocked: false,
            workflow_run_count: 25,
            planification_complete: false,
            pr_agent_complete: false,
            refinement_complete: false,
            yolo_mode: false,
            created_at: chrono::Utc::now().naive_utc(),
        };
        match iteration_cap(&task) {
            Err(ServerError::Conflict(msg)) => {
                assert!(msg.contains("max iterations"));
            }
            _ => panic!("expected Conflict error"),
        }
    }
}
