use hiqlite::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::archive::ArchiveRoot;

// ── Export types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExportPayload {
    pub exported_at: String,
    pub exported_by: ExportUser,
    pub projects: Vec<ExportProject>,
}

#[derive(Debug, Serialize)]
pub struct ExportUser {
    pub id: Uuid,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct ExportProject {
    pub id: i64,
    pub name: String,
    pub repo_folder_path: String,
    pub subproject_path: Option<String>,
    pub created_at: String,
    pub tasks: Vec<ExportTask>,
}

#[derive(Debug, Serialize)]
pub struct ExportTask {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub status: String,
    pub description: Option<String>,
    pub created_at: String,
    pub conversations: Vec<ExportConversation>,
}

#[derive(Debug, Serialize)]
pub struct ExportConversation {
    pub id: Uuid,
    pub provider_session_id: Option<String>,
    pub model: String,
    pub effort: String,
    pub name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ── Import types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImportPayload {
    pub exported_at: Option<String>,
    pub exported_by: Option<serde_json::Value>,
    pub projects: Vec<ImportSourceProject>,
}

#[derive(Debug, Deserialize)]
pub struct ImportSourceProject {
    pub id: serde_json::Value,
    pub name: String,
    pub repo_folder_path: Option<String>,
    pub tasks: Vec<ImportSourceTask>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSourceTask {
    pub id: serde_json::Value,
    pub title: String,
    pub status: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub conversations: Option<Vec<ImportSourceConversation>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSourceConversation {
    pub id: serde_json::Value,
    pub provider_session_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub name: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub projects: Vec<ImportProjectPreview>,
}

#[derive(Debug, Serialize)]
pub struct ImportProjectPreview {
    pub source_project_id: String,
    pub name: String,
    pub repo_folder_path: Option<String>,
    pub task_count: usize,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportExecuteRequest {
    pub raw_json: String,
    pub imports: Vec<ImportItem>,
}

#[derive(Debug, Deserialize)]
pub struct ImportItem {
    pub source_project_id: String,
    pub target_type: String,
    pub target_project_id: Option<i64>,
    pub name: Option<String>,
    pub repo_folder_path: Option<String>,
}

// ── Export logic ────────────────────────────────────────────────────────────

pub async fn build_export(
    client: &Client,
    archive_root: &str,
    user_id: Uuid,
    username: &str,
    project_ids: &[i64],
) -> Result<ExportPayload, String> {
    let archive = ArchiveRoot::new(std::path::PathBuf::from(archive_root));
    let mut projects = Vec::new();

    for &project_id in project_ids {
        let project = client
            .query_map_one::<crate::db::schema::Project, _>(
                "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at \
                 FROM projects WHERE id = $1 AND user_id = $2",
                hiqlite::params!(project_id, user_id.to_string()),
            )
            .await
            .map_err(|e| format!("project {project_id} not found: {e}"))?;

        let db_tasks: Vec<crate::db::schema::Task> = client
            .query_map::<crate::db::schema::Task, _>(
                "SELECT id, project_id, user_id, title, status, workflow_complete, \
                 workflow_blocked, workflow_run_count, planification_complete, \
                 pr_agent_complete, refinement_complete, yolo_mode, created_at \
                 FROM tasks WHERE project_id = $1 ORDER BY created_at",
                hiqlite::params!(project_id),
            )
            .await
            .map_err(|e| format!("failed to fetch tasks: {e}"))?;

        let mut export_tasks = Vec::with_capacity(db_tasks.len());

        for task in db_tasks {
            let doc_path = archive.task_doc_path(&project_id.to_string(), &task.id.to_string());
            let description = archive
                .read_task_doc(&doc_path)
                .map_err(|e| format!("failed to read task doc for task {}: {e}", task.id))?;
            let description = (!description.is_empty()).then_some(description);

            let conversations = client
                .query_map::<crate::db::schema::Conversation, _>(
                    "SELECT id, task_id, provider_session_id, model, effort, name, \
                     created_at, updated_at FROM conversations WHERE task_id = $1 \
                     ORDER BY created_at",
                    hiqlite::params!(task.id),
                )
                .await
                .map_err(|e| format!("failed to fetch conversations: {e}"))?;

            let export_convs: Vec<ExportConversation> = conversations
                .into_iter()
                .map(|c| ExportConversation {
                    id: c.id,
                    provider_session_id: c.provider_session_id,
                    model: c.model,
                    effort: c.effort,
                    name: c.name,
                    created_at: c.created_at.to_string(),
                    updated_at: c.updated_at.to_string(),
                })
                .collect();

            export_tasks.push(ExportTask {
                id: task.id,
                project_id: task.project_id,
                title: task.title,
                status: task.status,
                description,
                created_at: task.created_at.to_string(),
                conversations: export_convs,
            });
        }

        projects.push(ExportProject {
            id: project.id,
            name: project.name,
            repo_folder_path: project.repo_folder_path,
            subproject_path: project.subproject_path,
            created_at: project.created_at.to_string(),
            tasks: export_tasks,
        });
    }

    Ok(ExportPayload {
        exported_at: chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        exported_by: ExportUser {
            id: user_id,
            username: username.to_string(),
        },
        projects,
    })
}

// ── Import preview logic ────────────────────────────────────────────────────

pub fn preview_import(json_str: &str) -> Result<ImportPreview, String> {
    let payload: ImportPayload =
        serde_json::from_str(json_str).map_err(|e| format!("invalid JSON: {e}"))?;

    if payload.projects.is_empty() {
        return Err("JSON must contain at least one project".to_string());
    }

    let projects: Vec<ImportProjectPreview> = payload
        .projects
        .into_iter()
        .map(|sp| {
            let (status, error) = if sp.name.trim().is_empty() {
                (
                    "error".to_string(),
                    Some("project name is empty".to_string()),
                )
            } else {
                ("valid".to_string(), None)
            };
            ImportProjectPreview {
                source_project_id: sp.id.to_string(),
                name: sp.name,
                repo_folder_path: sp.repo_folder_path,
                task_count: sp.tasks.len(),
                status,
                error,
            }
        })
        .collect();

    Ok(ImportPreview { projects })
}

// ── Import execute logic ────────────────────────────────────────────────────

fn matches_source_id(id: &serde_json::Value, target: &str) -> bool {
    match id {
        serde_json::Value::String(s) => s == target,
        serde_json::Value::Number(n) => n.to_string() == target,
        _ => false,
    }
}

pub async fn execute_import(
    client: &Client,
    archive_root: &str,
    user_id: Uuid,
    request: ImportExecuteRequest,
) -> Result<(), String> {
    let payload: ImportPayload =
        serde_json::from_str(&request.raw_json).map_err(|e| format!("invalid JSON: {e}"))?;

    let archive = ArchiveRoot::new(std::path::PathBuf::from(archive_root));

    for item in &request.imports {
        let source_proj = payload
            .projects
            .iter()
            .find(|p| matches_source_id(&p.id, &item.source_project_id))
            .ok_or_else(|| {
                format!(
                    "source project {} not found in import data",
                    item.source_project_id
                )
            })?;

        match item.target_type.as_str() {
            "create_new" => {
                let name = item
                    .name
                    .as_deref()
                    .unwrap_or(&source_proj.name)
                    .to_string();
                let repo_path = item
                    .repo_folder_path
                    .as_deref()
                    .unwrap_or(source_proj.repo_folder_path.as_deref().unwrap_or(""))
                    .to_string();

                let project_id =
                    create_project_for_import(client, &user_id, &name, &repo_path).await?;

                for source_task in &source_proj.tasks {
                    import_task(client, &archive, project_id, &user_id, source_task).await?;
                }
            }
            "add_to_existing" => {
                let target_id = item.target_project_id.ok_or_else(|| {
                    "target_project_id is required for add_to_existing".to_string()
                })?;

                // Verify project exists and belongs to user
                client
                    .query_map_one::<crate::db::schema::Project, _>(
                        "SELECT id, user_id, name, repo_folder_path, subproject_path, created_at \
                         FROM projects WHERE id = $1 AND user_id = $2",
                        hiqlite::params!(target_id, user_id.to_string()),
                    )
                    .await
                    .map_err(|e| format!("target project {target_id} not found: {e}"))?;

                for source_task in &source_proj.tasks {
                    import_task(client, &archive, target_id, &user_id, source_task).await?;
                }
            }
            other => return Err(format!("unknown target_type: {other}")),
        }
    }

    Ok(())
}

async fn create_project_for_import(
    client: &Client,
    user_id: &Uuid,
    name: &str,
    repo_folder_path: &str,
) -> Result<i64, String> {
    let id: i64 = {
        let mut rows = client
            .query_raw(
                "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM projects",
                hiqlite::params!(),
            )
            .await
            .map_err(|e| format!("failed to get next project id: {e}"))?;
        rows.first_mut()
            .map(|r| r.get::<i64>("next_id"))
            .unwrap_or(1)
    };
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
            hiqlite::params!(id, user_id.to_string(), name, repo_folder_path, &now),
        )
        .await
        .map_err(|e| format!("failed to create project: {e}"))?;
    Ok(id)
}

async fn import_task(
    client: &Client,
    archive: &ArchiveRoot,
    project_id: i64,
    user_id: &Uuid,
    source_task: &ImportSourceTask,
) -> Result<(), String> {
    let id: i64 = {
        let mut rows = client
            .query_raw(
                "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM tasks",
                hiqlite::params!(),
            )
            .await
            .map_err(|e| format!("failed to get next task id: {e}"))?;
        rows.first_mut()
            .map(|r| r.get::<i64>("next_id"))
            .unwrap_or(1)
    };

    let status = source_task.status.as_deref().unwrap_or("pending");
    let now_default = || chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let created_at = source_task.created_at.clone().unwrap_or_else(now_default);

    client
        .execute(
            "INSERT INTO tasks (id, project_id, user_id, title, status, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                id,
                project_id,
                user_id.to_string(),
                &source_task.title,
                status,
                &created_at
            ),
        )
        .await
        .map_err(|e| format!("failed to create task: {e}"))?;

    // Write task doc if description is provided
    if let Some(ref desc) = source_task.description {
        if !desc.trim().is_empty() {
            let doc_path = archive.task_doc_path(&project_id.to_string(), &id.to_string());
            if let Some(parent) = doc_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create task doc dir: {e}"))?;
            }
            std::fs::write(&doc_path, desc)
                .map_err(|e| format!("failed to write task doc: {e}"))?;
        }
    }

    // Import conversations
    if let Some(ref convs) = source_task.conversations {
        for conv in convs {
            let conv_id = Uuid::new_v4();
            let model = conv.model.as_deref().unwrap_or("unknown");
            let effort = conv.effort.as_deref().unwrap_or("auto");
            let conv_created = conv
                .created_at
                .clone()
                .unwrap_or_else(|| created_at.clone());
            let conv_updated = conv
                .updated_at
                .clone()
                .unwrap_or_else(|| conv_created.clone());

            client
                .execute(
                    "INSERT INTO conversations (id, task_id, provider_session_id, model, \
                             effort, name, created_at, updated_at) \
                             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    hiqlite::params!(
                        conv_id.to_string(),
                        id,
                        conv.provider_session_id.clone(),
                        model,
                        effort,
                        conv.name.clone(),
                        &conv_created,
                        &conv_updated
                    ),
                )
                .await
                .map_err(|e| format!("failed to create conversation: {e}"))?;
        }
    }

    Ok(())
}
