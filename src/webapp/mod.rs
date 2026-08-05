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
use crate::db::schema::ActiveAgent;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;
use crate::services::{session, transcript};
use crate::webapp::components::breadcrumb::{breadcrumb_registry, BreadcrumbItem};
use crate::webapp::components::project_card::TaskCounts;
use crate::webapp::components::settings_sidebar::{SettingsSection, SettingsSubPage};
use crate::webapp::components::task_card::TaskCardData;

async fn active_agents(db: &hiqlite::Client, user_id: &Uuid) -> Vec<ActiveAgent> {
    services::tasks::get_running_agents(db, user_id)
        .await
        .unwrap_or_default()
}

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
        .route(
            "/webapp/projects/{project_id}/tasks/{task_id}/chat",
            get(chat_handler),
        )
        .route(
            "/webapp/projects/{project_id}/tasks/{task_id}/chat/{conversation_id}",
            get(chat_handler_with_conv),
        )
        .route(
            "/webapp/projects/{project_id}/tasks/{task_id}/commits/{oid}",
            get(commit_detail_handler),
        )
        .route("/webapp/onboarding", get(onboarding_handler))
        .route("/webapp/settings", get(settings_handler))
        .route(
            "/webapp/settings/providers-agents",
            get(settings_providers_handler),
        )
        .route(
            "/webapp/settings/providers-agents/model-config",
            get(settings_providers_handler),
        )
        .route(
            "/webapp/settings/providers-agents/agent-settings",
            get(settings_agent_settings_handler),
        )
        .route(
            "/webapp/settings/import-export",
            get(settings_import_export_handler),
        )
        .route(
            "/webapp/settings/import-export/export",
            get(settings_import_export_handler),
        )
        .route(
            "/webapp/settings/import-export/import",
            get(settings_import_handler),
        )
        .route("/webapp/settings/account", get(settings_account_handler))
        .route(
            "/webapp/settings/account/user-config",
            get(settings_account_handler),
        )
        .route(
            "/webapp/settings/account/api-keys",
            get(settings_api_keys_handler),
        )
        .route("/webapp/islands/uptime", get(uptime_handler))
        .route("/webapp/islands/infocard", get(infocard_handler))
}

fn render_shell(
    page_html: &str,
    user_json: Option<String>,
    breadcrumbs: Vec<BreadcrumbItem>,
    active_agents: Vec<ActiveAgent>,
) -> String {
    let shell = leptos::view! { <app::ShellPage user_json breadcrumbs active_agents /> }.to_html();
    if shell.contains("<main></main>") {
        shell.replace("<main></main>", &format!("<main>{}</main>", page_html))
    } else {
        tracing::error!("render_shell: <main></main> marker not found in shell HTML");
        shell
    }
}

