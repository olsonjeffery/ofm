use hiqlite::Client;

pub async fn recover_orphaned_runs(client: &Client) -> Result<usize, hiqlite::Error> {
    let count = crate::services::tasks::sweep_running_agent_runs_to_failed(client).await?;
    tracing::info!("Orphan recovery: {} agent runs swept to failed", count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::{AgentType, RunStatus};
    use tempfile::TempDir;
    use uuid::Uuid;

    async fn make_client() -> (hiqlite::Client, TempDir) {
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

        let now = chrono::Utc::now().naive_utc().to_string();
        client
            .execute(
                "INSERT INTO task_agent_runs (id, task_id, agent_type, status, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    task_id,
                    AgentType::Implementation.to_string(),
                    RunStatus::Running.to_string(),
                    &now
                ),
            )
            .await
            .unwrap();

        (client, tmp)
    }

    #[tokio::test]
    async fn test_sweep_orphaned_runs() {
        let (client, _tmp) = make_client().await;

        let count = recover_orphaned_runs(&client).await.unwrap();
        assert_eq!(count, 1);

        let mut rows = client
            .query_raw(
                "SELECT status, completed_at FROM task_agent_runs",
                hiqlite::params!(),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let status: String = rows[0].get("status");
        let completed_at: Option<String> = rows[0].get("completed_at");
        assert_eq!(status, "failed");
        assert!(completed_at.is_some());
    }

    #[tokio::test]
    async fn test_sweep_no_orphans() {
        let (client, _tmp) = make_client().await;

        // Mark the run completed first so there are no orphans
        client
            .execute(
                "UPDATE task_agent_runs SET status = $1",
                hiqlite::params!(RunStatus::Completed.to_string()),
            )
            .await
            .unwrap();

        let count = recover_orphaned_runs(&client).await.unwrap();
        assert_eq!(count, 0);
    }
}
