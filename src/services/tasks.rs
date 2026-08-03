use std::collections::HashMap;

use crate::db::schema::{
    ActiveAgent, AgentType, Conversation, ConversationWithRun, RunStatus, Task, TaskAgentRun,
    Worktree,
};
use hiqlite::Client;
use uuid::Uuid;

fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub async fn create_task(
    client: &Client,
    project_id: i64,
    user_id: &Uuid,
    title: &str,
    status: &str,
) -> Result<Task, hiqlite::Error> {
    let id: i64 = {
        let mut rows = client
            .query_raw(
                "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM tasks",
                hiqlite::params!(),
            )
            .await?;
        rows.first_mut()
            .map(|r| r.get::<i64>("next_id"))
            .unwrap_or(1)
    };
    client
        .execute(
            "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(id, project_id, user_id.to_string(), title, status, utc_now()),
        )
        .await?;
    get_task(client, id).await
}

pub async fn list_tasks(client: &Client, project_id: i64) -> Result<Vec<Task>, hiqlite::Error> {
    client
        .query_map::<Task, _>(
            "SELECT id, project_id, user_id, title, status, workflow_blocked, workflow_run_count, yolo_mode, created_at FROM tasks WHERE project_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(project_id),
        )
        .await
}

pub async fn get_task(client: &Client, task_id: i64) -> Result<Task, hiqlite::Error> {
    client
        .query_map_one::<Task, _>(
            "SELECT id, project_id, user_id, title, status, workflow_blocked, workflow_run_count, yolo_mode, created_at FROM tasks WHERE id = $1",
            hiqlite::params!(task_id),
        )
        .await
}

pub async fn update_task(
    client: &Client,
    task_id: i64,
    title: Option<&str>,
    status: Option<&str>,
) -> Result<Task, hiqlite::Error> {
    if title.is_none() && status.is_none() {
        return get_task(client, task_id).await;
    }
    client
        .execute(
            "UPDATE tasks SET title = COALESCE($1, title), status = COALESCE($2, status) WHERE id = $3",
            hiqlite::params!(title, status, task_id),
        )
        .await?;
    get_task(client, task_id).await
}

pub async fn delete_task(client: &Client, task_id: i64) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute("DELETE FROM tasks WHERE id = $1", hiqlite::params!(task_id))
        .await?;
    Ok(rows > 0)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_worktree(
    client: &Client,
    id: &Uuid,
    project_id: i64,
    task_id: i64,
    worktree_path: &str,
    repo_path: &str,
    branch: &str,
) -> Result<Worktree, hiqlite::Error> {
    client
        .execute(
            "INSERT INTO worktrees (id, project_id, task_id, worktree_path, repo_path, branch) VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                id.to_string(),
                project_id,
                task_id,
                worktree_path,
                repo_path,
                branch
            ),
        )
        .await?;
    get_worktree_by_task(client, task_id).await
}

pub async fn get_worktree_by_task(
    client: &Client,
    task_id: i64,
) -> Result<Worktree, hiqlite::Error> {
    client
        .query_map_one::<Worktree, _>(
            "SELECT id, project_id, task_id, worktree_path, repo_path, branch, created_at FROM worktrees WHERE task_id = $1",
            hiqlite::params!(task_id),
        )
        .await
}

pub async fn delete_worktree(client: &Client, task_id: i64) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM worktrees WHERE task_id = $1",
            hiqlite::params!(task_id),
        )
        .await?;
    Ok(rows > 0)
}

pub async fn create_agent_run_blocked(
    client: &Client,
    task_id: i64,
    agent_type: &AgentType,
) -> Result<TaskAgentRun, hiqlite::Error> {
    let id = Uuid::new_v4();
    let now = utc_now();
    client
        .execute(
            "INSERT INTO task_agent_runs (id, task_id, agent_type, status, created_at) VALUES ($1, $2, $3, $4, $5)",
            hiqlite::params!(id.to_string(), task_id, agent_type.to_string(), RunStatus::Blocked.to_string(), &now),
        )
        .await?;
    get_agent_run(client, &id).await
}