async fn login_handler() -> Html<String> {
    let login_html = pages::login::render_login_page();
    Html(render_shell(&login_html, None, Vec::new(), Vec::new()))
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
            Vec::new(),
            Vec::new(),
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
    let agents = active_agents(&state.db, &auth.user_id).await;

    let user = match crate::services::auth::current_user(&state.db, auth.user_id).await {
        Ok(u) => u,
        Err(_) => {
            return Html(render_shell(
                r#"<script>window.location.href='/webapp/login';</script>"#,
                None,
                Vec::new(),
                Vec::new(),
            ))
        }
    };

    let form_html = pages::onboarding::render_onboarding_form(
        user.git_name.unwrap_or_default(),
        user.git_email.unwrap_or_default(),
        user.is_technical,
    );
    Html(render_shell(
        &form_html,
        Some(user_json),
        Vec::new(),
        agents,
    ))
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

async fn render_settings(
    state: &AppState,
    auth: &AuthUser,
    section: SettingsSection,
    sub_page: SettingsSubPage,
    body: String,
) -> Html<String> {
    let user_json = serde_json::to_string(auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;
    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::settings(),
        breadcrumb_registry::settings_section(section),
        breadcrumb_registry::settings_sub_page(sub_page),
    ];
    Html(render_shell(&body, Some(user_json), breadcrumbs, agents))
}

async fn settings_handler(auth: AuthUser, State(state): State<AppState>) -> Html<String> {
    render_settings(
        &state,
        &auth,
        SettingsSection::ProvidersAgents,
        SettingsSubPage::ModelConfig,
        pages::settings::providers_agents::render(SettingsSubPage::ModelConfig),
    )
    .await
}

async fn settings_providers_handler(auth: AuthUser, State(state): State<AppState>) -> Html<String> {
    render_settings(
        &state,
        &auth,
        SettingsSection::ProvidersAgents,
        SettingsSubPage::ModelConfig,
        pages::settings::providers_agents::render(SettingsSubPage::ModelConfig),
    )
    .await
}

async fn settings_agent_settings_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Html<String> {
    render_settings(
        &state,
        &auth,
        SettingsSection::ProvidersAgents,
        SettingsSubPage::AgentSettings,
        pages::settings::providers_agents::render(SettingsSubPage::AgentSettings),
    )
    .await
}

async fn settings_import_export_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Html<String> {
    render_settings(
        &state,
        &auth,
        SettingsSection::ImportExport,
        SettingsSubPage::Export,
        pages::settings::import_export::render(SettingsSubPage::Export),
    )
    .await
}

async fn settings_import_handler(auth: AuthUser, State(state): State<AppState>) -> Html<String> {
    render_settings(
        &state,
        &auth,
        SettingsSection::ImportExport,
        SettingsSubPage::Import,
        pages::settings::import_export::render(SettingsSubPage::Import),
    )
    .await
}

async fn settings_account_handler(auth: AuthUser, State(state): State<AppState>) -> Html<String> {
    let user = match crate::services::auth::current_user(&state.db, auth.user_id).await {
        Ok(u) => u,
        Err(_) => {
            return Html(render_shell(
                r#"<script>window.location.href='/webapp/login';</script>"#,
                None,
                Vec::new(),
                Vec::new(),
            ))
        }
    };
    render_settings(
        &state,
        &auth,
        SettingsSection::Account,
        SettingsSubPage::UserConfig,
        pages::settings::account::render(
            SettingsSubPage::UserConfig,
            user.git_name.unwrap_or_default(),
            user.git_email.unwrap_or_default(),
            user.is_technical,
        ),
    )
    .await
}

async fn settings_api_keys_handler(auth: AuthUser, State(state): State<AppState>) -> Html<String> {
    let user = match crate::services::auth::current_user(&state.db, auth.user_id).await {
        Ok(u) => u,
        Err(_) => {
            return Html(render_shell(
                r#"<script>window.location.href='/webapp/login';</script>"#,
                None,
                Vec::new(),
                Vec::new(),
            ))
        }
    };
    render_settings(
        &state,
        &auth,
        SettingsSection::Account,
        SettingsSubPage::ApiKeys,
        pages::settings::account::render(
            SettingsSubPage::ApiKeys,
            user.git_name.unwrap_or_default(),
            user.git_email.unwrap_or_default(),
            user.is_technical,
        ),
    )
    .await
}

async fn dashboard_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;
    let projects = services::projects::list_projects(&state.db, &auth.user_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let task_counts = compute_task_counts(&state.db, &projects).await;
    let page_html =
        leptos::view! { <pages::dashboard::DashboardPage projects task_counts /> }.to_html();
    let breadcrumbs = vec![breadcrumb_registry::all_projects()];
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
}

async fn board_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;
    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    let tasks = services::tasks::list_tasks(&state.db, project_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let run_summary = services::tasks::get_agent_run_summary_for_project(&state.db, project_id)
        .await
        .unwrap_or_default();

    let task_data: Vec<TaskCardData> = tasks
        .into_iter()
        .map(|t| {
            let (agent_types_run, running_agent) =
                run_summary.get(&t.id).cloned().unwrap_or_default();
            TaskCardData {
                task: t,
                agent_types_run,
                running_agent,
            }
        })
        .collect();

    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::project(&project.name, project.id),
    ];
    let page_html = leptos::view! { <pages::board::BoardPage project tasks=task_data /> }.to_html();
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
}

