use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentType, Conversation, RunStatus};

pub struct SessionStart {
    pub conversation_id: Uuid,
    pub session_id: String,
}

pub async fn start_session(
    client: &Client,
    task_id: i64,
    model: &str,
    effort: &str,
    agent_type: AgentType,
) -> Result<SessionStart, hiqlite::Error> {
    let conversation_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let session_id = format!("UNSET_{}", Uuid::new_v4());
    let now = chrono::Utc::now().naive_utc().to_string();

    client
        .execute(
            "INSERT INTO conversations (id, task_id, provider_session_id, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            hiqlite::params!(
                conversation_id.to_string(),
                task_id,
                &session_id,
                model,
                effort,
                &now,
                &now
            ),
        )
        .await?;

    client
        .execute(
            "INSERT INTO task_agent_runs (id, task_id, agent_type, status, conversation_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                run_id.to_string(),
                task_id,
                agent_type.to_string(),
                RunStatus::Running.to_string(),
                conversation_id.to_string(),
                &now
            ),
        )
        .await?;

    Ok(SessionStart {
        conversation_id,
        session_id,
    })
}

pub async fn resume_session(
    client: &Client,
    conversation_id: Uuid,
) -> Result<Conversation, hiqlite::Error> {
    client
        .query_map_one::<Conversation, _>(
                "SELECT id, task_id, provider_session_id, model, effort, name, created_at, updated_at FROM conversations WHERE id = $1",
            hiqlite::params!(conversation_id.to_string()),
        )
        .await
}

pub async fn abort_session(client: &Client, session_id: &str) -> Result<(), hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "UPDATE task_agent_runs SET status = $1, completed_at = $2 WHERE conversation_id = (SELECT id FROM conversations WHERE provider_session_id = $3)",
            hiqlite::params!(RunStatus::Failed.to_string(), &now, session_id),
        )
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

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
            let now = chrono::Utc::now().naive_utc().to_string();
            client
                .execute(
                    "INSERT INTO tasks (id, project_id, user_id, title, created_at) VALUES ($1, $2, $3, $4, $5)",
                    hiqlite::params!(id, project_id, user_id.to_string(), "test-task", &now),
                )
                .await
                .unwrap();
            id
        };

        (client, task_id, tmp)
    }

    #[tokio::test]
    async fn test_start_session_creates_conversation() {
        let (client, task_id, _tmp) = make_client().await;

        let result = start_session(
            &client,
            task_id,
            "test-model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let conv: Conversation = client
            .query_map_one(
            "SELECT id, task_id, provider_session_id, model, effort, name, created_at, updated_at FROM conversations WHERE id = $1",
                hiqlite::params!(result.conversation_id.to_string()),
            )
            .await
            .unwrap();

        assert_eq!(conv.task_id, task_id);
        assert_eq!(conv.model, "test-model");
        assert_eq!(conv.effort, "balanced");
        assert_eq!(conv.provider_session_id, Some(result.session_id.clone()));
        assert!(
            result.session_id.starts_with("UNSET_"),
            "session_id should start with UNSET_ prefix"
        );
        assert_eq!(conv.id, result.conversation_id);
    }

    #[tokio::test]
    async fn test_start_session_creates_task_agent_run() {
        let (client, task_id, _tmp) = make_client().await;

        let result = start_session(
            &client,
            task_id,
            "test-model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let mut runs = client
            .query_raw(
                "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE conversation_id = $1",
                hiqlite::params!(result.conversation_id.to_string()),
            )
            .await
            .unwrap();

        assert_eq!(runs.len(), 1, "expected exactly one task_agent_run");
        let status: String = runs[0].get("status");
        let conv_id: String = runs[0].get("conversation_id");
        assert_eq!(status, "running");
        assert_eq!(conv_id, result.conversation_id.to_string());
    }

    #[tokio::test]
    async fn test_resume_session() {
        let (client, task_id, _tmp) = make_client().await;

        let result = start_session(
            &client,
            task_id,
            "resume-model",
            "fast",
            AgentType::Planification,
        )
        .await
        .unwrap();

        let conv = resume_session(&client, result.conversation_id)
            .await
            .unwrap();

        assert_eq!(conv.id, result.conversation_id);
        assert_eq!(conv.model, "resume-model");
        assert_eq!(conv.effort, "fast");
    }

    #[tokio::test]
    async fn test_resume_session_not_found() {
        let (client, _task_id, _tmp) = make_client().await;
        let fake_id = Uuid::new_v4();

        let result = resume_session(&client, fake_id).await;
        assert!(
            result.is_err(),
            "expected error for nonexistent conversation"
        );
    }

    #[tokio::test]
    async fn test_abort_session_marks_run_failed() {
        let (client, task_id, _tmp) = make_client().await;

        let result = start_session(
            &client,
            task_id,
            "abort-model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        abort_session(&client, &result.session_id).await.unwrap();

        let mut runs = client
            .query_raw(
                "SELECT status, completed_at FROM task_agent_runs WHERE conversation_id = $1",
                hiqlite::params!(result.conversation_id.to_string()),
            )
            .await
            .unwrap();

        assert_eq!(runs.len(), 1);
        let status: String = runs[0].get("status");
        let completed_at: Option<String> = runs[0].get("completed_at");
        assert_eq!(status, "failed");
        assert!(
            completed_at.is_some(),
            "completed_at should be set after abort"
        );
    }
}
