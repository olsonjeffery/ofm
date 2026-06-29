use crate::db::schema::{Task, Worktree};
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
