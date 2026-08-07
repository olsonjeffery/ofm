pub mod guards;
pub mod recovery;
pub mod state_machine;

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use hiqlite::Client;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agents::{self, pull_request::PullRequestStatus};
use crate::config::OfmConfig;
use crate::db::schema::{AgentType, RunStatus};
use crate::providers::registry;
use crate::providers::types::{ProviderEvent, TurnInput};
use crate::providers::LlmProvider;
use crate::server::error::ServerError;
use crate::server::routes::conversations::is_text_echo;
use crate::server::ws::bus::BroadcastBus;
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::services::tasks;
use futures_util::FutureExt;

fn system_topic() -> WsTopic {
    WsTopic {
        kind: WsTopicKind::System,
        id: TopicId(0),
    }
}

/// Broadcast a "global agent status changed" signal on the System topic. The
/// navbar agent-dropdown subscribes to this topic on every page, so any agent
/// lifecycle transition (start, complete, fail, stop, blocked, question) must
/// publish through here for the dropdown to stay current.
pub async fn broadcast_agent_status(ws_bus: &Arc<BroadcastBus>, action: &str) {
    let topic = system_topic();
    ws_bus
        .broadcast(
            &topic,
            ServerMessage::Event {
                topic: topic.clone(),
                event_type: "agent_status".to_string(),
                timestamp: chrono::Utc::now(),
                payload: serde_json::json!({"action": action}),
                html: None,
            },
        )
        .await;
}

pub const MAX_WORKFLOW_RUNS: i32 = 25;

