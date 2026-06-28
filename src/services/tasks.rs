use crate::db::schema::{Task, Worktree};
use rusqlite::{Connection, Row};
use uuid::Uuid;

fn uuid_from_row(row: &Row, i: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&row.get::<_, String>(i)?)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn task_from_row(row: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: uuid_from_row(row, 0)?,
        project_id: uuid_from_row(row, 1)?,
        user_id: uuid_from_row(row, 2)?,
        title: row.get(3)?,
        status: row.get(4)?,
        workflow_complete: row.get::<_, i32>(5)? != 0,
        workflow_blocked: row.get::<_, i32>(6)? != 0,
        workflow_run_count: row.get(7)?,
        planification_complete: row.get::<_, i32>(8)? != 0,
        pr_agent_complete: row.get::<_, i32>(9)? != 0,
        refinement_complete: row.get::<_, i32>(10)? != 0,
        yolo_mode: row.get::<_, i32>(11)? != 0,
        created_at: chrono::NaiveDateTime::parse_from_str(
            &row.get::<_, String>(12)?,
            "%Y-%m-%d %H:%M:%S",
        )
        .unwrap_or_default(),
    })
}

fn worktree_from_row(row: &Row) -> rusqlite::Result<Worktree> {
    Ok(Worktree {
        id: uuid_from_row(row, 0)?,
        project_uuid: uuid_from_row(row, 1)?,
        task_uuid: uuid_from_row(row, 2)?,
        project_id: row.get(3)?,
        task_id: row.get(4)?,
        worktree_path: row.get(5)?,
        repo_path: row.get(6)?,
        branch: row.get(7)?,
        created_at: chrono::NaiveDateTime::parse_from_str(
            &row.get::<_, String>(8)?,
            "%Y-%m-%d %H:%M:%S",
        )
        .unwrap_or_default(),
    })
}

pub fn create_task(
    conn: &Connection,
    id: &Uuid,
    project_id: &Uuid,
    user_id: &Uuid,
    title: &str,
    status: &str,
) -> Result<Task, rusqlite::Error> {
    conn.execute(
        "INSERT INTO tasks (id, project_id, user_id, title, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            id.to_string(),
            project_id.to_string(),
            user_id.to_string(),
            title,
            status
        ],
    )?;
    get_task(conn, id)
}

pub fn list_tasks(conn: &Connection, project_id: &Uuid) -> Result<Vec<Task>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, user_id, title, status, workflow_complete, workflow_blocked, workflow_run_count, planification_complete, pr_agent_complete, refinement_complete, yolo_mode, created_at FROM tasks WHERE project_id = ?1 ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([project_id.to_string()], task_from_row)?;
    rows.collect()
}

pub fn get_task(conn: &Connection, task_id: &Uuid) -> Result<Task, rusqlite::Error> {
    conn.query_row(
        "SELECT id, project_id, user_id, title, status, workflow_complete, workflow_blocked, workflow_run_count, planification_complete, pr_agent_complete, refinement_complete, yolo_mode, created_at FROM tasks WHERE id = ?1",
        [task_id.to_string()],
        task_from_row,
    )
}

pub fn update_task(
    conn: &Connection,
    task_id: &Uuid,
    title: Option<&str>,
    status: Option<&str>,
) -> Result<Task, rusqlite::Error> {
    let mut sets = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(t) = title {
        sets.push("title = ?");
        params.push(Box::new(t.to_string()));
    }
    if let Some(s) = status {
        sets.push("status = ?");
        params.push(Box::new(s.to_string()));
    }
    if sets.is_empty() {
        return get_task(conn, task_id);
    }
    params.push(Box::new(task_id.to_string()));
    let sql = format!("UPDATE tasks SET {} WHERE id = ?", sets.join(", "));
    conn.execute(
        &sql,
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
    )?;
    get_task(conn, task_id)
}

pub fn delete_task(conn: &Connection, task_id: &Uuid) -> Result<bool, rusqlite::Error> {
    let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", [task_id.to_string()])?;
    Ok(rows > 0)
}

pub fn insert_worktree(
    conn: &Connection,
    id: &Uuid,
    project_uuid: &Uuid,
    task_uuid: &Uuid,
    project_id: u32,
    task_id: u32,
    worktree_path: &str,
    repo_path: &str,
    branch: &str,
) -> Result<Worktree, rusqlite::Error> {
    conn.execute(
        "INSERT INTO worktrees (id, project_uuid, task_uuid, project_id, task_id, worktree_path, repo_path, branch) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![id.to_string(), project_uuid.to_string(), task_uuid.to_string(), project_id as i64, task_id as i64, worktree_path, repo_path, branch],
    )?;
    get_worktree_by_task(conn, task_uuid)
}

pub fn get_worktree_by_task(
    conn: &Connection,
    task_uuid: &Uuid,
) -> Result<Worktree, rusqlite::Error> {
    conn.query_row(
        "SELECT id, project_uuid, task_uuid, project_id, task_id, worktree_path, repo_path, branch, created_at FROM worktrees WHERE task_uuid = ?1",
        [task_uuid.to_string()],
        worktree_from_row,
    )
}

pub fn delete_worktree(conn: &Connection, task_uuid: &Uuid) -> Result<bool, rusqlite::Error> {
    let rows = conn.execute(
        "DELETE FROM worktrees WHERE task_uuid = ?1",
        [task_uuid.to_string()],
    )?;
    Ok(rows > 0)
}
