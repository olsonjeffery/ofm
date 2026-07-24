use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use futures_util::Stream;

use crate::opencode_sdk::types::{GlobalEvent, PromptBody, PromptResponse, Provider, Session};
use crate::opencode_sdk::SdkError;

// ── Auth helper ───────────────────────────────────────────────────────────

fn basic_auth_header(password: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
    format!("Basic {encoded}")
}

// ── Inner client ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct InnerClient {
    base_url: String,
    password: String,
    http_client: reqwest::Client,
    directory: Option<String>,
}

impl InnerClient {
    fn auth_header(&self) -> String {
        basic_auth_header(&self.password)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn session_url(&self, path: &str) -> String {
        self.url(&format!("/session{path}"))
    }
}

// ── OpencodeClient ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OpencodeClient {
    inner: std::sync::Arc<InnerClient>,
    pub session: SessionApi,
    pub event: EventApi,
    pub config: ConfigApi,
    pub log_data: bool,
}

impl OpencodeClient {
    pub fn new(base_url: &str, password: Option<&str>, log_data: bool) -> Self {
        let inner = std::sync::Arc::new(InnerClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            password: password.unwrap_or("").to_string(),
            http_client: reqwest::Client::new(),
            directory: None,
        });
        Self {
            inner: inner.clone(),
            session: SessionApi(inner.clone()),
            event: EventApi(inner.clone(), log_data),
            config: ConfigApi(inner),
            log_data,
        }
    }

    pub fn with_directory(mut self, directory: &str) -> Self {
        let inner_ref = std::sync::Arc::make_mut(&mut self.inner);
        inner_ref.directory = Some(directory.to_string());
        self.session = SessionApi(self.inner.clone());
        self.event = EventApi(self.inner.clone(), self.log_data);
        self.config = ConfigApi(self.inner.clone());
        self
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    pub fn password(&self) -> Option<&str> {
        if self.inner.password.is_empty() {
            None
        } else {
            Some(&self.inner.password)
        }
    }
}

// ── SessionApi ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SessionApi(std::sync::Arc<InnerClient>);

impl SessionApi {
    pub async fn create(&self, title: &str) -> Result<Session, SdkError> {
        let mut req = self
            .0
            .http_client
            .post(self.0.session_url(""))
            .header("Authorization", self.0.auth_header())
            .json(&serde_json::json!({"title": title}));
        if let Some(dir) = &self.0.directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await.map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "POST /session failed: {status} — {body}"
            )));
        }
        resp.json().await.map_err(SdkError::Http)
    }

    pub async fn get(&self, id: &str) -> Result<Session, SdkError> {
        let resp = self
            .0
            .http_client
            .get(self.0.session_url(&format!("/{id}")))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "GET /session/{id} failed: {status} — {body}"
            )));
        }
        resp.json().await.map_err(SdkError::Http)
    }

    pub async fn list(&self) -> Result<Vec<Session>, SdkError> {
        let mut req = self
            .0
            .http_client
            .get(self.0.session_url(""))
            .header("Authorization", self.0.auth_header());
        if let Some(dir) = &self.0.directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await.map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "GET /session failed: {status} — {body}"
            )));
        }
        resp.json().await.map_err(SdkError::Http)
    }

    pub async fn delete(&self, id: &str) -> Result<(), SdkError> {
        let resp = self
            .0
            .http_client
            .delete(self.0.session_url(&format!("/{id}")))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "DELETE /session/{id} failed: {status} — {body}"
            )));
        }
        Ok(())
    }

    pub async fn prompt(&self, id: &str, body: &PromptBody) -> Result<PromptResponse, SdkError> {
        let resp = self
            .0
            .http_client
            .post(self.0.session_url(&format!("/{id}/message")))
            .header("Authorization", self.0.auth_header())
            .json(body)
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "POST /session/{id}/message failed: {status} — {body_text}"
            )));
        }
        resp.json().await.map_err(SdkError::Http)
    }

    pub async fn prompt_async(&self, id: &str, body: &PromptBody) -> Result<(), SdkError> {
        let resp = self
            .0
            .http_client
            .post(self.0.session_url(&format!("/{id}/prompt_async")))
            .header("Authorization", self.0.auth_header())
            .json(body)
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "POST /session/{id}/prompt_async failed: {status} — {body_text}"
            )));
        }
        Ok(())
    }

    pub async fn abort(&self, id: &str) -> Result<bool, SdkError> {
        let resp = self
            .0
            .http_client
            .post(self.0.session_url(&format!("/{id}/abort")))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        Ok(resp.status().is_success())
    }

    pub async fn messages(
        &self,
        id: &str,
    ) -> Result<Vec<crate::opencode_sdk::types::Message>, SdkError> {
        let resp = self
            .0
            .http_client
            .get(self.0.session_url(&format!("/{id}/message")))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "GET /session/{id}/message failed: {status} — {body}"
            )));
        }
        resp.json().await.map_err(SdkError::Http)
    }
}