pub async fn list_conversations_for_task(
    client: &Client,
    task_id: i64,
) -> Result<Vec<ConversationWithRun>, hiqlite::Error> {
    let conversations = client
        .query_map::<Conversation, _>(
            "SELECT id, task_id, provider_session_id, model, effort, name, created_at, updated_at FROM conversations WHERE task_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(task_id),
        )
        .await?;
    let mut results = Vec::with_capacity(conversations.len());
    for conv in conversations {
        let run = client
            .query_map_one::<TaskAgentRun, _>(
                "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE conversation_id = $1",
                hiqlite::params!(conv.id.to_string()),
            )
            .await
            .ok();
        results.push(ConversationWithRun {
            conversation: conv,
            run,
        });
    }
    Ok(results)
}

pub async fn create_agent_run(
    client: &Client,
    task_id: i64,
    agent_type: &AgentType,
    conversation_id: &Uuid,
) -> Result<TaskAgentRun, hiqlite::Error> {
    let id = Uuid::new_v4();
    let now = utc_now();
    client
        .execute(
            "INSERT INTO task_agent_runs (id, task_id, agent_type, status, conversation_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                id.to_string(),
                task_id,
                agent_type.to_string(),
                RunStatus::Running.to_string(),
                conversation_id.to_string(),
                &now
            ),
        )
        .await?;
    get_agent_run(client, &id).await
}

pub async fn get_agent_run(client: &Client, run_id: &Uuid) -> Result<TaskAgentRun, hiqlite::Error> {
    client
        .query_map_one::<TaskAgentRun, _>(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE id = $1",
            hiqlite::params!(run_id.to_string()),
        )
        .await
}

pub async fn get_agent_run_by_conversation(
    client: &Client,
    conversation_id: &Uuid,
) -> Result<TaskAgentRun, hiqlite::Error> {
    client
        .query_map_one::<TaskAgentRun, _>(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE conversation_id = $1",
            hiqlite::params!(conversation_id.to_string()),
        )
        .await
}

pub async fn get_agent_run_summary_for_project(
    client: &Client,
    project_id: i64,
) -> Result<HashMap<i64, (Vec<AgentType>, Option<AgentType>)>, hiqlite::Error> {
    let distinct_rows = client
        .query_raw(
            "SELECT DISTINCT task_id, agent_type FROM task_agent_runs WHERE task_id IN (SELECT id FROM tasks WHERE project_id = $1)",
            hiqlite::params!(project_id),
        )
        .await?;

    let mut agent_types: HashMap<i64, Vec<AgentType>> = HashMap::new();
    for mut row in distinct_rows {
        let task_id: i64 = row.get("task_id");
        let agent_type_str: String = row.get("agent_type");
        if let Ok(at) = agent_type_str.parse::<AgentType>() {
            agent_types.entry(task_id).or_default().push(at);
        }
    }

    let running_rows = client
        .query_raw(
            "SELECT task_id, agent_type FROM task_agent_runs WHERE task_id IN (SELECT id FROM tasks WHERE project_id = $1) AND status = 'running'",
            hiqlite::params!(project_id),
        )
        .await?;

    let mut result: HashMap<i64, (Vec<AgentType>, Option<AgentType>)> = agent_types
        .into_iter()
        .map(|(id, types)| (id, (types, None)))
        .collect();
    for mut row in running_rows {
        let task_id: i64 = row.get("task_id");
        let agent_type_str: String = row.get("agent_type");
        if let Ok(at) = agent_type_str.parse::<AgentType>() {
            result
                .entry(task_id)
                .or_insert_with(|| (Vec::new(), None))
                .1 = Some(at);
        }
    }

    Ok(result)
}

pub async fn get_running_agent_for_task(
    client: &Client,
    task_id: i64,
) -> Result<Option<TaskAgentRun>, hiqlite::Error> {
    let mut rows = client
        .query_raw(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE task_id = $1 AND status = 'running' LIMIT 1",
            hiqlite::params!(task_id),
        )
        .await?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(TaskAgentRun::from(&mut rows[0])))
    }
}

