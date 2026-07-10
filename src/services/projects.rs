use crate::db::schema::Project;
use hiqlite::Client;
use uuid::Uuid;

pub async fn create_project(
    client: &Client,
    user_id: &Uuid,
    name: &str,
    repo_folder_path: &str,
    subproject_path: Option<&str>,
) -> Result<Project, hiqlite::Error> {
    let id: i64 = {
        let mut rows = client
            .query_raw(
                "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM projects",
                hiqlite::params!(),
            )
            .await?;
        rows.first_mut()
            .map(|r| r.get::<i64>("next_id"))
            .unwrap_or(1)
    };
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path, subproject_path) VALUES ($1, $2, $3, $4, $5)",
            hiqlite::params!(id, user_id.to_string(), name, repo_folder_path, subproject_path),
        )
        .await?;
    get_project(client, id).await
}

pub async fn list_projects(
    client: &Client,
    user_id: &Uuid,
) -> Result<Vec<Project>, hiqlite::Error> {
    client
        .query_map::<Project, _>(
            "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at FROM projects WHERE user_id = $1 ORDER BY created_at DESC",
            hiqlite::params!(user_id.to_string()),
        )
        .await
}

pub async fn get_project(client: &Client, project_id: i64) -> Result<Project, hiqlite::Error> {
    client
        .query_map_one::<Project, _>(
            "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at FROM projects WHERE id = $1",
            hiqlite::params!(project_id),
        )
        .await
}

pub async fn update_project(
    client: &Client,
    project_id: i64,
    name: Option<&str>,
    repo_folder_path: Option<&str>,
    subproject_path: Option<&str>,
) -> Result<Project, hiqlite::Error> {
    if name.is_none() && repo_folder_path.is_none() && subproject_path.is_none() {
        return get_project(client, project_id).await;
    }
    client
        .execute(
            "UPDATE projects SET name = COALESCE($1, name), repo_folder_path = COALESCE($2, repo_folder_path), subproject_path = COALESCE($3, subproject_path) WHERE id = $4",
            hiqlite::params!(name, repo_folder_path, subproject_path, project_id),
        )
        .await?;
    get_project(client, project_id).await
}

pub async fn delete_project(client: &Client, project_id: i64) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM projects WHERE id = $1",
            hiqlite::params!(project_id),
        )
        .await?;
    Ok(rows > 0)
}
