pub mod guards;
pub mod recovery;
pub mod state_machine;

use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine;
use hiqlite::Client;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agents::{self, pull_request::PullRequestStatus};
use crate::config::OfmConfig;
use crate::db::schema::{AgentType, RunStatus, SessionDb};
use crate::providers::registry;
use crate::providers::types::{ProviderEvent, TurnInput};
use crate::providers::LlmProvider;
use crate::server::error::ServerError;
use crate::server::state::OidcEndpoints;
use crate::server::ws::bus::BroadcastBus;
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::services::tasks;
use futures_util::FutureExt;

pub const MAX_WORKFLOW_RUNS: i32 = 25;

type DynMap = Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>>;
type TokenMap = Arc<Mutex<HashMap<Uuid, String>>>;

pub enum NextAction {
    StartAgent(AgentType),
    Stop,
    Terminal,
}

pub fn internal_err(e: impl std::fmt::Display) -> ServerError {
    ServerError::Internal(e.to_string())
}

pub async fn completion_handler(
    client: &Client,
    conversation_id: Uuid,
    active_sessions: &Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>>,
) -> Result<NextAction, ServerError> {
    // Provider shutdown is deferred to process exit (see `src/main.rs`).
    // `active_sessions` is retained in the signature for API stability and
    // so callers can continue to pass the shared map.
    let _ = active_sessions;
    let run = tasks::get_agent_run_by_conversation(client, &conversation_id)
        .await
        .map_err(internal_err)?;

    if run.status != RunStatus::Running {
        return Ok(NextAction::Terminal);
    }

    tasks::mark_agent_run_completed(client, &run.id)
        .await
        .map_err(internal_err)?;

    // Do NOT shut down the provider here. The opencode server is persistent
    // across turn completion (mirrors the reference implementation's
    // `OpenCodeServerPool` — see `spec/reference/server/services/providers/
    // opencode/index.ts`). Keeping the provider in `active_sessions` lets a
    // subsequent `send_message` resume the same `session_id` without
    // surfacing a stale-session error. The server is reaped by the signal
    // handlers in `src/main.rs` when ofm exits.

    let task = tasks::get_task(client, run.task_id)
        .await
        .map_err(internal_err)?;

    if task.workflow_run_count >= MAX_WORKFLOW_RUNS {
        tasks::mark_task_blocked(client, run.task_id)
            .await
            .map_err(internal_err)?;
        return Ok(NextAction::Stop);
    }

    let config_statuses =
        registry::resolve_agent_config_statuses(client, task.user_id, task.project_id).await;
    Ok(state_machine::next_agent(
        &task,
        &run.agent_type,
        &config_statuses,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn start_next_agent<'a>(
    db: &'a Client,
    task: &'a crate::db::schema::Task,
    agent_type: AgentType,
    config_root: &'a std::path::Path,
    footprint: &'a str,
    archive_root: &'a str,
    active_sessions: &'a DynMap,
    ws_bus: &'a Arc<BroadcastBus>,
    access_tokens: &'a TokenMap,
    oidc_provider: &'a Option<OidcEndpoints>,
    config: &'a OfmConfig,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<crate::db::schema::TaskAgentRun, ServerError>>
            + Send
            + 'a,
    >,
> {
    Box::pin(async move {
        let harness_config = match registry::resolve_harness_config(
            db,
            &agent_type,
            Some(&task.user_id),
            Some(task.project_id),
        )
        .await
        {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!(task_id = task.id, agent_type = %agent_type, error = %e, "No provider config found, creating blocked run");
                let run = tasks::create_agent_run_blocked(db, task.id, &agent_type)
                    .await
                    .map_err(|e| ServerError::Internal(e.to_string()))?;
                return Ok(run);
            }
        };

        guards::one_running_per_task(db, task.id).await?;
        guards::iteration_cap(task)?;

        tasks::increment_workflow_run_count(db, task.id)
            .await
            .map_err(internal_err)?;

        let model = harness_config
            .model
            .as_deref()
            .unwrap_or("default")
            .to_string();
        let effort = harness_config
            .effort
            .as_deref()
            .unwrap_or("balanced")
            .to_string();

        let session_result = crate::services::session::start_session(
            db,
            task.id,
            &model,
            &effort,
            agent_type.clone(),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

        let mut provider = registry::resolve_provider_for_user(
            &harness_config,
            config_root,
            task.user_id,
            config.info_log_client_data,
        )
        .await
        .map_err(|e| ServerError::Internal(format!("Failed to resolve provider: {e}")))?;

        let working_dir = std::path::Path::new("/tmp");
        provider
            .start(working_dir)
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to start provider: {e}")))?;

        let conv_id_str = session_result.conversation_id.to_string();

        let archive = crate::archive::ArchiveRoot::new(std::path::PathBuf::from(archive_root));
        let worktree = tasks::get_worktree_by_task(db, task.id).await.ok();
        let cwd = worktree
            .as_ref()
            .map(|w| w.worktree_path.clone())
            .unwrap_or_else(|| "/tmp".to_string());

        let task_str = task.id.to_string();
        let doc_path = archive.task_doc_path(&task.project_id.to_string(), &task_str);

        let access_token =
            get_or_refresh_token(db, access_tokens, oidc_provider.as_ref(), task.user_id)
                .await
                .unwrap_or_default();
        if !access_token.is_empty() {
            write_ofm_agent_json(&cwd, &access_token, &config.hostname, config.port);
        }

        let context_prompt = archive
            .build_context_prompt(
                footprint,
                task.project_id,
                task.id,
                &config.hostname,
                config.port,
                std::process::id(),
            )
            .ok()
            .unwrap_or_default();

        let prompt_text = {
            let phase_prompt = match agent_type {
                AgentType::Planification => {
                    agents::planning::build_planning_prompt(&doc_path.to_string_lossy(), &task_str)
                }
                AgentType::Implementation => {
                    agents::implementation::build_implementation_prompt(&doc_path.to_string_lossy())
                }
                AgentType::Review => {
                    agents::review::build_review_prompt(task.id, &doc_path.to_string_lossy())
                }
                AgentType::Refinement => agents::refinement::build_refinement_prompt(
                    task.id,
                    &doc_path.to_string_lossy(),
                ),
                AgentType::Pr => agents::pull_request::build_pull_request_prompt(
                    task.id,
                    &doc_path.to_string_lossy(),
                    &PullRequestStatus::NoPr,
                ),
                _ => String::new(),
            };
            [phase_prompt, context_prompt]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        };

        let turn_input = TurnInput::new(
            prompt_text.clone(),
            cwd,
            model,
            effort,
            "auto".to_string(),
            vec![],
            String::new(),
        )
        .session_id(session_result.session_id.clone());

        match provider.start_turn(turn_input).await {
            Ok(mut rx) => {
                active_sessions.lock().await.insert(conv_id_str, provider);

                let db = db.clone();
                let ws_bus = ws_bus.clone();
                let active_sessions = active_sessions.clone();
                let access_tokens = access_tokens.clone();
                let config = config.clone();
                let oidc_for_spawn = oidc_provider.clone();
                let conversation_id = session_result.conversation_id;
                let task_id = task.id;
                let mut s_id = session_result.session_id;
                let prompt_text = prompt_text.clone();
                let config_root = config_root.to_path_buf();
                let footprint = footprint.to_string();
                let archive_root = archive_root.to_string();

                tokio::spawn(async move {
                    let completed_normally = Arc::new(AtomicBool::new(false));

                    let broadcast_fut = AssertUnwindSafe(async {
                        loop {
                            tokio::select! {
                                event = rx.recv() => {
                                    let event = match event { Some(e) => e, None => break };

                                    let topic = WsTopic { kind: WsTopicKind::Task, id: TopicId(task_id) };

                                    if let Err(e) = crate::services::transcript::persist_event(&db, &event, &s_id, task_id).await {
                                        tracing::error!("Failed to persist event: {e}");
                                        let error_event = ProviderEvent::Error { error: format!("Failed to persist event: {e}"), timestamp: chrono::Utc::now().naive_utc() };
                                        let (event_type, payload) = error_event.to_ws_event();
                                        ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type, timestamp: chrono::Utc::now(), payload, html: None }).await;
                                        break;
                                    }

                                    if let ProviderEvent::SessionStart { session_id } = &event {
                                        s_id = session_id.clone();
                                        let _ = db.execute("UPDATE conversations SET provider_session_id = $1 WHERE id = $2", hiqlite::params!(session_id, conversation_id.to_string())).await;
                                        let prompt_ts = chrono::Utc::now().naive_utc();
                                        let prompt_event = ProviderEvent::UserText { text: prompt_text.clone(), timestamp: prompt_ts };
                                        if let Err(e) = crate::services::transcript::persist_event(&db, &prompt_event, &s_id, task_id).await {
                                            tracing::error!("Failed to persist initial prompt: {e}");
                                            let error_event = ProviderEvent::Error { error: format!("Failed to persist initial prompt: {e}"), timestamp: chrono::Utc::now().naive_utc() };
                                            let (event_type, payload) = error_event.to_ws_event();
                                            ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type, timestamp: chrono::Utc::now(), payload, html: None }).await;
                                            break;
                                        }
                                        let prompt_ts_str = prompt_ts.format("%Y-%m-%d %H:%M:%S").to_string();
                                        ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type: "user_text".to_string(), timestamp: chrono::Utc::now(), payload: serde_json::json!({"text": prompt_text, "conversation_id": conversation_id.to_string(), "timestamp": prompt_ts_str}), html: Some(crate::webapp::components::message_stream::render_event(&prompt_event)) }).await;
                                    }

                                    let (event_type, payload) = event.to_ws_event();
                                    let is_done = matches!(event, ProviderEvent::Done { .. });
                                    let rendered = crate::webapp::components::message_stream::render_event(&event);
                                    ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type, timestamp: chrono::Utc::now(), payload, html: if rendered.is_empty() { None } else { Some(rendered) } }).await;

                                    if is_done {
                                        completed_normally.store(true, Ordering::SeqCst);
                                        let done_now = chrono::Utc::now().naive_utc().to_string();
                                        let _ = db.execute("UPDATE conversations SET updated_at = $1 WHERE id = $2", hiqlite::params!(&done_now, conversation_id.to_string())).await;
                                        match completion_handler(&db, conversation_id, &active_sessions).await {
                                            Ok(NextAction::StartAgent(agent_type)) => {
                                                let db = db.clone();
                                                let config_root = config_root.clone();
                                                let footprint = footprint.clone();
                                                let archive_root = archive_root.clone();
                                                let active_sessions = active_sessions.clone();
                                                let ws_bus = ws_bus.clone();
                                                let access_tokens = access_tokens.clone();
                                                let config = config.clone();
                                                let oidc_for_spawn = oidc_for_spawn.clone();
                                                tokio::spawn(async move {
                                                    if let Ok(task) = tasks::get_task(&db, task_id).await {
                                                        if let Err(e) = start_next_agent(&db, &task, agent_type, &config_root, &footprint, &archive_root, &active_sessions, &ws_bus, &access_tokens, &oidc_for_spawn, &config).await {
                                                            tracing::warn!("Failed to auto-advance to next agent: {e:?}");
                                                        }
                                                    }
                                                });
                                            }
                                            Ok(_) => {}
                                            Err(e) => tracing::warn!("Error in completion handler: {e:?}"),
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    });

                    let result = broadcast_fut.catch_unwind().await;
                    if let Err(ref panic) = result {
                        tracing::error!("Broadcast task panicked: {panic:?}");
                    }

                    if !completed_normally.load(Ordering::SeqCst) {
                        {
                            let sessions = active_sessions.lock().await;
                            if let Some(p) = sessions.get(&conversation_id.to_string()) {
                                if let Err(e) = p.abort_turn().await {
                                    tracing::warn!(conversation_id = %conversation_id, "Error aborting provider in broadcast cleanup: {e}");
                                }
                            }
                        }
                        let _ = db.execute("UPDATE task_agent_runs SET status = 'failed' WHERE conversation_id = $1", hiqlite::params!(conversation_id.to_string())).await;
                        let topic = WsTopic {
                            kind: WsTopicKind::Task,
                            id: TopicId(task_id),
                        };
                        let error_event = ProviderEvent::Error {
                            error: "Agent session ended unexpectedly. Send a message to resume."
                                .into(),
                            timestamp: chrono::Utc::now().naive_utc(),
                        };
                        ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type: "error".to_string(), timestamp: chrono::Utc::now(), payload: serde_json::json!({"error": "Agent session ended unexpectedly. Send a message to resume."}), html: Some(crate::webapp::components::message_stream::render_event(&error_event)) }).await;
                    }
                });
            }
            Err(e) => {
                tracing::warn!("Failed to start turn: {e}");
                active_sessions.lock().await.insert(conv_id_str, provider);
            }
        }

        let run = tasks::get_agent_run_by_conversation(db, &session_result.conversation_id)
            .await
            .map_err(internal_err)?;
        Ok(run)
    })
}