// ── EventApi ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EventApi(std::sync::Arc<InnerClient>, bool);

impl EventApi {
    pub async fn subscribe(&self) -> Result<EventStream, SdkError> {
        let resp = self
            .0
            .http_client
            .get(self.0.url("/event"))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "GET /event failed: {status} — {body}"
            )));
        }
        Ok(EventStream {
            stream: Some(Box::pin(resp.bytes_stream())),
            buf: Vec::new(),
            pending: Vec::new(),
            retry_delay: Duration::from_millis(3000),
            max_retries: 3,
            retry_count: 0,
            sleep: None,
            reconnect: Some(EventStreamReconnect {
                http_client: self.0.http_client.clone(),
                url: self.0.url("/event"),
                auth_header: self.0.auth_header(),
            }),
            reconnect_fut: None,
            cancellation: Arc::new(AtomicBool::new(false)),
            log_data: self.1,
        })
    }
}

// ── EventStream ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct EventStreamReconnect {
    http_client: reqwest::Client,
    url: String,
    auth_header: String,
}

/// A cancellation handle for an [`EventStream`].
/// Call `cancel()` to signal the stream to stop.
#[derive(Clone)]
pub struct EventStreamCancellation {
    inner: Arc<AtomicBool>,
}

impl EventStreamCancellation {
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

type SseByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;
type ReconnectFut = Pin<Box<dyn Future<Output = Result<reqwest::Response, reqwest::Error>> + Send>>;

pub struct EventStream {
    stream: Option<SseByteStream>,
    buf: Vec<u8>,
    pending: Vec<GlobalEvent>,
    retry_delay: Duration,
    max_retries: u32,
    retry_count: u32,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    reconnect: Option<EventStreamReconnect>,
    reconnect_fut: Option<ReconnectFut>,
    cancellation: Arc<AtomicBool>,
    log_data: bool,
}

impl EventStream {
    pub fn cancellation_handle(&self) -> EventStreamCancellation {
        EventStreamCancellation {
            inner: self.cancellation.clone(),
        }
    }

    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::SeqCst);
    }
}

fn parse_sse_lines(buf: &mut Vec<u8>, log_data: bool) -> (Vec<GlobalEvent>, Option<Duration>) {
    let mut events = Vec::new();
    let mut retry = None;
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
        let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len().saturating_sub(1)]);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        if log_data {
            tracing::info!("data received: {}", trimmed);
        }
        if let Some(data) = trimmed.strip_prefix("data: ") {
            match serde_json::from_str::<GlobalEvent>(data) {
                Ok(global) => events.push(global),
                Err(e) => {
                    let truncated: String = data.chars().take(1024).collect();
                    tracing::warn!(
                        error = %e,
                        raw_data = %truncated,
                        "Failed to parse SSE data as GlobalEvent"
                    );
                }
            }
        } else if let Some(val) = trimmed.strip_prefix("retry: ") {
            if let Ok(ms) = val.trim().parse::<u64>() {
                retry = Some(Duration::from_millis(ms));
            }
        }
    }
    (events, retry)
}

impl Stream for EventStream {
    type Item = Result<GlobalEvent, SdkError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check cancellation
        if self.cancellation.load(Ordering::SeqCst) {
            return Poll::Ready(None);
        }

        // Drain pending queue
        if let Some(event) = self.pending.pop() {
            return Poll::Ready(Some(Ok(event)));
        }

