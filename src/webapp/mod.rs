pub mod app;
pub mod auth;
pub mod auth_pages;
pub mod components;
pub mod islands;
pub mod pages;
pub mod shim;
pub mod styles;

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use axum_extra::extract::cookie::PrivateCookieJar;
use leptos::prelude::*;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::providers::registry;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;
use crate::webapp::components::project_card::TaskCounts;

pub fn webapp_routes() -> Router<AppState> {
    Router::new()
        .route("/webapp/login", get(login_handler))
        .route("/webapp/callback", get(callback_handler))
}

pub fn webapp_protected_routes() -> Router<AppState> {
    Router::new()
        .route("/webapp", get(dashboard_handler))
        .route("/webapp/projects/{id}", get(board_handler))
        .route(
            "/webapp/projects/{project_id}/tasks/{task_id}",
            get(task_detail_handler),
        )
        .route("/webapp/onboarding", get(onboarding_handler))
        .route("/webapp/settings", get(settings_handler))
        .route("/webapp/islands/uptime", get(uptime_handler))
        .route("/webapp/islands/infocard", get(infocard_handler))
}

fn render_shell(page_html: &str, user_json: Option<String>) -> String {
    let shell = leptos::view! { <app::ShellPage user_json /> }.to_html();
    shell.replace("<main style=\"width: 95%; margin: 0 auto; min-height: calc(100vh - 3.25rem);\"></main>", &format!("<main style=\"width: 95%; margin: 0 auto; min-height: calc(100vh - 3.25rem);\">{page_html}</main>"))
}

async fn login_handler() -> Html<String> {
    let login_html = pages::login::render_login_page();
    Html(render_shell(&login_html, None))
}

async fn callback_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(_params): Query<HashMap<String, String>>,
) -> Html<String> {
    let go = |path| {
        Html(render_shell(
            &format!("<script>window.location.href='{path}';</script>"),
            None,
        ))
    };

    let session_id = match crate::server::routes::auth::parse_session_cookie(&jar) {
        Ok(id) => id,
        Err(_) => return go("/webapp/login"),
    };

    let user_id = match resolve_user_id_from_session(&state.db, session_id).await {
        Some(id) => id,
        None => return go("/webapp/login"),
    };

    let user = match crate::services::auth::current_user(&state.db, user_id).await {
        Ok(u) => u,
        Err(_) => return go("/webapp/login"),
    };

    if user.has_completed_onboarding {
        go("/webapp/")
    } else {
        go("/webapp/onboarding")
    }
}

async fn onboarding_handler(State(state): State<AppState>, auth: AuthUser) -> Html<String> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();

    let user = match crate::services::auth::current_user(&state.db, auth.user_id).await {
        Ok(u) => u,
        Err(_) => {
            return Html(render_shell(
                r#"<script>window.location.href='/webapp/login';</script>"#,
                None,
            ))
        }
    };

    let git_name = user.git_name.unwrap_or_default();
    let git_email = user.git_email.unwrap_or_default();
    let is_technical = user.is_technical;

    let form_html = pages::onboarding::render_onboarding_form(git_name, git_email, is_technical);
    Html(render_shell(&form_html, Some(user_json)))
}

async fn resolve_user_id_from_session(db: &hiqlite::Client, session_id: Uuid) -> Option<Uuid> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM sessions WHERE id = $1",
            hiqlite::params!(session_id.to_string()),
        )
        .await
        .ok()?;
    let session = rows.first_mut()?;
    let session_db = crate::db::schema::SessionDb::from(&mut *session);

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if session_db.expires_at < now {
        return None;
    }

    Some(session_db.user_id)
}

async fn settings_handler(auth: AuthUser) -> Html<String> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let settings_html =
        leptos::view! { <pages::settings::SettingsPage access_token=String::new() /> }.to_html();
    Html(render_shell(&settings_html, Some(user_json)))
}

async fn dashboard_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let projects = services::projects::list_projects(&state.db, &auth.user_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let task_counts = compute_task_counts(&state.db, &projects).await;
    let page_html =
        leptos::view! { <pages::dashboard::DashboardPage projects task_counts /> }.to_html();
    Ok(Html(render_shell(&page_html, Some(user_json))))
}

async fn board_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    let tasks = services::tasks::list_tasks(&state.db, project_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let page_html = leptos::view! { <pages::board::BoardPage project tasks /> }.to_html();
    Ok(Html(render_shell(&page_html, Some(user_json))))
}

async fn task_detail_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((project_id, task_id)): Path<(i64, i64)>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();

    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }

    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let worktree = services::tasks::get_worktree_by_task(&state.db, task_id)
        .await
        .ok();

    let doc_content = worktree.and_then(|w| {
        let archive =
            crate::archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
        let proj_str = w.project_id.to_string();
        let task_str = w.task_id.to_string();
        let doc_path = archive.task_doc_path(&proj_str, &task_str);
        archive.read_task_doc(&doc_path).ok()
    });

    let agent_runs = services::tasks::list_agent_runs_for_task(&state.db, task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let agent_config_statuses =
        registry::resolve_agent_config_statuses(&state.db, auth.user_id, project_id).await;

    let page_html = leptos::view! {
        <pages::task_detail::TaskDetailPage
            task
            doc_content
            agent_runs
            agent_config_statuses=agent_config_statuses
        />
    }
    .to_html();
    Ok(Html(render_shell(&page_html, Some(user_json))))
}

async fn compute_task_counts(
    db: &hiqlite::Client,
    projects: &[crate::db::schema::Project],
) -> HashMap<i64, TaskCounts> {
    let mut counts = HashMap::new();
    for project in projects {
        let tasks = services::tasks::list_tasks(db, project.id)
            .await
            .unwrap_or_default();
        let mut tc = TaskCounts::default();
        for task in &tasks {
            match task.status.as_str() {
                "pending" => tc.pending += 1,
                "in_progress" => tc.in_progress += 1,
                "in_review" => tc.in_review += 1,
                "completed" => tc.completed += 1,
                _ => {}
            }
        }
        counts.insert(project.id, tc);
    }
    counts
}

async fn uptime_handler() -> Html<String> {
    Html(islands::uptime::render_uptime())
}

async fn infocard_handler(Query(params): Query<HashMap<String, String>>) -> Html<String> {
    let title = params.get("title").map(String::as_str).unwrap_or("Info");
    let body = params
        .get("body")
        .map(String::as_str)
        .unwrap_or("No content.");
    Html(islands::infocard::render_infocard(title, body))
}
