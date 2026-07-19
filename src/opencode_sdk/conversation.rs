use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::Stream;
use tokio::sync::Mutex;

use crate::opencode_sdk::client::EventStream;
use crate::opencode_sdk::types::{Event, GlobalEvent, Part, PartInput, TextPartInput};
use crate::opencode_sdk::{OpenCodeServer, OpencodeClient, SdkError, ServerOptions};

// ── Phase config ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PhaseConfig {
    pub model: String,
    pub agent: String,
    pub system_prompt: Option<String>,
    pub tools: Option<serde_json::Value>,
    pub cwd: Option<String>,
}

// ── One-shot config ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OneShotConfig {
    pub model: String,
    pub agent: Option<String>,
    pub system: Option<String>,
    pub cwd: Option<String>,
}

impl Default for OneShotConfig {
    fn default() -> Self {
        Self {
            model: "default".into(),
            agent: None,
            system: None,
            cwd: None,
        }
    }
}

// ── PhaseConversation ─────────────────────────────────────────────────────

pub struct PhaseConversation {
    server: Arc<Mutex<Option<OpenCodeServer>>>,
    client: OpencodeClient,
    session_id: String,
    is_closed: bool,
}

impl PhaseConversation {
    pub async fn start(server_opts: ServerOptions, config: &PhaseConfig) -> Result<Self, SdkError> {
        let server = crate::opencode_sdk::server::create_opencode_server(server_opts).await?;
        let password = server.password().map(|s| s.to_string());
        let client = OpencodeClient::new(&server.url(), password.as_deref());

        let title = format!("phase-{}", config.agent);
        let session = client.session.create(&title).await?;

        Ok(Self {
            server: Arc::new(Mutex::new(Some(server))),
            client,
            session_id: session.id,
            is_closed: false,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn client(&self) -> &OpencodeClient {
        &self.client
    }

    pub async fn run_phase(
        &self,
        prompt: &str,
        _phase_name: &str,
    ) -> Result<PhaseEventStream, SdkError> {
        let model_ref = crate::opencode_sdk::types::ModelRef {
            provider_id: "default".into(),
            model_id: "default".into(),
        };

        let body = crate::opencode_sdk::types::PromptBody {
            message_id: None,
            model: Some(model_ref),
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: prompt.to_string(),
            })],
        };

        self.client
            .session
            .prompt_async(&self.session_id, &body)
            .await?;

        let event_stream = self.client.event.subscribe().await?;
        Ok(PhaseEventStream {
            inner: event_stream,
            session_id: self.session_id.clone(),
            done: false,
        })
    }

    pub async fn abort(&self) -> Result<(), SdkError> {
        self.client.session.abort(&self.session_id).await?;
        Ok(())
    }

    pub async fn close(mut self) -> Result<(), SdkError> {
        if self.is_closed {
            return Ok(());
        }
        self.is_closed = true;

        let _ = self.client.session.abort(&self.session_id).await;

        let mut guard = self.server.lock().await;
        if let Some(mut server) = guard.take() {
            server.shutdown().await?;
        }
        Ok(())
    }
}

// ── One-shot ──────────────────────────────────────────────────────────────

pub async fn one_shot(
    client: &OpencodeClient,
    prompt: &str,
    config: &OneShotConfig,
) -> Result<String, SdkError> {
    let session = client.session.create("one-shot").await?;

    let model_ref = crate::opencode_sdk::types::ModelRef {
        provider_id: "default".into(),
        model_id: config.model.clone(),
    };

    let body = crate::opencode_sdk::types::PromptBody {
        message_id: None,
        model: Some(model_ref),
        agent: config.agent.clone(),
        no_reply: None,
        system: config.system.clone(),
        tools: None,
        parts: vec![PartInput::Text(TextPartInput {
            text: prompt.to_string(),
        })],
    };

    let response = client.session.prompt(&session.id, &body).await?;

    let text: String = response
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    let _ = client.session.delete(&session.id).await;

    Ok(text)
}

// ── UnstructuredConversation ──────────────────────────────────────────────

pub struct UnstructuredConversation {
    client: OpencodeClient,
    session_id: String,
}