async fn task_detail_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((project_id, task_id)): Path<(i64, i64)>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;

    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }

    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id || task.project_id != project_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let worktree = services::tasks::get_worktree_by_task(&state.db, task_id)
        .await
        .ok();

    let worktree_missing = worktree
        .as_ref()
        .map(|w| !std::path::Path::new(&w.worktree_path).exists())
        .unwrap_or(false);

    let worktree_path = worktree.as_ref().map(|w| w.worktree_path.clone());

    let doc_content = worktree.and_then(|w| {
        let archive =
            crate::archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
        let proj_str = w.project_id.to_string();
        let task_str = w.task_id.to_string();
        let doc_path = archive.task_doc_path(&proj_str, &task_str);
        archive.read_task_doc(&doc_path).ok()
    });

    let commits = match worktree_path {
        Some(path) => tokio::task::spawn_blocking(move || {
            services::commits::list_commits_for_worktree(std::path::Path::new(&path))
        })
        .await
        .unwrap_or_else(|_| Err(services::commits::Error::Other("task cancelled".into())))
        .unwrap_or_default(),
        None => Vec::new(),
    };

    let conversations = services::tasks::list_conversations_for_task(&state.db, task_id)
        .await
        .unwrap_or_default();

    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::project(&project.name, project.id),
        breadcrumb_registry::task(&task.title, project.id, task.id),
    ];
    let page_html = leptos::view! {
        <pages::task_detail::TaskDetailPage
            task
            doc_content
            conversations=conversations
            worktree_missing
            commits
            />
    }
    .to_html();
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
}

async fn commit_detail_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((project_id, task_id, oid)): Path<(i64, i64, String)>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;

    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }

    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id || task.project_id != project_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let worktree = services::tasks::get_worktree_by_task(&state.db, task_id)
        .await
        .ok();

    let diff = match worktree {
        Some(w) => {
            let path = w.worktree_path.clone();
            let oid_str = oid.clone();
            tokio::task::spawn_blocking(move || {
                let oid =
                    services::commits::resolve_oid(std::path::Path::new(&path), &oid_str).ok()?;
                services::commits::commit_diff(std::path::Path::new(&path), &oid).ok()
            })
            .await
            .ok()
            .flatten()
        }
        None => None,
    };

    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::project(&project.name, project.id),
        breadcrumb_registry::task(&task.title, project.id, task.id),
        breadcrumb_registry::commit(&oid, project.id, task.id),
    ];
    let page_html = leptos::view! {
        <pages::commit_detail::CommitDetailPage diff project_id task_id />
    }
    .to_html();
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
}

/// "opencode <friendly-name>"; falls back to the config filename (minus
/// `.json`) when the ref is not a user-owned `{uuid}.json` config.
async fn opencode_config_name(
    db: &hiqlite::Client,
    user_id: &Uuid,
    provider_config_ref: &str,
) -> String {
    if let Some(stripped) = provider_config_ref.strip_suffix(".json") {
        if let Ok(cfg_id) = Uuid::parse_str(stripped) {
            if let Some(name) =
                services::settings::get_model_config_name(db, *user_id, cfg_id).await
            {
                return name;
            }
        }
        return stripped.to_string();
    }
    provider_config_ref.to_string()
}

