use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;
use tokio::sync::Notify;

use crate::opencode_sdk::client::EventStreamCancellation;
use crate::opencode_sdk::types::{ModelRef, PartInput, PromptBody, TextPartInput};
use crate::opencode_sdk::{self, OpenCodeServer, OpencodeClient};
use crate::providers::types::{ProviderEvent, ResumeInput, TurnInput};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_MODEL: &str = "phi4-mini";
const PROVIDER_ID: &str = "openai-compatible";
const CONTAINER_PREFIX: &str = "ofm-ramalama-";

/// On-demand ramalama + phi4-mini provider.
///
/// Unlike `OpenCodeSdkProvider` (which shares one `opencode serve` subprocess
/// per user via the pool), the ramalama host serves a single model/session at
/// a time, so each provider instance owns exactly one `ramalama serve`
/// subprocess (spawned on a random port) plus a transient `opencode serve`
/// adapter that points at it via an on-the-fly OpenAI-compatible provider
/// config. Both subprocesses are torn down when the provider is shut down or
/// dropped.
pub struct RamalamaProvider {
    config: HarnessConfig,
    /// The model name served by ramalama (e.g. "phi4-mini").
    model_config: String,
    config_root: PathBuf,
    log_data: bool,
    /// The `ramalama serve` subprocess, owned by this provider.
    child: Mutex<Option<tokio::process::Child>>,
    port: Mutex<Option<u16>>,
    /// The docker/podman container name passed via `--name`, used to remove
    /// the container on teardown (killing the CLI alone orphans it).
    container_name: Mutex<Option<String>>,
    /// The model id reported by `/v1/models` (e.g. `library/phi4-mini`),
    /// which may differ from the user-entered short name (`phi4-mini`).
    served_model_id: Mutex<Option<String>>,
    /// Transient opencode client + server acting as the OpenAI-compatible
    /// adapter. Persist across turns within the conversation so `resume_turn`
    /// can reuse the opencode `session_id`.
    client: Mutex<Option<OpencodeClient>>,
    server: Mutex<Option<OpenCodeServer>>,
    session_id: Mutex<Option<String>>,
    /// Cancellation for the in-flight event stream reader task.
    event_cancellation: Mutex<Option<EventStreamCancellation>>,
    reader_cancellation: Mutex<Option<Arc<Notify>>>,
}

impl RamalamaProvider {
    pub async fn new(
        config: &HarnessConfig,
        config_root: &Path,
        log_data: bool,
    ) -> Result<Self, ProviderError> {
        let model_config = config
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Ok(Self {
            config: config.clone(),
            model_config,
            config_root: config_root.to_path_buf(),
            log_data,
            child: Mutex::new(None),
            port: Mutex::new(None),
            container_name: Mutex::new(None),
            served_model_id: Mutex::new(None),
            client: Mutex::new(None),
            server: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            reader_cancellation: Mutex::new(None),
        })
    }

    fn cancel_inflight(&self) {
        if let Some(notify) = self.reader_cancellation.lock().unwrap().take() {
            notify.notify_one();
        }
        if let Some(cancellation) = self.event_cancellation.lock().unwrap().take() {
            cancellation.cancel();
        }
    }