type DynMap = Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>>;

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
    ws_bus: &Arc<BroadcastBus>,
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

    let topic = system_topic();
    ws_bus
        .broadcast(
            &topic,
            ServerMessage::Event {
                topic: topic.clone(),
                event_type: "agent_status".to_string(),
                timestamp: chrono::Utc::now(),
                payload: serde_json::json!({"action": "completed", "conversation_id": run.id.to_string()}),
                html: None,
            },
        )
        .await;

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

        // Surface the block live so open pages (task detail, chat) reload and
        // show the recovery banner without a manual refresh.
        if let Ok(task) = tasks::get_task(client, run.task_id).await {
            let topic = WsTopic {
                kind: WsTopicKind::Task,
                id: TopicId(task.id),
            };
            ws_bus
                .broadcast(
                    &topic,
                    ServerMessage::Event {
                        topic: topic.clone(),
                        event_type: "task_updated".to_string(),
                        timestamp: chrono::Utc::now(),
                        payload: serde_json::to_value(&task).unwrap_or_default(),
                        html: None,
                    },
                )
                .await;
        }

        return Ok(NextAction::Stop);
    }

    let config_statuses =
        registry::resolve_agent_config_statuses(client, task.user_id, task.project_id).await;

    let review_ready = if run.agent_type == AgentType::Review {
        match crate::services::session::resume_session(client, conversation_id).await {
            Ok(conv) => match conv.provider_session_id {
                Some(ref session_id) => {
                    crate::services::transcript::last_model_text(client, session_id, run.task_id)
                        .await
                        .ok()
                        .flatten()
                        .is_some_and(|text| text.contains("READY"))
                }
                None => false,
            },
            Err(_) => false,
        }
    } else {
        false
    };

    Ok(state_machine::next_agent(
        &task,
        &run.agent_type,
        &config_statuses,
        review_ready,
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
                broadcast_agent_status(ws_bus, "blocked").await;
                return Ok(run);
            }
        };

        // Rig configs are capture-only until RIG 1 lands. Reject the run
        // *before* `start_session` inserts a run row so the agent is cleanly
        // blocked from execution with an actionable message.
        if harness_config.harness == "rig" {
            return Err(ServerError::Conflict(
                registry::rig_not_yet_executable_message(&harness_config.provider_config_ref),
            ));
        }

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

        broadcast_agent_status(ws_bus, "refresh").await;

        let mut provider = registry::resolve_provider_for_user(
            &harness_config,
            config_root,
            task.user_id,
            config.info_log_client_data,
            std::path::Path::new(footprint),
        )
        .await
        .map_err(|e| ServerError::Internal(format!("Failed to resolve provider: {e}")))?;

        let conv_id_str = session_result.conversation_id.to_string();

        let archive = crate::archive::ArchiveRoot::new(std::path::PathBuf::from(archive_root));
        let worktree = tasks::get_worktree_by_task(db, task.id).await.ok();
        let cwd = worktree
            .as_ref()
            .map(|w| w.worktree_path.clone())
            .unwrap_or_else(|| "/tmp".to_string());

        provider
            .start(std::path::Path::new(&cwd))
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to start provider: {e}")))?;

        let task_str = task.id.to_string();
        let doc_path = archive.task_doc_path(&task.project_id.to_string(), &task_str);

        let context_prompt = archive
            .build_context_prompt(
                footprint,
                task.project_id,
                task.id,
                &config.agent_host(),
                config.port,
                std::process::id(),
            )
            .ok()
            .unwrap_or_default();

        let prompt_text = {
            // Prompt Library resolution runs first: a designation for this
            // agent_type at (project/global) scope replaces the stock template.
            // On any resolution/rendering failure we fall back to the existing
            // template builders, so the default flow is untouched unless a
            // user deliberately designates a prompt.
            let library_prompt = crate::services::prompts::resolve_prompt_for_agent(
                db,
                &agent_type,
                &task.user_id,
                task.project_id,
            )
            .await
            .ok()
            .flatten();

            let phase_prompt = match library_prompt {
                Some(prompt) => {
                    let project = crate::services::projects::get_project(db, task.project_id)
                        .await
                        .ok();
                    let default_branch = if let Some(ref p) = project {
                        crate::worktree::detect_default_branch(&p.repo_folder_path)
                            .await
                            .unwrap_or_else(|_| "main".into())
                    } else {
                        "main".to_string()
                    };
                    let vars = crate::prompts::PromptVars {
                        task_id: task.id.to_string(),
                        project_id: task.project_id.to_string(),
                        task_doc_path: doc_path.to_string_lossy().into_owned(),
                        task_worktree_path: cwd.clone(),
                        task_worktree_branch: worktree
                            .as_ref()
                            .map(|w| w.branch.clone())
                            .unwrap_or_default(),
                        project_default_branch: default_branch,
                        project_name: project.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
                        task_name: task.title.clone(),
                        tags: project
                            .as_ref()
                            .map(|p| p.tags.join(", "))
                            .unwrap_or_default(),
                    };
                    let flattened = crate::services::prompts::flattened_content(db, &prompt)
                        .await
                        .unwrap_or_else(|_| prompt.content.clone());
                    crate::prompts::render(&flattened, &vars)
                }
                None => match agent_type {
                    AgentType::Planification => agents::planning::build_planning_prompt(
                        &doc_path.to_string_lossy(),
                        &task_str,
                    ),
                    AgentType::Implementation => {
                        agents::implementation::build_implementation_prompt(
                            &doc_path.to_string_lossy(),
                        )
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
                },
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
                let config = config.clone();
                let conversation_id = session_result.conversation_id;
                let task_id = task.id;
                let mut s_id = session_result.session_id;
                let prompt_text = prompt_text.clone();
                let config_root = config_root.to_path_buf();
                let footprint = footprint.to_string();
                let archive_root = archive_root.to_string();
                let task_user_id = task.user_id;
                let task_project_id = task.project_id;

                tokio::spawn(async move {
                    let completed_normally = Arc::new(AtomicBool::new(false));

                    let broadcast_fut = AssertUnwindSafe(async {
                        loop {
                            tokio::select! {
                                event = rx.recv() => {
                                    let event = match event { Some(e) => e, None => break };

                                    // Skip echoed prompt text — Text events > 90% similar to prompt_text
                                    if let ProviderEvent::Text { text, .. } = &event {
                                        if is_text_echo(text, &prompt_text) {
                                            tracing::debug!(session_id = %s_id, "Skipping echoed prompt Text event");
                                            continue;
                                        }
                                    }

                                    let topic = WsTopic { kind: WsTopicKind::Task, id: TopicId(task_id) };

                                    if let Err(e) = crate::services::transcript::persist_event(&db, &event, &s_id, task_id).await {
                                        tracing::error!("Failed to persist event: {e}");
                                        let error_event = ProviderEvent::Error { error: format!("Failed to persist event: {e}"), timestamp: chrono::Utc::now().naive_utc() };
                                        let (event_type, payload) = error_event.to_ws_event();
                                        let payload = if let Some(obj) = payload.as_object() {
                                            let mut map = obj.clone();
                                            map.insert("conversation_id".to_string(), serde_json::json!(conversation_id.to_string()));
                                            serde_json::Value::Object(map)
                                        } else {
                                            serde_json::json!({"conversation_id": conversation_id.to_string()})
                                        };
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
                                            let payload = if let Some(obj) = payload.as_object() {
                                                let mut map = obj.clone();
                                                map.insert("conversation_id".to_string(), serde_json::json!(conversation_id.to_string()));
                                                serde_json::Value::Object(map)
                                            } else {
                                                serde_json::json!({"conversation_id": conversation_id.to_string()})
                                            };
                                            ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type, timestamp: chrono::Utc::now(), payload, html: None }).await;
                                            break;
                                        }
                                        let prompt_ts_str = prompt_ts.format("%Y-%m-%d %H:%M:%S").to_string();
                                        ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type: "user_text".to_string(), timestamp: chrono::Utc::now(), payload: serde_json::json!({"text": prompt_text, "conversation_id": conversation_id.to_string(), "timestamp": prompt_ts_str}), html: Some(crate::webapp::components::message_stream::render_event(&prompt_event)) }).await;

                                        // Fire-and-forget conversation title generation. Uses the
                                        // harness config assigned to the `conversation_title` agent
                                        // type (separate from the agent-run config); skips silently
                                        // when none is configured.
                                        tracing::info!(
                                            conversation_id = %conversation_id,
                                            task_id = task_id,
                                            prompt_preview = %prompt_text.chars().take(120).collect::<String>(),
                                            "orchestration: resolving conversation_title config for title generation"
                                        );
                                        let title_harness_config = registry::resolve_harness_config(
                                            &db,
                                            &AgentType::ConversationTitle,
                                            Some(&task_user_id),
                                            Some(task_project_id),
                                        )
                                        .await;
                                        if let Ok(title_config) = title_harness_config {
                                            tracing::info!(
                                                conversation_id = %conversation_id,
                                                provider_config_ref = %title_config.provider_config_ref,
                                                harness = %title_config.harness,
                                                "orchestration: spawning title generation after SessionStart"
                                            );
                                            let _title_db = db.clone();
                                            let _title_config_root = config_root.clone();
                                            let _title_harness = title_config;
                                            let _title_conv_id = conversation_id;
                                            let _title_prompt = prompt_text.clone();
                                            let _title_log_data = config.info_log_client_data;
                                            let _title_ws_bus = ws_bus.clone();
                                            let _title_task_id = task_id;
                                            let _title_footprint = footprint.to_string();
                                            tokio::spawn(async move {
                                                tracing::info!(
                                                    conversation_id = %_title_conv_id,
                                                    "orchestration: title generation task started"
                                                );
                                                crate::providers::generate_conversation_title(
                                                    &_title_db,
                                                    &_title_config_root,
                                                    &_title_harness,
                                                    _title_conv_id,
                                                    &_title_prompt,
                                                    _title_log_data,
                                                    std::path::Path::new(&_title_footprint),
                                                ).await;
                                                tracing::info!(
                                                    conversation_id = %_title_conv_id,
                                                    "orchestration: generate_conversation_title returned"
                                                );
                                                match crate::services::session::resume_session(&_title_db, _title_conv_id).await {
                                                    Ok(conv) => {
                                                        tracing::info!(
                                                            conversation_id = %_title_conv_id,
                                                            name = ?conv.name,
                                                            is_valid = conv.name.as_deref().map(crate::webapp::components::conversation_list::is_valid_name).unwrap_or(false),
                                                            "orchestration: resumed conversation after title generation"
                                                        );
                                                        if let Some(ref name) = conv.name {
                                                            if crate::webapp::components::conversation_list::is_valid_name(name) {
                                                                tracing::info!(conversation_id = %_title_conv_id, name = %name, "orchestration: broadcasting conversation-name-updated");
                                                                _title_ws_bus.broadcast(
                                                                    &WsTopic { kind: WsTopicKind::Task, id: TopicId(_title_task_id) },
                                                                    ServerMessage::Event {
                                                                        topic: WsTopic { kind: WsTopicKind::Task, id: TopicId(_title_task_id) },
                                                                        event_type: "conversation-name-updated".to_string(),
                                                                        timestamp: chrono::Utc::now(),
                                                                        payload: serde_json::json!({
                                                                            "conversation_id": _title_conv_id.to_string(),
                                                                            "name": name,
                                                                        }),
                                                                        html: None,
                                                                    },
                                                                ).await;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            conversation_id = %_title_conv_id,
                                                            error = %e,
                                                            "orchestration: resume_session failed after title generation"
                                                        );
                                                    }
                                                }
                                            });
                                        } else {
                                            tracing::info!(
                                                conversation_id = %conversation_id,
                                                "orchestration: no conversation_title config — skipping title generation"
                                            );
                                        }
                                    }

                                    let (event_type, payload) = event.to_ws_event();
                                    let payload = if let Some(obj) = payload.as_object() {
                                        let mut map = obj.clone();
                                        map.insert("conversation_id".to_string(), serde_json::json!(conversation_id.to_string()));
                                        serde_json::Value::Object(map)
                                    } else {
                                        serde_json::json!({"conversation_id": conversation_id.to_string()})
                                    };
                                    let is_done = matches!(event, ProviderEvent::Done { .. });

                                    // Pre-mark the linked agent run as failed the instant a
                                    // provider/model error is seen. The completion handler is
                                    // database-driven (status != running → no chain), so marking
                                    // `failed` here halts the runaway loop on the first genuinely
                                    // failed turn instead of burning the whole iteration cap.
                                    // Do NOT break on error — let the stream run to `Done`; the
                                    // error event is still broadcast below and the transcript is
                                    // preserved. Mirrors `failLinkedAgentRunIfRunning` in the
                                    // reference implementation.
                                    let is_error = matches!(event, ProviderEvent::Error { .. });
                                    if is_error {
                                        let _ = crate::services::tasks::fail_linked_agent_run(
                                            &db, &conversation_id,
                                        )
                                        .await;
                                    }

                                    let rendered = crate::webapp::components::message_stream::render_event(&event);
                                    ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type, timestamp: chrono::Utc::now(), payload, html: if rendered.is_empty() { None } else { Some(rendered) } }).await;

                                    // A question pauses the agent waiting on user input — signal the
                                    // global status feed so the dropdown surfaces the open question.
                                    if matches!(event, ProviderEvent::QuestionAsked { .. }) {
                                        broadcast_agent_status(&ws_bus, "question").await;
                                    }

                                    if is_done {
                                        completed_normally.store(true, Ordering::SeqCst);
                                        let done_now = chrono::Utc::now().naive_utc().to_string();
                                        let _ = db.execute("UPDATE conversations SET updated_at = $1 WHERE id = $2", hiqlite::params!(&done_now, conversation_id.to_string())).await;
                                        match completion_handler(&db, conversation_id, &active_sessions, &ws_bus).await {
                                            Ok(NextAction::StartAgent(agent_type)) => {
                                                let db = db.clone();
                                                let config_root = config_root.clone();
                                                let footprint = footprint.clone();
                                                let archive_root = archive_root.clone();
                                                let active_sessions = active_sessions.clone();
                                                let ws_bus = ws_bus.clone();
                                                let config = config.clone();
                                                tokio::spawn(async move {
                                                    if let Ok(task) = tasks::get_task(&db, task_id).await {
                                                        if let Err(e) = start_next_agent(&db, &task, agent_type, &config_root, &footprint, &archive_root, &active_sessions, &ws_bus, &config).await {
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
                        ws_bus.broadcast(&topic, ServerMessage::Event { topic: topic.clone(), event_type: "error".to_string(), timestamp: chrono::Utc::now(), payload: serde_json::json!({"error": "Agent session ended unexpectedly. Send a message to resume.", "conversation_id": conversation_id.to_string()}), html: Some(crate::webapp::components::message_stream::render_event(&error_event)) }).await;
                        broadcast_agent_status(&ws_bus, "failed").await;
                    }
                });
            }
            Err(e) => {
                tracing::warn!("Failed to start turn: {e}");
                active_sessions.lock().await.insert(conv_id_str, provider);
                // The run was created as `running` by start_session but no turn
                // ever began — mark it failed so it leaves the active-agent set.
                let _ = db.execute(
                    "UPDATE task_agent_runs SET status = 'failed', completed_at = $2 WHERE conversation_id = $1 AND status = 'running'",
                    hiqlite::params!(session_result.conversation_id.to_string(), chrono::Utc::now().naive_utc().to_string()),
                )
                .await;
                broadcast_agent_status(ws_bus, "failed").await;
            }
        }

        let run = tasks::get_agent_run_by_conversation(db, &session_result.conversation_id)
            .await
            .map_err(internal_err)?;
        Ok(run)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::schema::AgentType;
    use crate::server::ws::bus::BroadcastBus;
    use crate::services::session;
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

    async fn seed_agent_config(client: &hiqlite::Client, agent_type: &str) {
        let now = chrono::Utc::now().naive_utc().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                hiqlite::params!(
                    uuid::Uuid::new_v4().to_string(),
                    agent_type,
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
        seed_agent_config(&client, "review").await;

        let sessions = empty_sessions();
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        let run = tasks::get_agent_run_by_conversation(&client, &result.conversation_id)
            .await
            .unwrap();
        assert_eq!(run.status, RunStatus::Completed);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[tokio::test]
    async fn test_completion_handler_review_ready_routes_to_refinement() {
        let (client, task_id, _tmp) = make_client().await;

        // Seed refinement + implementation configs so the phase-skip check passes
        seed_agent_config(&client, "refinement").await;
        seed_agent_config(&client, "implementation").await;

        let result =
            session::start_session(&client, task_id, "model", "balanced", AgentType::Review)
                .await
                .unwrap();

        // Persist a model message containing the READY keyword
        crate::services::transcript::persist_event(
            &client,
            &ProviderEvent::Text {
                text: "All checks pass. READY to proceed.".into(),
                timestamp: chrono::Utc::now().naive_utc(),
            },
            &result.session_id,
            task_id,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Refinement)
        ));
    }

    #[tokio::test]
    async fn test_completion_handler_review_without_ready_loops_to_implementation() {
        let (client, task_id, _tmp) = make_client().await;

        // Seed implementation + refinement configs so the phase-skip check passes
        seed_agent_config(&client, "implementation").await;
        seed_agent_config(&client, "refinement").await;

        let result =
            session::start_session(&client, task_id, "model", "balanced", AgentType::Review)
                .await
                .unwrap();

        // Persist a model message WITHOUT the READY keyword
        crate::services::transcript::persist_event(
            &client,
            &ProviderEvent::Text {
                text: "Needs more work. ready to continue.".into(),
                timestamp: chrono::Utc::now().naive_utc(),
            },
            &result.session_id,
            task_id,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Implementation)
        ));
    }

    #[tokio::test]
    async fn test_completion_handler_non_review_agent_ignores_ready() {
        let (client, task_id, _tmp) = make_client().await;

        // Seed a review config so implementation chains to review
        seed_agent_config(&client, "review").await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        // Even though the last model message contains READY, a non-Review agent
        // must not be diverted into the finish pipeline.
        crate::services::transcript::persist_event(
            &client,
            &ProviderEvent::Text {
                text: "Implementation output says READY but routing ignores it.".into(),
                timestamp: chrono::Utc::now().naive_utc(),
            },
            &result.session_id,
            task_id,
        )
        .await
        .unwrap();

        let sessions = empty_sessions();
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

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
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Terminal));
    }

    #[tokio::test]
    async fn test_fail_linked_agent_run_halts_chaining() {
        let (client, task_id, _tmp) = make_client().await;

        // Seed a review config so a healthy run would chain to refinement/PR —
        // the failure pre-mark must make the handler Terminal instead.
        seed_agent_config(&client, "refinement").await;

        let result = session::start_session(
            &client,
            task_id,
            "model",
            "balanced",
            AgentType::Implementation,
        )
        .await
        .unwrap();

        let updated = tasks::fail_linked_agent_run(&client, &result.conversation_id)
            .await
            .unwrap();
        assert!(updated);

        let sessions = empty_sessions();
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        assert!(
            matches!(action, NextAction::Terminal),
            "a pre-marked failed run must not chain"
        );
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
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
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
        let ws_bus = BroadcastBus::new();
        let action = completion_handler(&client, result.conversation_id, &sessions, &ws_bus)
            .await
            .unwrap();

        assert!(matches!(action, NextAction::Stop));

        let task = tasks::get_task(&client, task_id).await.unwrap();
        assert!(task.workflow_blocked);
    }

    #[tokio::test]
    async fn test_orchestration_broadcast_includes_conversation_id() {
        let ws_bus = BroadcastBus::new();
        let task_id: i64 = 42;
        let topic = WsTopic {
            kind: WsTopicKind::Task,
            id: TopicId(task_id),
        };
        let conversation_id = uuid::Uuid::new_v4();

        let mut rx = ws_bus.subscribe(&topic).await;

        let (event_type, payload) = crate::providers::types::ProviderEvent::Error {
            error: "test error".into(),
            timestamp: chrono::Utc::now().naive_utc(),
        }
        .to_ws_event();

        let payload = if let Some(obj) = payload.as_object() {
            let mut map = obj.clone();
            map.insert(
                "conversation_id".to_string(),
                serde_json::json!(conversation_id.to_string()),
            );
            serde_json::Value::Object(map)
        } else {
            serde_json::json!({"conversation_id": conversation_id.to_string()})
        };

        ws_bus
            .broadcast(
                &topic,
                ServerMessage::Event {
                    topic: topic.clone(),
                    event_type,
                    timestamp: chrono::Utc::now(),
                    payload: payload.clone(),
                    html: None,
                },
            )
            .await;

        let received = rx.recv().await.unwrap();
        match &*received {
            ServerMessage::Event { payload: p, .. } => {
                assert_eq!(
                    p.get("conversation_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    conversation_id.to_string(),
                    "broadcast payload must include conversation_id"
                );
            }
            _ => panic!("expected ServerMessage::Event"),
        }
    }
}
