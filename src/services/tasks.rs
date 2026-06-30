use crate::db::schema::{AgentType, RunStatus, Task, TaskAgentRun, Worktree};
use hiqlite::Client;
use uuid::Uuid;

pub async fn create_task(
    client: &Client,
    id: &Uuid,
    project_id: &Uuid,
    user_id: &Uuid,
    title: &str,
    status: &str,
) -> Result<Task, hiqlite::Error> {
    client
        .execute(
            "INSERT INTO tasks (id, project_id, user_id, title, status) VALUES ($1, $2, $3, $4, $5)",
            hiqlite::params!(
                id.to_string(),
                project_id.to_string(),
                user_id.to_string(),
                title,
                status
            ),
        )
        .await?;
    get_task(client, id).await
}

pub async fn list_tasks(client: &Client, project_id: &Uuid) -> Result<Vec<Task>, hiqlite::Error> {
    client
        .query_map::<Task, _>(
            "SELECT id, project_id, user_id, title, status, workflow_complete, workflow_blocked, workflow_run_count, planification_complete, pr_agent_complete, refinement_complete, yolo_mode, created_at FROM tasks WHERE project_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(project_id.to_string()),
        )
        .await
}

pub async fn get_task(client: &Client, task_id: &Uuid) -> Result<Task, hiqlite::Error> {
    client
        .query_map_one::<Task, _>(
            "SELECT id, project_id, user_id, title, status, workflow_complete, workflow_blocked, workflow_run_count, planification_complete, pr_agent_complete, refinement_complete, yolo_mode, created_at FROM tasks WHERE id = $1",
            hiqlite::params!(task_id.to_string()),
        )
        .await
}

pub async fn update_task(
    client: &Client,
    task_id: &Uuid,
    title: Option<&str>,
    status: Option<&str>,
) -> Result<Task, hiqlite::Error> {
    if title.is_none() && status.is_none() {
        return get_task(client, task_id).await;
    }
    client
        .execute(
            "UPDATE tasks SET title = COALESCE($1, title), status = COALESCE($2, status) WHERE id = $3",
            hiqlite::params!(title, status, task_id.to_string()),
        )
        .await?;
    get_task(client, task_id).await
}

pub async fn delete_task(client: &Client, task_id: &Uuid) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM tasks WHERE id = $1",
            hiqlite::params!(task_id.to_string()),
        )
        .await?;
    Ok(rows > 0)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_worktree(
    client: &Client,
    id: &Uuid,
    project_uuid: &Uuid,
    task_uuid: &Uuid,
    project_id: u32,
    task_id: u32,
    worktree_path: &str,
    repo_path: &str,
    branch: &str,
) -> Result<Worktree, hiqlite::Error> {
    client
        .execute(
            "INSERT INTO worktrees (id, project_uuid, task_uuid, project_id, task_id, worktree_path, repo_path, branch) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            hiqlite::params!(
                id.to_string(),
                project_uuid.to_string(),
                task_uuid.to_string(),
                project_id as i64,
                task_id as i64,
                worktree_path,
                repo_path,
                branch
            ),
        )
        .await?;
    get_worktree_by_task(client, task_uuid).await
}

pub async fn get_worktree_by_task(
    client: &Client,
    task_uuid: &Uuid,
) -> Result<Worktree, hiqlite::Error> {
    client
        .query_map_one::<Worktree, _>(
            "SELECT id, project_uuid, task_uuid, project_id, task_id, worktree_path, repo_path, branch, created_at FROM worktrees WHERE task_uuid = $1",
            hiqlite::params!(task_uuid.to_string()),
        )
        .await
}

pub async fn delete_worktree(client: &Client, task_uuid: &Uuid) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM worktrees WHERE task_uuid = $1",
            hiqlite::params!(task_uuid.to_string()),
        )
        .await?;
    Ok(rows > 0)
}

pub async fn create_agent_run(
    client: &Client,
    task_id: &Uuid,
    agent_type: &AgentType,
    conversation_id: &Uuid,
) -> Result<TaskAgentRun, hiqlite::Error> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "INSERT INTO task_agent_runs (id, task_id, agent_type, status, conversation_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                id.to_string(),
                task_id.to_string(),
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

pub async fn get_running_agent_for_task(
    client: &Client,
    task_id: &Uuid,
) -> Result<Option<TaskAgentRun>, hiqlite::Error> {
    let mut rows = client
        .query_raw(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE task_id = $1 AND status = 'running' LIMIT 1",
            hiqlite::params!(task_id.to_string()),
        )
        .await?;
    if rows.is_empty() {
        Ok(None)
    } else {
        Ok(Some(TaskAgentRun::from(&mut rows[0])))
    }
}

pub async fn list_agent_runs_for_task(
    client: &Client,
    task_id: &Uuid,
) -> Result<Vec<TaskAgentRun>, hiqlite::Error> {
    client
        .query_map::<TaskAgentRun, _>(
            "SELECT id, task_id, agent_type, status, conversation_id, created_at, completed_at FROM task_agent_runs WHERE task_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(task_id.to_string()),
        )
        .await
}

pub async fn mark_agent_run_completed(
    client: &Client,
    run_id: &Uuid,
) -> Result<(), hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "UPDATE task_agent_runs SET status = $1, completed_at = $2 WHERE id = $3",
            hiqlite::params!(RunStatus::Completed.to_string(), &now, run_id.to_string()),
        )
        .await?;
    Ok(())
}

pub async fn mark_agent_run_failed(client: &Client, run_id: &Uuid) -> Result<(), hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "UPDATE task_agent_runs SET status = $1, completed_at = $2 WHERE id = $3",
            hiqlite::params!(RunStatus::Failed.to_string(), &now, run_id.to_string()),
        )
        .await?;
    Ok(())
}

pub async fn sweep_running_agent_runs_to_failed(client: &Client) -> Result<usize, hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
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
    task_id: &Uuid,
) -> Result<(), hiqlite::Error> {
    client
        .execute(
            "UPDATE tasks SET workflow_run_count = workflow_run_count + 1 WHERE id = $1",
            hiqlite::params!(task_id.to_string()),
        )
        .await?;
    Ok(())
}

pub async fn mark_task_blocked(client: &Client, task_id: &Uuid) -> Result<(), hiqlite::Error> {
    client
        .execute(
            "UPDATE tasks SET workflow_blocked = 1 WHERE id = $1",
            hiqlite::params!(task_id.to_string()),
        )
        .await?;
    Ok(())
}