impl UnstructuredConversation {
    pub async fn start(client: &OpencodeClient) -> Result<Self, SdkError> {
        let conversation_client = client.clone();
        let session = conversation_client.session.create("unstructured").await?;
        Ok(Self {
            client: conversation_client,
            session_id: session.id,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn client(&self) -> &OpencodeClient {
        &self.client
    }

    pub async fn send_message(&self, text: &str) -> Result<EventStream, SdkError> {
        let body = crate::opencode_sdk::types::PromptBody {
            message_id: None,
            model: Some(crate::opencode_sdk::types::ModelRef {
                provider_id: "default".into(),
                model_id: "default".into(),
            }),
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: text.to_string(),
            })],
        };

        self.client
            .session
            .prompt_async(&self.session_id, &body)
            .await?;
        self.client.event.subscribe().await
    }

    pub async fn messages(&self) -> Result<Vec<crate::opencode_sdk::types::Message>, SdkError> {
        self.client.session.messages(&self.session_id).await
    }

    pub async fn abort(&self) -> Result<(), SdkError> {
        self.client.session.abort(&self.session_id).await?;
        Ok(())
    }
}

// ── PhaseEventStream ──────────────────────────────────────────────────────

pub struct PhaseEventStream {
    inner: EventStream,
    session_id: String,
    done: bool,
}

impl PhaseEventStream {
    fn matches_session(global: &GlobalEvent, session_id: &str) -> bool {
        match &global.payload {
            Event::SessionIdle(data) => data.session_id == session_id,
            Event::SessionStatus(data) => data.session_id == session_id,
            Event::SessionError(data) => data.session_id == session_id,
            Event::MessageUpdated(data) => data.info.session_id == session_id,
            _ => true,
        }
    }

    fn is_terminal(global: &GlobalEvent, session_id: &str) -> bool {
        match &global.payload {
            Event::SessionIdle(data) => data.session_id == session_id,
            Event::SessionError(data) => data.session_id == session_id,
            _ => false,
        }
    }
}

impl Stream for PhaseEventStream {
    type Item = Result<GlobalEvent, SdkError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        loop {
            let pinned = Pin::new(&mut self.inner);
            match pinned.poll_next(cx) {
                Poll::Ready(Some(Ok(global))) => {
                    if !Self::matches_session(&global, &self.session_id) {
                        continue;
                    }
                    if Self::is_terminal(&global, &self.session_id) {
                        self.done = true;
                        return Poll::Ready(Some(Ok(global)));
                    }
                    return Poll::Ready(Some(Ok(global)));
                }
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode_sdk::types::*;

    #[test]
    fn test_phase_event_stream_matches_session() {
        let global = GlobalEvent {
            directory: "/tmp".into(),
            payload: Event::SessionIdle(SessionIdData {
                session_id: "sess1".into(),
            }),
        };
        assert!(PhaseEventStream::matches_session(&global, "sess1"));
        assert!(!PhaseEventStream::matches_session(&global, "sess2"));
    }

    #[test]
    fn test_phase_event_stream_is_terminal() {
        let idle = GlobalEvent {
            directory: "/tmp".into(),
            payload: Event::SessionIdle(SessionIdData {
                session_id: "sess1".into(),
            }),
        };
        assert!(PhaseEventStream::is_terminal(&idle, "sess1"));
        assert!(!PhaseEventStream::is_terminal(&idle, "sess2"));

        let error = GlobalEvent {
            directory: "/tmp".into(),
            payload: Event::SessionError(SessionErrorData {
                session_id: "sess1".into(),
                error: "err".into(),
            }),
        };
        assert!(PhaseEventStream::is_terminal(&error, "sess1"));

        let status = GlobalEvent {
            directory: "/tmp".into(),
            payload: Event::SessionStatus(SessionStatusData {
                session_id: "sess1".into(),
                status: SessionStatusValue {
                    status_type: "idle".into(),
                },
            }),
        };
        assert!(!PhaseEventStream::is_terminal(&status, "sess1"));
    }

    #[test]
    fn test_one_shot_config_default() {
        let config = OneShotConfig::default();
        assert_eq!(config.model, "default");
        assert!(config.agent.is_none());
    }

    #[test]
    fn test_phase_config_construction() {
        let config = PhaseConfig {
            model: "gpt-4".into(),
            agent: "coder".into(),
            system_prompt: Some("Be helpful".into()),
            tools: None,
            cwd: Some("/tmp".into()),
        };
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.agent, "coder");
    }
}