    /// Ensure the `ramalama serve` subprocess is running. Safe to call from
    /// `start()` and `one_shot_prompt()`; a no-op when already started.
    async fn ensure_started(&self) -> Result<(), ProviderError> {
        if self.port.lock().unwrap().is_some() {
            return Ok(());
        }

        let probe = std::process::Command::new("ramalama").output();
        if let Err(e) = &probe {
            if e.kind() == std::io::ErrorKind::NotFound {
                return Err(ProviderError::Config(
                    "ramalama binary not found in PATH — install ramalama or set OFM_RAMALAMA_PHI4_MINI_ENABLED=false".into(),
                ));
            }
        }

        let port = crate::rauthy::find_available_port().map_err(ProviderError::Io)?;
        let container_name = format!("{CONTAINER_PREFIX}{port}");
        let model_ref = self.resolve_model_ref();

        // Reap any stale container left by a SIGKILLed previous run sharing
        // this port-derived name, so `ramalama serve --name` cannot collide.
        self.remove_container(&container_name).await;

        // `--name` is the *container* name, so it must be unique per provider
        // instance (per-conversation). The model is passed as a positional
        // argument so ramalama serves exactly that one model (not the router
        // mode that would expose all local models under full slugs).
        let mut cmd = tokio::process::Command::new("ramalama");
        cmd.args([
            "serve",
            "--port",
            &port.to_string(),
            "--name",
            &container_name,
            &model_ref,
        ]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                tracing::error!("failed to spawn ramalama serve: {e}");
                return Err(ProviderError::Io(e));
            }
        };

        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, "ramalama");
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, "ramalama");
        }

        let health = self.wait_for_health(port, &mut child).await;
        let served_model_id = if health.is_ok() {
            self.resolve_served_model_id(port).await
        } else {
            None
        };

        if let Err(e) = health {
            // Never spawn-and-forget: on any error, kill + reap the CLI and
            // remove the named container it spawned before returning.
            tracing::warn!(
                port = port,
                container = %container_name,
                error = %e,
                "ramalama serve failed to start; killing subprocess and removing container"
            );
            let _ = child.kill().await;
            let _ = child.wait().await;
            self.remove_container(&container_name).await;
            return Err(e);
        }

        tracing::info!(
            port = port,
            model = %self.model_config,
            served_model_id = ?served_model_id,
            "RamaLama server started at http://127.0.0.1:{port}/v1"
        );
        *self.child.lock().unwrap() = Some(child);
        *self.port.lock().unwrap() = Some(port);
        *self.container_name.lock().unwrap() = Some(container_name);
        *self.served_model_id.lock().unwrap() = served_model_id;
        Ok(())
    }

    /// Resolve the user-entered model name to a model reference that
    /// `ramalama serve` accepts. Short names (e.g. `phi4-mini`) are mapped to
    /// the ollama library ref; already-qualified refs pass through unchanged.
    fn resolve_model_ref(&self) -> String {
        if self.model_config.contains("://") || self.model_config.starts_with("library/") {
            self.model_config.clone()
        } else {
            format!("ollama://library/{}:latest", self.model_config)
        }
    }

    /// Query `/v1/models` and resolve the model id the server actually serves
    /// (e.g. `library/phi4-mini`). The llama.cpp backend exposes the full ref
    /// slug rather than the user-entered short name.
    async fn resolve_served_model_id(&self, port: u16) -> Option<String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok()?;
        let resp = client
            .get(format!("http://127.0.0.1:{port}/v1/models"))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let json: serde_json::Value = resp.json().await.ok()?;
        let ids: Vec<String> = json["data"]
            .as_array()?
            .iter()
            .filter_map(|m| m["id"].as_str().map(String::from))
            .collect();
        // Prefer a model whose id matches the configured name; fall back to the
        // first served model.
        ids.iter()
            .find(|id| id.contains(&self.model_config))
            .or_else(|| ids.first())
            .cloned()
    }

    /// Remove the named ramalama container. Killing the `ramalama serve` CLI
    /// alone orphans the container, so teardown targets it by name (following
    /// the AGENTS.md "kill precisely by named container" rule). `docker rm -f`
    /// is synchronous and deterministic; `ramalama stop` (which also handles
    /// podman engines) is the fallback.
    async fn remove_container(&self, container_name: &str) {
        let docker_rm = tokio::process::Command::new("docker")
            .args(["rm", "-f", container_name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        if let Ok(status) = docker_rm {
            if status.success() {
                return;
            }
        }
        let _ = tokio::process::Command::new("ramalama")
            .args(["stop", container_name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }

    /// Poll the llama.cpp health endpoint until the server responds.
    ///
    /// ramalama 0.23's llama.cpp backend returns 404 for the Ollama-style
    /// `/api/tags`; `/health` and `/v1/models` both respond 200. Poll
    /// `/v1/models` (which also serves as the model-discovery endpoint) so the
    /// readiness signal matches what the OpenAI-compatible adapter will hit.
    /// Follows the `rauthy::wait_until_healthy()` pattern, with the added
    /// check that the subprocess has not exited prematurely.
    async fn wait_for_health(
        &self,
        port: u16,
        child: &mut tokio::process::Child,
    ) -> Result<(), ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;
        let start = std::time::Instant::now();

        loop {
            if let Some(status) = child.try_wait().map_err(ProviderError::Io)? {
                return Err(ProviderError::Config(format!(
                    "ramalama serve exited prematurely with status: {status}"
                )));
            }
            if start.elapsed() > HEALTH_TIMEOUT {
                return Err(ProviderError::Timeout);
            }

            match client
                .get(format!("http://127.0.0.1:{port}/v1/models"))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => tokio::time::sleep(HEALTH_POLL_INTERVAL).await,
            }
        }
    }

    /// Generate the on-the-fly OpenAI-compatible provider snippet pointing at
    /// the ramalama server.
    ///
    /// The provider must be declared as a custom `@ai-sdk/openai-compatible`
    /// provider (per opencode's docs) with `options.baseURL` and a `models`
    /// map keyed by the served model id — otherwise opencode cannot route the
    /// prompt and returns a 500.
    fn build_opencode_provider_snippet(&self) -> Result<String, ProviderError> {
        let port = self.port.lock().unwrap().ok_or(ProviderError::NotStarted)?;
        let served_model_id = self.prompt_model_id();
        let snippet = serde_json::json!({
            "provider": {
                PROVIDER_ID: {
                    "npm": "@ai-sdk/openai-compatible",
                    "name": "RamaLama (local)",
                    "options": {
                        "baseURL": format!("http://127.0.0.1:{port}/v1")
                    },
                    "models": {
                        &served_model_id: {
                            "name": self.model_config
                        }
                    }
                }
            }
        });
        serde_json::to_string_pretty(&snippet).map_err(|e| ProviderError::Config(e.to_string()))
    }

    /// Base opencode server config (permissions) merged with the provider
    /// snippet so `opencode serve` can reach the ramalama server.
    fn build_server_config(&self, snippet: &str) -> serde_json::Value {
        let mut base = serde_json::json!({
            "provider": {},
            "permission": {
                "edit": "allow",
                "bash": "allow",
                "webfetch": "allow",
                "doom_loop": "allow",
                "external_directory": "allow"
            }
        });
        if let Ok(snippet_val) = serde_json::from_str::<serde_json::Value>(snippet) {
            deep_merge(&mut base, &snippet_val);
        }
        base
    }

    async fn spawn_transient_server(
        &self,
    ) -> Result<(OpencodeClient, OpenCodeServer), ProviderError> {
        let snippet = self.build_opencode_provider_snippet()?;
        let server_config = self.build_server_config(&snippet);
        let options = opencode_sdk::ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, server) = opencode_sdk::create_opencode(options, self.log_data)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        Ok((client, server))
    }

    /// The model id to send in prompts: the served id reported by `/v1/models`
    /// when available (llama.cpp exposes full refs like `library/phi4-mini`),
    /// falling back to the user-entered name.
    fn prompt_model_id(&self) -> String {
        self.served_model_id
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.model_config.clone())
    }

    fn build_prompt_body(&self, prompt: &str) -> PromptBody {
        let model_id = self.prompt_model_id();
        PromptBody {
            message_id: None,
            model: Some(ModelRef {
                provider_id: PROVIDER_ID.into(),
                model_id,
            }),
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: prompt.to_string(),
            })],
        }
    }

    /// Subscribe to the opencode global event stream and spawn a reader task
    /// that maps SDK events to `ProviderEvent`s on the returned channel.
    async fn subscribe_and_spawn(
        &self,
        client: &OpencodeClient,
        session_id: &str,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        self.cancel_inflight();

        let reader_stop: Arc<Notify> = Arc::new(Notify::new());
        *self.reader_cancellation.lock().unwrap() = Some(reader_stop.clone());

        let event_stream = client
            .event
            .subscribe()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        let cancellation = event_stream.cancellation_handle();
        *self.event_cancellation.lock().unwrap() = Some(cancellation);

        let s_id = session_id.to_string();
        let (tx, rx) = mpsc::channel(1024);

        tx.send(ProviderEvent::SessionStart {
            session_id: s_id.clone(),
        })
        .await
        .map_err(|_| ProviderError::Protocol("channel closed".into()))?;

        tokio::spawn(async move {
            let mut stream = event_stream;
            'reader: loop {
                tokio::select! {
                    result = stream.next() => {
                        match result {
                            Some(Ok(global)) => {
                                for provider_event in
                                    crate::providers::opencode_sdk_provider::map_sdk_event_to_provider_event(&global, &s_id)
                                {
                                    if tx.send(provider_event).await.is_err() {
                                        break 'reader;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                let _ = tx
                                    .send(ProviderEvent::Error {
                                        error: e.to_string(),
                                        timestamp: chrono::Utc::now().naive_utc(),
                                    })
                                    .await;
                                break;
                            }
                            None => break,
                        }
                    }
                    _ = reader_stop.notified() => {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

fn spawn_reader(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static, label: &'static str) {
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!("[{label}] {line}");
        }
    });
}

fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

#[async_trait]
impl LlmProvider for RamalamaProvider {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec![self.model_config.clone()])
    }

    async fn start(&mut self, _working_dir: &Path) -> Result<(), ProviderError> {
        self.ensure_started().await
    }

    async fn start_turn(
        &self,
        input: TurnInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let (mut client, server) = self.spawn_transient_server().await?;
        if !input.cwd.is_empty() {
            client = client.with_directory(&input.cwd);
        }

        let session = client
            .session
            .create(&input.prompt)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        *self.session_id.lock().unwrap() = Some(session.id.clone());

        // Subscribe BEFORE issuing the prompt so we don't miss events that
        // fire immediately when the prompt is queued on the server.
        let rx = self.subscribe_and_spawn(&client, &session.id).await?;

        let body = self.build_prompt_body(&input.prompt);
        client
            .session
            .prompt_async(&session.id, &body)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        *self.client.lock().unwrap() = Some(client);
        *self.server.lock().unwrap() = Some(server);
        Ok(rx)
    }

    async fn resume_turn(
        &self,
        input: ResumeInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or(ProviderError::NotStarted)?;

        let session_id = input.session_id;
        *self.session_id.lock().unwrap() = Some(session_id.clone());

        let prompt = input
            .messages
            .as_array()
            .and_then(|msgs| msgs.last())
            .and_then(|last| {
                last.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| last.get("delta").and_then(|d| d.as_str()))
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "continue".to_string());

        // Subscribe BEFORE issuing the prompt_async so we don't miss events
        // that fire immediately when the prompt is queued on the server.
        let rx = self.subscribe_and_spawn(&client, &session_id).await?;

        let body = self.build_prompt_body(&prompt);
        client
            .session
            .prompt_async(&session_id, &body)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        Ok(rx)
    }

    async fn abort_turn(&self) -> Result<(), ProviderError> {
        self.cancel_inflight();
        let (session_id, client) = {
            let s = self.session_id.lock().unwrap().clone();
            let c = self.client.lock().unwrap().clone();
            (s, c)
        };
        if let (Some(client), Some(session_id)) = (client, session_id) {
            let _ = client.session.abort(&session_id).await;
        }
        Ok(())
    }

    async fn one_shot_prompt(&self, prompt: &str, _model: &str) -> Result<String, ProviderError> {
        self.ensure_started().await?;
        let (client, mut server) = self.spawn_transient_server().await?;

        let config = opencode_sdk::conversation::OneShotConfig {
            model: self.prompt_model_id(),
            provider_id: PROVIDER_ID.into(),
            ..Default::default()
        };

        let result = opencode_sdk::conversation::one_shot(&client, prompt, &config)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let _ = server.shutdown().await;
        Ok(result)
    }

    async fn shutdown(&mut self) -> Result<bool, ProviderError> {
        self.cancel_inflight();

        // Kill the ramalama serve subprocess and reap it. Take the child out
        // of the mutex first so the guard is dropped before the awaits keep
        // the future Send-safe.
        let child = self.child.lock().unwrap().take();
        if let Some(mut child) = child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        // Killing the CLI alone orphans the docker/podman container; remove it
        // precisely by the name we assigned.
        let container_name = self.container_name.lock().unwrap().take();
        if let Some(name) = container_name {
            self.remove_container(&name).await;
        }

        *self.port.lock().unwrap() = None;
        *self.served_model_id.lock().unwrap() = None;

        // Dropping the transient opencode server kills its subprocess.
        *self.server.lock().unwrap() = None;
        *self.client.lock().unwrap() = None;
        *self.session_id.lock().unwrap() = None;
        Ok(true)
    }
}

impl Drop for RamalamaProvider {
    fn drop(&mut self) {
        // Belt-and-suspenders: `start_kill()` is synchronous and cannot reap,
        // so the SIGKILLed child is reaped by init on this short-lived
        // subprocess. `shutdown()` is the authoritative teardown path. The
        // container is removed by name synchronously (fire-and-forget).
        if let Some(container_name) = self.container_name.lock().unwrap().take() {
            let removed = std::process::Command::new("docker")
                .args(["rm", "-f", &container_name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if !matches!(removed, Ok(status) if status.success()) {
                let _ = std::process::Command::new("ramalama")
                    .args(["stop", &container_name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::ScopeType;

    fn test_provider(port: Option<u16>) -> RamalamaProvider {
        RamalamaProvider {
            config: HarnessConfig {
                agent_type: "conversation_title".into(),
                harness: "ramalama".into(),
                provider_config_ref: crate::providers::RLML_MINI_SENTINEL_ID.into(),
                model: Some("phi4-mini".into()),
                effort: None,
                scope: ScopeType::User,
            },
            model_config: "phi4-mini".into(),
            config_root: PathBuf::from("/tmp"),
            log_data: false,
            child: Mutex::new(None),
            port: Mutex::new(port),
            container_name: Mutex::new(None),
            served_model_id: Mutex::new(None),
            client: Mutex::new(None),
            server: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            reader_cancellation: Mutex::new(None),
        }
    }

    #[test]
    fn test_build_opencode_snippet() {
        let provider = test_provider(Some(12345));
        let snippet = provider.build_opencode_provider_snippet().unwrap();
        let v: serde_json::Value = serde_json::from_str(&snippet).unwrap();
        assert_eq!(
            v["provider"]["openai-compatible"]["npm"],
            "@ai-sdk/openai-compatible"
        );
        assert_eq!(
            v["provider"]["openai-compatible"]["options"]["baseURL"],
            "http://127.0.0.1:12345/v1"
        );
        assert_eq!(
            v["provider"]["openai-compatible"]["models"]["phi4-mini"]["name"],
            "phi4-mini"
        );
    }

    #[test]
    fn test_build_opencode_snippet_not_started() {
        let provider = test_provider(None);
        assert!(matches!(
            provider.build_opencode_provider_snippet(),
            Err(ProviderError::NotStarted)
        ));
    }

    #[test]
    fn test_build_server_config_merges_snippet() {
        let provider = test_provider(Some(12345));
        let snippet = provider.build_opencode_provider_snippet().unwrap();
        let config = provider.build_server_config(&snippet);
        assert_eq!(
            config["provider"]["openai-compatible"]["options"]["baseURL"],
            "http://127.0.0.1:12345/v1"
        );
        assert_eq!(config["permission"]["bash"], "allow");
    }

    #[tokio::test]
    async fn test_shutdown_without_start() {
        let mut provider = test_provider(None);
        assert!(provider.shutdown().await.is_ok());
    }

    #[tokio::test]
    async fn test_get_models_list_returns_model() {
        let provider = test_provider(None);
        let models = provider.get_models_list().await.unwrap();
        assert_eq!(models, vec!["phi4-mini"]);
    }

    #[test]
    fn test_resolve_model_ref_short_name() {
        let provider = test_provider(None);
        assert_eq!(
            provider.resolve_model_ref(),
            "ollama://library/phi4-mini:latest"
        );
    }

    #[test]
    fn test_resolve_model_ref_qualified_passthrough() {
        let mut provider = test_provider(None);
        provider.model_config = "ollama://library/phi4-mini:latest".into();
        assert_eq!(
            provider.resolve_model_ref(),
            "ollama://library/phi4-mini:latest"
        );

        let mut provider = test_provider(None);
        provider.model_config = "library/phi4-mini".into();
        assert_eq!(provider.resolve_model_ref(), "library/phi4-mini");
    }

    #[test]
    fn test_prompt_model_id_prefers_served_id() {
        let provider = test_provider(None);
        *provider.served_model_id.lock().unwrap() = Some("library/phi4-mini".into());
        assert_eq!(provider.prompt_model_id(), "library/phi4-mini");

        let provider = test_provider(None);
        assert_eq!(provider.prompt_model_id(), "phi4-mini");
    }

    #[test]
    fn test_build_prompt_body_uses_served_model_id() {
        let provider = test_provider(None);
        *provider.served_model_id.lock().unwrap() = Some("library/phi4-mini".into());
        let body = provider.build_prompt_body("hello");
        assert_eq!(body.model.unwrap().model_id, "library/phi4-mini");
    }

    #[test]
    fn test_container_name_prefixed() {
        assert_eq!(format!("{CONTAINER_PREFIX}38001"), "ofm-ramalama-38001");
    }

    /// End-to-end lifecycle test against a real `ramalama serve` + docker.
    /// Skipped when ramalama is not on PATH. Verifies:
    /// 1. `ensure_started()` spawns the CLI and waits for health on
    ///    `/v1/models` (not the Ollama-only `/api/tags`).
    /// 2. The served model id is resolved to the llama.cpp slug.
    /// 3. `shutdown()` kills the CLI and removes the named container.
    #[tokio::test]
    #[ignore = "requires ramalama CLI + docker"]
    async fn test_real_ramalama_lifecycle() {
        let probe = std::process::Command::new("ramalama").output();
        if probe.is_err() {
            eprintln!("skipping: ramalama not on PATH");
            return;
        }

        let provider = test_provider(None);
        provider.ensure_started().await.expect("ensure_started");

        let port = provider.port.lock().unwrap().expect("port set");
        let served = provider.served_model_id.lock().unwrap().clone();
        tracing::info!(port, served = ?served, "lifecycle test server up");
        assert!(
            served.is_some(),
            "expected served model id resolved from /v1/models"
        );

        // Health check: /v1/models should respond (the fix for issue #1).
        let resp = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/v1/models"))
            .send()
            .await
            .expect("health request");
        assert!(resp.status().is_success(), "health endpoint should be 200");

        // One-shot through the transient opencode server.
        let one_shot = provider
            .one_shot_prompt("Reply with the single word: pineapple", "phi4-mini")
            .await;
        tracing::info!(one_shot = ?one_shot, "lifecycle test one_shot result");
        assert!(
            one_shot.is_ok(),
            "one_shot_prompt should succeed: {one_shot:?}"
        );

        // Teardown must remove the container.
        let container = provider.container_name.lock().unwrap().clone();
        let mut provider = provider;
        provider.shutdown().await.expect("shutdown");
        if let Some(name) = container {
            let output = std::process::Command::new("docker")
                .args([
                    "ps",
                    "-a",
                    "--filter",
                    &format!("name={name}"),
                    "--format",
                    "{{.Names}}",
                ])
                .output()
                .expect("docker ps");
            let names = String::from_utf8_lossy(&output.stdout);
            assert!(
                !names.contains(&name),
                "container '{name}' should be removed after shutdown, got: {names}"
            );
        }
    }
}