pub(crate) fn decode_jwt_exp(token: &str) -> Option<i64> {
    let payload_part = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_part)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    claims.get("exp")?.as_i64()
}

pub(crate) fn write_ofm_agent_json(cwd: &str, access_token: &str, hostname: &str, port: u16) {
    if access_token.is_empty() {
        tracing::error!(cwd = %cwd, "Skipping .ofm_agent.json write: empty access token");
        return;
    }
    if let Some(exp) = decode_jwt_exp(access_token) {
        if exp <= chrono::Utc::now().timestamp() {
            tracing::error!(cwd = %cwd, exp = %exp, "Skipping .ofm_agent.json write: expired access token");
            return;
        }
    }
    let token_expiration = decode_jwt_exp(access_token).unwrap_or(0);
    let agent_vars = serde_json::json!({
        "agentVars": {
            "accessToken": access_token,
            "tokenExpiration": token_expiration,
            "ofmHost": hostname,
            "ofmPort": port,
            "ofmPid": std::process::id(),
        }
    });
    let json_path = std::path::Path::new(cwd).join(".ofm_agent.json");
    match serde_json::to_string_pretty(&agent_vars) {
        Ok(json_str) => {
            if let Err(e) = std::fs::write(&json_path, &json_str) {
                tracing::warn!(path = %json_path.display(), "Failed to write .ofm_agent.json: {e}");
            } else {
                tracing::info!(path = %json_path.display(), "Wrote .ofm_agent.json with access token");
                #[cfg(unix)]
                {
                    let _ = std::fs::set_permissions(
                        &json_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize .ofm_agent.json: {e}");
        }
    }
}

pub(crate) async fn get_or_refresh_token(
    db: &hiqlite::Client,
    access_tokens: &TokenMap,
    oidc: Option<&OidcEndpoints>,
    user_id: Uuid,
) -> Option<String> {
    {
        let cache = access_tokens.lock().await;
        if let Some(token) = cache.get(&user_id) {
            if let Some(exp) = decode_jwt_exp(token) {
                if exp > chrono::Utc::now().timestamp() {
                    return Some(token.clone());
                }
            } else {
                return Some(token.clone());
            }
        }
    }
    let oidc = oidc?;
    let session: SessionDb = db
        .query_raw(
            "SELECT * FROM sessions WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1",
            hiqlite::params!(user_id.to_string()),
        )
        .await
        .ok()
        .and_then(|mut rows| rows.first_mut().map(|r| SessionDb::from(&mut *r)))?;
    if !session.access_token.is_empty()
        && crate::services::auth::validate_access_token(oidc, &session.access_token).await
    {
        let mut cache = access_tokens.lock().await;
        cache.insert(user_id, session.access_token.clone());
        return Some(session.access_token);
    }
    match crate::services::auth::refresh_access_token(db, oidc, session.id).await {
        Ok(token) => {
            let mut cache = access_tokens.lock().await;
            cache.insert(user_id, token.clone());
            Some(token)
        }
        Err(e) => {
            let is_jwt_token =
                matches!(&e, ServerError::BadRequest(msg) if msg.contains("JwtToken"));
            if is_jwt_token {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                match crate::services::auth::refresh_access_token(db, oidc, session.id).await {
                    Ok(token) => {
                        let mut cache = access_tokens.lock().await;
                        cache.insert(user_id, token.clone());
                        return Some(token);
                    }
                    Err(_) => {
                        let _ = db
                            .execute(
                                "DELETE FROM sessions WHERE id = $1",
                                hiqlite::params!(session.id.to_string()),
                            )
                            .await;
                    }
                }
            }
            tracing::error!(user_id = %user_id, "Failed to refresh access token: {e:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::AgentType;
    use crate::server::state::OidcEndpoints;
    use crate::services::session;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::Json;
    use axum::Router;
    use serde_json::json;
    use tempfile::TempDir;

    fn empty_sessions() -> Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>> {
        Arc::new(Mutex::new(HashMap::new()))
    }

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
    async fn test_completion_handler_running_to_completed() {
        let (client, task_id, _tmp) = make_client().await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        // Seed a review config so the phase-skip check passes
        let now = chrono::Utc::now().naive_utc().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                hiqlite::params!(
                    uuid::Uuid::new_v4().to_string(),
                    "review",
                    "opencode",
                    "test.json",
                    "global",
                    "gpt-4",
                    "balanced",
                    &now,
                    &now,
                ),
            )
            .await
            .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        let run = tasks::get_agent_run_by_conversation(&client, &result.conversation_id)
            .await
            .unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[tokio::test]
    async fn test_completion_handler_failed_no_chain() {
        let (client, task_id, _tmp) = make_client().await;

        let result =
            session::start_session(&client, task_id, "model", "balanced", AgentType::Review)
                .await
                .unwrap();

        tasks::mark_agent_run_failed(
            &client,
            &tasks::get_agent_run_by_conversation(&client, &result.conversation_id)
                .await
                .unwrap()
                .id,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Terminal));
    }

    #[tokio::test]
    async fn test_completion_handler_planning_stops() {
        let (client, task_id, _tmp) = make_client().await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Planification,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));
    }

    #[tokio::test]
    async fn test_completion_handler_iteration_cap_auto_blocks() {
        let (client, task_id, _tmp) = make_client().await;

        client
            .execute(
                "UPDATE tasks SET workflow_run_count = 25 WHERE id = $1",
                hiqlite::params!(task_id),
            )
            .await
            .unwrap();

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let action = completion_handler(&client, result.conversation_id, &sessions)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));

        let task = tasks::get_task(&client, task_id).await.unwrap();
        assert!(task.workflow_blocked);
    }

    #[test]
    fn test_decode_jwt_exp_valid_token() {
        // Create a JWT with known exp claim
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"alg\":\"HS256\"}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode("{\"exp\":1700000000,\"sub\":\"test\"}");
        let token = format!("{header}.{payload}.signature");
        assert_eq!(decode_jwt_exp(&token), Some(1700000000_i64));
    }

    #[test]
    fn test_decode_jwt_exp_missing_exp() {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"alg\":\"HS256\"}");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"sub\":\"test\"}");
        let token = format!("{header}.{payload}.signature");
        assert_eq!(decode_jwt_exp(&token), None);
    }

    #[test]
    fn test_decode_jwt_exp_invalid_token() {
        assert_eq!(decode_jwt_exp("not-a-jwt"), None);
    }

    #[test]
    fn test_decode_jwt_exp_empty_token() {
        assert_eq!(decode_jwt_exp(""), None);
    }

    #[test]
    fn test_write_agent_json_skips_empty_token() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path().to_str().unwrap();
        write_ofm_agent_json(cwd, "", "127.0.0.1", 3183);
        let json_path = tmp.path().join(".ofm_agent.json");
        assert!(
            !json_path.exists(),
            "file should NOT be written for empty token"
        );
    }

    #[test]
    fn test_write_agent_json_skips_expired_token() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path().to_str().unwrap();
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"alg\":\"HS256\"}");
        let past_ts = chrono::Utc::now().timestamp() - 3600;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"exp":{past_ts},"sub":"test"}}"#));
        let expired_token = format!("{header}.{payload}.signature");
        write_ofm_agent_json(cwd, &expired_token, "127.0.0.1", 3183);
        let json_path = tmp.path().join(".ofm_agent.json");
        assert!(
            !json_path.exists(),
            "file should NOT be written for expired token"
        );
    }

    #[test]
    fn test_write_agent_json_writes_valid_token() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cwd = tmp.path().to_str().unwrap();
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"alg\":\"HS256\"}");
        let future_ts = chrono::Utc::now().timestamp() + 3600;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"exp":{future_ts},"sub":"test"}}"#));
        let valid_token = format!("{header}.{payload}.signature");
        write_ofm_agent_json(cwd, &valid_token, "127.0.0.1", 3183);
        let json_path = tmp.path().join(".ofm_agent.json");
        assert!(json_path.exists(), "file should be written for valid token");
        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["agentVars"]["accessToken"], valid_token);
        assert_eq!(parsed["agentVars"]["ofmHost"], "127.0.0.1");
        assert_eq!(parsed["agentVars"]["ofmPort"], 3183);
    }

    #[test]
    fn test_ofm_agent_json_structure() {
        // Validate the JSON structure matches expected schema
        let agent_vars = serde_json::json!({
            "agentVars": {
                "accessToken": "test-token",
                "tokenExpiration": 1700000000_i64,
                "ofmHost": "127.0.0.1",
                "ofmPort": 3183_u16,
                "ofmPid": 12345_u32,
            }
        });
        let json_str = serde_json::to_string_pretty(&agent_vars).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["agentVars"]["accessToken"], "test-token");
        assert_eq!(parsed["agentVars"]["tokenExpiration"], 1700000000);
        assert_eq!(parsed["agentVars"]["ofmHost"], "127.0.0.1");
        assert_eq!(parsed["agentVars"]["ofmPort"], 3183);
        assert_eq!(parsed["agentVars"]["ofmPid"], 12345);
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
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

        let user_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, 1, $3, 1, 0)",
                hiqlite::params!(user_id.to_string(), "fallbackuser", now.clone()),
            )
            .await
            .unwrap();

        let session_id = uuid::Uuid::new_v4();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "test-refresh-token-for-fallback", future, now),
            )
            .await
            .unwrap();

        let mock_app = Router::new().route(
            "/token",
            post(|| async {
                Json(json!({
                    "access_token": "new-access-token-from-mock",
                    "refresh_token": "new-refresh-token",
                    "expires_in": 7200
                }))
            }),
        );
        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_addr = mock_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(mock_listener, mock_app).await.unwrap() });

        let oidc = OidcEndpoints {
            end_session_endpoint: None,
            authorization_endpoint: format!("http://{}/auth", mock_addr),
            token_endpoint: format!("http://{}/token", mock_addr),
            revocation_endpoint: None,
            userinfo_endpoint: format!("http://{}/userinfo", mock_addr),
            client_id: "test-client".into(),
            client_secret: None,
            redirect_uri: format!("http://{}/callback", mock_addr),
            jwks_cache: None,
            jwks_issuer: None,
        };

        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));

        let result = get_or_refresh_token(&client, &access_tokens, Some(&oidc), user_id).await;

        assert!(
            result.is_some(),
            "should have obtained a token via OIDC refresh"
        );
        assert_eq!(
            result.unwrap(),
            "new-access-token-from-mock",
            "token should match mock response"
        );

        let cache = access_tokens.lock().await;
        assert!(
            cache.contains_key(&user_id),
            "token should be cached after refresh"
        );
        assert_eq!(cache.get(&user_id).unwrap(), "new-access-token-from-mock");
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_cache_hit() {
        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));
        let user_id = uuid::Uuid::new_v4();

        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("{\"alg\":\"HS256\"}");
        let future_ts = chrono::Utc::now().timestamp() + 3600;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(format!(r#"{{"exp":{future_ts},"sub":"test"}}"#));
        let valid_token = format!("{header}.{payload}.signature");

        {
            let mut cache = access_tokens.lock().await;
            cache.insert(user_id, valid_token.clone());
        }

        // Cache-hit path never touches DB; use a dummy client by starting a
        // minimal hiqlite node with a temporary dir that stays alive.
        let _tmp = tempfile::TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: _tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-123".into(),
            secret_api: "test-api-secret-123".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;

        let result = get_or_refresh_token(&client, &access_tokens, None, user_id).await;

        assert!(result.is_some(), "should return cached token");
        assert_eq!(result.unwrap(), valid_token, "should match cached value");
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_persisted_token_valid() {
        let tmp = tempfile::TempDir::new().unwrap();
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

        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let persisted_token = "persisted-valid-token";
        client
            .execute(
                "INSERT INTO users (id, username, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, 1, $3, 1, 0)",
                hiqlite::params!(user_id.to_string(), "persisteduser", now.clone()),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, access_token, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), persisted_token, "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        let mock_app = Router::new().route("/userinfo", get(|| async { (StatusCode::OK, "ok") }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

        let oidc = OidcEndpoints {
            end_session_endpoint: None,
            authorization_endpoint: format!("http://{}/auth", addr),
            token_endpoint: format!("http://{}/token", addr),
            revocation_endpoint: None,
            userinfo_endpoint: format!("http://{}/userinfo", addr),
            client_id: "test-client".into(),
            client_secret: None,
            redirect_uri: format!("http://{}/callback", addr),
            jwks_cache: None,
            jwks_issuer: None,
        };

        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));
        let result = get_or_refresh_token(&client, &access_tokens, Some(&oidc), user_id).await;

        assert!(result.is_some(), "should return the persisted valid token");
        assert_eq!(result.unwrap(), persisted_token);
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_stale_persisted_token() {
        let tmp = tempfile::TempDir::new().unwrap();
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

        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        client
            .execute(
                "INSERT INTO users (id, username, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, 1, $3, 1, 0)",
                hiqlite::params!(user_id.to_string(), "staleuser", now.clone()),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, access_token, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "stale-token", "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        // Userinfo returns 401 (stale), token endpoint returns success
        let mock_app = Router::new()
            .route(
                "/userinfo",
                get(|| async { (StatusCode::UNAUTHORIZED, "unauthorized") }),
            )
            .route(
                "/token",
                post(|| async {
                    Json(json!({
                        "access_token": "fresh-access-token",
                        "refresh_token": "new-refresh-token",
                        "expires_in": 7200
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

        let oidc = OidcEndpoints {
            end_session_endpoint: None,
            authorization_endpoint: format!("http://{}/auth", addr),
            token_endpoint: format!("http://{}/token", addr),
            revocation_endpoint: None,
            userinfo_endpoint: format!("http://{}/userinfo", addr),
            client_id: "test-client".into(),
            client_secret: None,
            redirect_uri: format!("http://{}/callback", addr),
            jwks_cache: None,
            jwks_issuer: None,
        };

        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));
        let result = get_or_refresh_token(&client, &access_tokens, Some(&oidc), user_id).await;

        assert!(
            result.is_some(),
            "should fall through to refresh and succeed"
        );
        assert_eq!(result.unwrap(), "fresh-access-token");
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_jwt_token_retry() {
        let tmp = tempfile::TempDir::new().unwrap();
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

        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        client
            .execute(
                "INSERT INTO users (id, username, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, 1, $3, 1, 0)",
                hiqlite::params!(user_id.to_string(), "retryuser", now.clone()),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, access_token, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "", "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mock_app = {
            let cc = call_count.clone();
            Router::new()
                .route(
                    "/userinfo",
                    get(|| async { (StatusCode::UNAUTHORIZED, "unauthorized") }),
                )
                .route(
                    "/token",
                    post(move || {
                        let prev = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        async move {
                            if prev == 0 {
                                // First call → JwtToken error
                                (StatusCode::BAD_REQUEST, Json(json!({"error": "JwtToken"})))
                            } else {
                                // Second call → success
                                (
                                    StatusCode::OK,
                                    Json(json!({
                                        "access_token": "token-after-retry",
                                        "refresh_token": "new-refresh-token",
                                        "expires_in": 7200
                                    })),
                                )
                            }
                        }
                    }),
                )
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

        let oidc = OidcEndpoints {
            end_session_endpoint: None,
            authorization_endpoint: format!("http://{}/auth", addr),
            token_endpoint: format!("http://{}/token", addr),
            revocation_endpoint: None,
            userinfo_endpoint: format!("http://{}/userinfo", addr),
            client_id: "test-client".into(),
            client_secret: None,
            redirect_uri: format!("http://{}/callback", addr),
            jwks_cache: None,
            jwks_issuer: None,
        };

        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));
        let result = get_or_refresh_token(&client, &access_tokens, Some(&oidc), user_id).await;

        assert!(result.is_some(), "should succeed after retry");
        assert_eq!(result.unwrap(), "token-after-retry");
    }

    #[tokio::test]
    async fn test_get_or_refresh_token_jwt_token_retry_exhaustion() {
        let tmp = tempfile::TempDir::new().unwrap();
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

        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        client
            .execute(
                "INSERT INTO users (id, username, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, 1, $3, 1, 0)",
                hiqlite::params!(user_id.to_string(), "exhaustuser", now.clone()),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, access_token, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "", "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        let mock_app = Router::new()
            .route(
                "/userinfo",
                get(|| async { (StatusCode::UNAUTHORIZED, "unauthorized") }),
            )
            .route(
                "/token",
                post(|| async { (StatusCode::BAD_REQUEST, Json(json!({"error": "JwtToken"}))) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

        let oidc = OidcEndpoints {
            end_session_endpoint: None,
            authorization_endpoint: format!("http://{}/auth", addr),
            token_endpoint: format!("http://{}/token", addr),
            revocation_endpoint: None,
            userinfo_endpoint: format!("http://{}/userinfo", addr),
            client_id: "test-client".into(),
            client_secret: None,
            redirect_uri: format!("http://{}/callback", addr),
            jwks_cache: None,
            jwks_issuer: None,
        };

        let access_tokens: TokenMap = Arc::new(Mutex::new(HashMap::new()));
        let result = get_or_refresh_token(&client, &access_tokens, Some(&oidc), user_id).await;

        assert!(
            result.is_none(),
            "should return None after retry exhaustion"
        );

        // Verify session was deleted
        let mut rows = client
            .query_raw(
                "SELECT COUNT(*) AS cnt FROM sessions WHERE id = $1",
                hiqlite::params!(session_id.to_string()),
            )
            .await
            .unwrap();
        let count: i64 = rows[0].get("cnt");
        assert_eq!(count, 0, "session should be deleted after retry exhaustion");
    }
}