/// "model · opencode <config>" for the opencode harness, else just the model.
async fn status_bar_model_label(
    db: &hiqlite::Client,
    agent_type: &crate::db::schema::AgentType,
    user_id: &Uuid,
    project_id: i64,
    fallback_model: &str,
) -> String {
    match crate::providers::registry::resolve_harness_config(
        db,
        agent_type,
        Some(user_id),
        Some(project_id),
    )
    .await
    {
        Ok(cfg) if cfg.harness == "opencode" => {
            let name = opencode_config_name(db, user_id, &cfg.provider_config_ref).await;
            format!("{fallback_model} · opencode {name}")
        }
        _ => fallback_model.to_string(),
    }
}

async fn chat_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((project_id, task_id)): Path<(i64, i64)>,
) -> Result<Html<String>, ServerError> {
    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }

    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let conversations = services::tasks::list_conversations_for_task(&state.db, task_id)
        .await
        .unwrap_or_default();

    if let Some(first) = conversations.first() {
        let redirect_url = format!(
            "/webapp/projects/{}/tasks/{}/chat/{}",
            project_id, task_id, first.conversation.id
        );
        return Ok(Html(format!(
            "<script>window.location.href='{redirect_url}';</script>"
        )));
    }

    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;
    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::project(&project.name, project.id),
        breadcrumb_registry::task(&task.title, project.id, task.id),
        breadcrumb_registry::chat(),
    ];
    let page_html = leptos::view! {
        <pages::chat::ChatPage
            _project_id=project_id
            task_id
            active_conversation_id=None
            initial_messages=Vec::new()
            conversation_name=None
            current_run=None
            agent_type=None
            model_label=String::new()
        />
    }
    .to_html();
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
}

async fn chat_handler_with_conv(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((project_id, task_id, conversation_id)): Path<(i64, i64, uuid::Uuid)>,
) -> Result<Html<String>, ServerError> {
    let user_json = serde_json::to_string(&auth).unwrap_or_default();
    let agents = active_agents(&state.db, &auth.user_id).await;

    let project = services::projects::get_project(&state.db, project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }

    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let conv = session::resume_session(&state.db, conversation_id)
        .await
        .map_err(|_| ServerError::NotFound("Conversation not found".into()))?;

    if conv.task_id != task_id {
        return Err(ServerError::NotFound("Conversation not found".into()));
    }

    let provider_session_id = conv.provider_session_id.clone().unwrap_or_default();
    let messages = match transcript::load_transcript(&state.db, &provider_session_id, task_id).await
    {
        Ok(msgs) => msgs,
        Err(e) => {
            tracing::error!("Failed to load transcript for conversation {conversation_id}: {e}");
            Vec::new()
        }
    };

    let current_run = services::tasks::get_running_agent_for_task(&state.db, task_id)
        .await
        .ok()
        .flatten();

    let agent_run = services::tasks::get_agent_run_by_conversation(&state.db, &conversation_id)
        .await
        .ok();

    let agent_type = agent_run.as_ref().map(|r| r.agent_type.clone());
    let model_label = match &agent_type {
        Some(at) => {
            status_bar_model_label(&state.db, at, &auth.user_id, project_id, &conv.model).await
        }
        None => conv.model.clone(),
    };

    let conv_name = conv.name.clone().unwrap_or_else(|| conv.model.clone());

    let breadcrumbs = vec![
        breadcrumb_registry::all_projects(),
        breadcrumb_registry::project(&project.name, project.id),
        breadcrumb_registry::task(&task.title, project.id, task.id),
        breadcrumb_registry::chat_conversation(
            &conv_name,
            project.id,
            task.id,
            conversation_id,
            agent_run.as_ref().map(|r| r.agent_type.icon()),
        ),
    ];
    let page_html = leptos::view! {
    <pages::chat::ChatPage
        _project_id=project_id
        task_id
        active_conversation_id=Some(conversation_id)
        initial_messages=messages
        conversation_name=Some(conv_name)
        current_run
        agent_type={agent_type}
        model_label={model_label}
    />
    }
    .to_html();
    Ok(Html(render_shell(
        &page_html,
        Some(user_json),
        breadcrumbs,
        agents,
    )))
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