        loop {
            // Parse buffered data for SSE events (including retry field)
            let log_data = self.log_data;
            let (mut new_events, retry) = parse_sse_lines(&mut self.buf, log_data);
            if let Some(delay) = retry {
                self.retry_delay = delay;
            }
            if !new_events.is_empty() {
                let first = new_events.remove(0);
                new_events.reverse();
                self.pending = new_events;
                return Poll::Ready(Some(Ok(first)));
            }

            // Check if we're currently in a sleep/delay between reconnection attempts
            if let Some(sleep) = self.sleep.as_mut() {
                match sleep.as_mut().poll(cx) {
                    Poll::Ready(()) => {
                        self.sleep = None;
                        // Kick off a reconnect request future (if we have a
                        // reconnect config). The future is polled below on
                        // subsequent invocations of poll_next. We avoid
                        // Handle::block_on here because it panics when called
                        // from within the async runtime that owns this stream.
                        if self.reconnect.is_some() && self.reconnect_fut.is_none() {
                            let reconnect = self.reconnect.clone().unwrap();
                            self.reconnect_fut = Some(Box::pin(async move {
                                reconnect
                                    .http_client
                                    .get(&reconnect.url)
                                    .header("Authorization", &reconnect.auth_header)
                                    .send()
                                    .await
                            }));
                        }
                        // Fall through to the reconnect_fut handling below.
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // Poll an in-flight reconnect request, if any.
            if let Some(reconnect_fut) = self.reconnect_fut.as_mut() {
                match reconnect_fut.as_mut().poll(cx) {
                    Poll::Ready(Ok(resp)) => {
                        self.reconnect_fut = None;
                        if resp.status().is_success() {
                            self.stream = Some(Box::pin(resp.bytes_stream()));
                            continue;
                        } else {
                            let status = resp.status();
                            return Poll::Ready(Some(Err(SdkError::Protocol(format!(
                                "reconnect failed with status {status}"
                            )))));
                        }
                    }
                    Poll::Ready(Err(e)) => {
                        self.reconnect_fut = None;
                        if e.is_timeout() || e.is_connect() {
                            if self.cancellation.load(Ordering::SeqCst) {
                                return Poll::Ready(None);
                            }
                            // Treat transient connect failure as a retryable
                            // timeout: schedule another sleep if retries are
                            // still available, otherwise end the stream.
                            if self.retry_count < self.max_retries {
                                self.retry_count += 1;
                                let delay = self.retry_delay;
                                self.sleep = Some(Box::pin(tokio::time::sleep(delay)));
                                cx.waker().wake_by_ref();
                                return Poll::Pending;
                            }
                            return Poll::Ready(None);
                        }
                        return Poll::Ready(Some(Err(SdkError::Http(e))));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // Check if stream is active
            match self.stream.as_mut() {
                Some(stream_pin) => match stream_pin.as_mut().poll_next(cx) {
                    Poll::Ready(Some(Ok(chunk))) => {
                        self.buf.extend_from_slice(&chunk);
                        continue;
                    }
                    Poll::Ready(Some(Err(e))) => {
                        tracing::warn!(
                            error = %e,
                            body = %String::from_utf8_lossy(&self.buf),
                            "Event stream transport error",
                        );
                        return Poll::Ready(Some(Err(SdkError::Http(e))));
                    }
                    Poll::Ready(None) => {
                        // Stream ended — attempt reconnection
                        self.stream = None;
                        if self.retry_count < self.max_retries {
                            self.retry_count += 1;
                            let delay = self.retry_delay;
                            let sleep = Box::pin(tokio::time::sleep(delay));
                            self.sleep = Some(sleep);
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        return Poll::Ready(None);
                    }
                    Poll::Pending => return Poll::Pending,
                },
                None => {
                    // No active stream — if we have a reconnect configured and retries left
                    if self.reconnect.is_some() && self.retry_count < self.max_retries {
                        self.retry_count += 1;
                        let delay = self.retry_delay;
                        let sleep = Box::pin(tokio::time::sleep(delay));
                        self.sleep = Some(sleep);
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                    return Poll::Ready(None);
                }
            }
        }
    }
}

// ── ConfigApi ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfigApi(std::sync::Arc<InnerClient>);

impl ConfigApi {
    pub async fn providers(&self) -> Result<Vec<Provider>, SdkError> {
        let resp = self
            .0
            .http_client
            .get(self.0.url("/config/providers"))
            .header("Authorization", self.0.auth_header())
            .send()
            .await
            .map_err(SdkError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SdkError::Protocol(format!(
                "GET /config/providers failed: {status} — {body}"
            )));
        }
        #[derive(serde::Deserialize)]
        struct ProvidersResponse {
            providers: Vec<Provider>,
        }
        let wrapped: ProvidersResponse = resp
            .json()
            .await
            .map_err(|e| SdkError::Protocol(format!("failed to parse providers response: {e}")))?;
        Ok(wrapped.providers)
    }
}

// ── Factory ───────────────────────────────────────────────────────────────

pub fn create_opencode_client(base_url: &str, password: Option<&str>) -> OpencodeClient {
    OpencodeClient::new(base_url, password, false)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencode_sdk::types::*;

    #[test]
    fn test_opencode_sdk_basic_auth_header() {
        let header = basic_auth_header("test-pw");
        assert!(header.starts_with("Basic "));
    }

    #[test]
    fn test_opencode_sdk_url_construction() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", Some("pw"), false);

        assert_eq!(client.base_url(), "http://127.0.0.1:3183");
    }

    #[test]
    fn test_opencode_sdk_url_trailing_slash_stripped() {
        let client = OpencodeClient::new("http://127.0.0.1:3183/", Some("pw"), false);
        assert_eq!(client.base_url(), "http://127.0.0.1:3183");
    }

    #[test]
    fn test_opencode_sdk_client_with_directory() {
        let client =
            OpencodeClient::new("http://127.0.0.1:3183", None, false).with_directory("/tmp");
        assert!(client.inner.directory.is_some());
    }

    #[test]
    fn test_opencode_sdk_prompt_body_serialization() {
        let body = PromptBody {
            message_id: None,
            model: Some(ModelRef {
                provider_id: "test".into(),
                model_id: "gpt-4".into(),
            }),
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: "Hello".into(),
            })],
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["parts"][0]["type"], "text");
        assert_eq!(json["parts"][0]["text"], "Hello");
        assert_eq!(json["model"]["providerID"], "test");
        assert_eq!(json["model"]["modelID"], "gpt-4");
    }

    #[test]
    fn test_opencode_sdk_prompt_body_with_message_id() {
        let body = PromptBody {
            message_id: Some("msg1".into()),
            model: None,
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: "Hello".into(),
            })],
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["messageID"], "msg1");
    }

    #[test]
    fn test_opencode_sdk_session_list_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("");
        assert_eq!(url, "http://127.0.0.1:3183/session");
    }

    #[test]
    fn test_opencode_sdk_session_get_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("/sess1");
        assert_eq!(url, "http://127.0.0.1:3183/session/sess1");
    }

    #[test]
    fn test_opencode_sdk_session_prompt_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("/sess1/message");
        assert_eq!(url, "http://127.0.0.1:3183/session/sess1/message");
    }

    #[test]
    fn test_opencode_sdk_session_prompt_async_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("/sess1/prompt_async");
        assert_eq!(url, "http://127.0.0.1:3183/session/sess1/prompt_async");
    }

    #[test]
    fn test_opencode_sdk_session_abort_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("/sess1/abort");
        assert_eq!(url, "http://127.0.0.1:3183/session/sess1/abort");
    }

    #[test]
    fn test_opencode_sdk_session_messages_url() {
        let client = OpencodeClient::new("http://127.0.0.1:3183", None, false);
        let url = client.session.0.session_url("/sess1/message");
        assert_eq!(url, "http://127.0.0.1:3183/session/sess1/message");
    }

    #[test]
    fn test_opencode_sdk_event_url() {
        let inner = InnerClient {
            base_url: "http://127.0.0.1:3183".into(),
            password: String::new(),
            http_client: reqwest::Client::new(),
            directory: None,
        };
        let url = inner.url("/event");
        assert_eq!(url, "http://127.0.0.1:3183/event");
    }

    #[test]
    fn test_opencode_sdk_config_providers_url() {
        let inner = InnerClient {
            base_url: "http://127.0.0.1:3183".into(),
            password: String::new(),
            http_client: reqwest::Client::new(),
            directory: None,
        };
        let url = inner.url("/config/providers");
        assert_eq!(url, "http://127.0.0.1:3183/config/providers");
    }

    #[test]
    fn test_opencode_sdk_drain_sse_lines() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let body = "data: {\"id\":\"evt_1\",\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"s1\"}}\n\ndata: {\"id\":\"evt_2\",\"type\":\"session.status\",\"properties\":{\"sessionID\":\"s1\",\"status\":{\"type\":\"idle\"}}}\n";
            let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = server.local_addr().unwrap().port();
            let _ = tx.send(port);
            let _serve = tokio::spawn(async move {
                let (stream, _) = server.accept().await.unwrap();
                let (mut reader, mut writer) = stream.into_split();
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 4096];
                let n = reader.read(&mut buf).await.unwrap();
                let _req = std::str::from_utf8(&buf[..n]).unwrap().to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n{body}"
                );
                writer.write_all(resp.as_bytes()).await.unwrap();
            });

            let port = rx.await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let client = reqwest::Client::new();
            let resp = client
                .get(&format!("http://127.0.0.1:{port}/event"))
                .send()
                .await
                .unwrap();

            use std::sync::Arc;
            use std::sync::atomic::AtomicBool;
            let mut stream = EventStream {
                stream: Some(Box::pin(resp.bytes_stream())),
                buf: Vec::new(),
                pending: Vec::new(),
                retry_delay: Duration::from_millis(3000),
                max_retries: 3,
                retry_count: 0,
                sleep: None,
                reconnect: None,
                reconnect_fut: None,
                cancellation: Arc::new(AtomicBool::new(false)),
                log_data: false
            };

            let mut count = 0;
            use futures_util::StreamExt;
            while let Some(event) = stream.next().await {
                let event = event.unwrap();
                count += 1;
                match &event.payload {
                    Event::SessionIdle(_) => assert!(event.id.is_some()),
                    Event::SessionStatus(_) => assert!(event.id.is_some()),
                    _ => {}
                }
                if count == 2 {
                    break;
                }
            }
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn test_opencode_sdk_sse_comment_line_ignored() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b":comment\ndata: {\"id\":\"evt_1\",\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"s1\"}}\n");
        let (events, _) = parse_sse_lines(&mut buf, false);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].payload, Event::SessionIdle(_)));
    }

    #[test]
    fn test_opencode_sdk_sse_invalid_json_skipped() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"data: not-json\n");
        let (events, _) = parse_sse_lines(&mut buf, false);
        assert!(events.is_empty());
    }

    #[test]
    fn test_opencode_sdk_sse_empty_data_ignored() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"data: \n");
        let (events, _) = parse_sse_lines(&mut buf, false);
        assert!(events.is_empty());
    }

    #[test]
    fn test_opencode_sdk_sse_retry_field_parsed() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"retry: 5000\ndata: {\"id\":\"evt_1\",\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"s1\"}}\n");
        let (events, retry) = parse_sse_lines(&mut buf, false);
        assert_eq!(events.len(), 1);
        assert_eq!(retry, Some(Duration::from_millis(5000)));
    }

    #[test]
    fn test_event_stream_error_logs_body() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            use std::io::Write as IoWrite;
            use std::sync::Mutex;

            // Set up a tracing subscriber that captures log output
            let captured = Arc::new(Mutex::new(Vec::<u8>::new()));

            struct TestWriter {
                buf: Arc<Mutex<Vec<u8>>>,
            }
            impl IoWrite for TestWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    self.buf.lock().unwrap().write(buf)
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    self.buf.lock().unwrap().flush()
                }
            }

            let subscriber = tracing_subscriber::fmt()
                .with_writer({
                    let captured = captured.clone();
                    move || TestWriter {
                        buf: captured.clone(),
                    }
                })
                .finish();
            let _guard = tracing::subscriber::set_default(subscriber);

            // Get a real reqwest error
            let err = reqwest::get("http://127.0.0.1:1/").await.unwrap_err();

            // Build a stream that yields partial data (no newline, so it stays in
            // buf after parse_sse_lines) and then errors out.
            let partial = b"incomplete chunked data here";
            let stream =
                futures_util::stream::iter(vec![Ok(Bytes::from_static(partial)), Err(err)]);

            use futures_util::StreamExt;
            let mut event_stream = EventStream {
                stream: Some(Box::pin(stream)),
                buf: Vec::new(),
                pending: Vec::new(),
                retry_delay: Duration::from_millis(3000),
                max_retries: 0,
                retry_count: 0,
                sleep: None,
                reconnect: None,
                reconnect_fut: None,
                cancellation: Arc::new(AtomicBool::new(false)),
                log_data: false,
            };

            let result = event_stream.next().await;
            assert!(result.is_some());
            assert!(result.unwrap().is_err(), "expected error from stream");

            // Verify the body appears in the trace log
            let binding = captured.lock().unwrap();
            let log_output = String::from_utf8_lossy(&binding);
            assert!(
                log_output.contains("incomplete chunked data here"),
                "buffered body should appear in trace log: {log_output:?}"
            );
        });
    }
}
