use rusqlite::{Connection, Row};
use uuid::Uuid;
use crate::db::schema::Project;

fn uuid_from_row(row: &Row, i: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&row.get::<_, String>(i)?)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

fn project_from_row(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: uuid_from_row(row, 0)?,
        user_id: uuid_from_row(row, 1)?,
        name: row.get(2)?,
        repo_folder_path: row.get(3)?,
        subproject_path: row.get(4)?,
        created_at: chrono::NaiveDateTime::parse_from_str(
            &row.get::<_, String>(5)?,
            "%Y-%m-%d %H:%M:%S",
        ).unwrap_or_default(),
    })
}

pub fn create_project(
    conn: &Connection,
    user_id: &Uuid,
    name: &str,
    repo_folder_path: &str,
    subproject_path: Option<&str>,
) -> Result<Project, rusqlite::Error> {
    let id = Uuid::new_v4();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path, subproject_path) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id.to_string(), user_id.to_string(), name, repo_folder_path, subproject_path],
    )?;
    get_project(conn, &id)
}

pub fn list_projects(conn: &Connection, user_id: &Uuid) -> Result<Vec<Project>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at FROM projects WHERE user_id = ?1 ORDER BY created_at DESC"
    )?;
    let rows = stmt.query_map([user_id.to_string()], project_from_row)?;
    rows.collect()
}

pub fn get_project(conn: &Connection, project_id: &Uuid) -> Result<Project, rusqlite::Error> {
    conn.query_row(
        "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at FROM projects WHERE id = ?1",
        [project_id.to_string()],
        project_from_row,
    )
}

pub fn update_project(
    conn: &Connection,
    project_id: &Uuid,
    name: Option<&str>,
    repo_folder_path: Option<&str>,
    subproject_path: Option<&str>,
) -> Result<Project, rusqlite::Error> {
    let mut sets = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(n) = name {
        sets.push("name = ?");
        params.push(Box::new(n.to_string()));
    }
    if let Some(r) = repo_folder_path {
        sets.push("repo_folder_path = ?");
        params.push(Box::new(r.to_string()));
    }
    if subproject_path.is_some() {
        sets.push("subproject_path = ?");
        params.push(Box::new(subproject_path.map(|s| s.to_string())));
    }
    if sets.is_empty() {
        return get_project(conn, project_id);
    }
    params.push(Box::new(project_id.to_string()));
    let sql = format!("UPDATE projects SET {} WHERE id = ?", sets.join(", "));
    conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))?;
    get_project(conn, project_id)
}

pub fn delete_project(conn: &Connection, project_id: &Uuid) -> Result<bool, rusqlite::Error> {
    let rows = conn.execute("DELETE FROM projects WHERE id = ?1", [project_id.to_string()])?;
    Ok(rows > 0)
}