pub async fn get_running_agents(
    client: &Client,
    user_id: &Uuid,
) -> Result<Vec<ActiveAgent>, hiqlite::Error> {
    let rows = client
        .query_raw(
            "SELECT
                tar.agent_type,
                t.project_id,
                p.name AS project_title,
                t.id AS task_id,
                t.title AS task_title,
                c.id AS conversation_id,
                c.name AS conversation_name
            FROM task_agent_runs tar
            JOIN tasks t ON t.id = tar.task_id
            JOIN projects p ON p.id = t.project_id
            LEFT JOIN conversations c ON c.id = tar.conversation_id
            WHERE tar.status = 'running' AND t.user_id = $1
            ORDER BY t.project_id, t.id",
            hiqlite::params!(user_id.to_string()),
        )
        .await?;
    let mut agents = Vec::with_capacity(rows.len());
    for mut row in rows {
        agents.push(ActiveAgent::from(&mut row));
    }
    Ok(agents)
}

pub async fn list_agent_runs_for_task(
    client: &Client,
    task_id: i64,
) -> Result<Vec<TaskAgentRun>, hiqlite::Error> {
    client
        .query_map::<TaskAgentRun, _>(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE task_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(task_id),
        )
        .await
}

async fn set_agent_run_status(
    client: &Client,
    run_id: &Uuid,
    status: &RunStatus,
) -> Result<(), hiqlite::Error> {
    let now = utc_now();
    client
        .execute(
            "UPDATE task_agent_runs SET status = $1, completed_at = $2 WHERE id = $3",
            hiqlite::params!(status.to_string(), &now, run_id.to_string()),
        )
        .await?;
    Ok(())
}

pub async fn mark_agent_run_completed(
    client: &Client,
    run_id: &Uuid,
) -> Result<(), hiqlite::Error> {
    set_agent_run_status(client, run_id, &RunStatus::Completed).await
}

pub async fn mark_agent_run_failed(client: &Client, run_id: &Uuid) -> Result<(), hiqlite::Error> {
    set_agent_run_status(client, run_id, &RunStatus::Failed).await
}

pub async fn sweep_running_agent_runs_to_failed(client: &Client) -> Result<usize, hiqlite::Error> {
    let now = utc_now();
    let rows = client
        .execute(
            "UPDATE task_agent_runs SET status = $1, completed_at = $2 WHERE status = $3",
            hiqlite::params!(
                RunStatus::Failed.to_string(),
                &now,
                RunStatus::Running.to_string()
            ),
        )
        .await?;
    Ok(rows)
}

pub async fn increment_workflow_run_count(
    client: &Client,
    task_id: i64,
) -> Result<(), hiqlite::Error> {
    client
        .execute(
            "UPDATE tasks SET workflow_run_count = workflow_run_count + 1 WHERE id = $1",
            hiqlite::params!(task_id),
        )
        .await?;
    Ok(())
}

pub async fn mark_task_blocked(client: &Client, task_id: i64) -> Result<(), hiqlite::Error> {
    client
        .execute(
            "UPDATE tasks SET workflow_blocked = 1 WHERE id = $1",
            hiqlite::params!(task_id),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::services::projects;
    use tempfile::TempDir;

    async fn make_client() -> (Client, TempDir) {
        let db_dir = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: db_dir.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();
        (client, db_dir)
    }

    async fn seed_task(client: &Client) -> (i64, i64) {
        let user_id = db::ensure_default_user(client).await.unwrap();
        let project = projects::create_project(client, &user_id, "test", "/tmp/test", None)
            .await
            .unwrap();
        let project_id = project.id;
        let task = create_task(client, project_id, &user_id, "test task", "pending")
            .await
            .unwrap();
        (project_id, task.id)
    }

    #[tokio::test]
    async fn test_mark_task_blocked() {
        let (client, _tmp) = make_client().await;
        let (_, task_id) = seed_task(&client).await;

        mark_task_blocked(&client, task_id).await.unwrap();

        let task = get_task(&client, task_id).await.unwrap();
        assert!(task.workflow_blocked, "workflow_blocked");
    }

    #[tokio::test]
    async fn test_create_task_sets_created_at() {
        let (client, _tmp) = make_client().await;
        let (_, task_id) = seed_task(&client).await;
        let task = get_task(&client, task_id).await.unwrap();
        let epoch =
            chrono::NaiveDateTime::parse_from_str("1970-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap();
        assert_ne!(
            task.created_at, epoch,
            "created_at should not be the default epoch"
        );
        assert!(task.created_at > epoch, "created_at should be after epoch");
    }
}
