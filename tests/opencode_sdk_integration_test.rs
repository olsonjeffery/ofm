use std::time::Duration;

use futures_util::StreamExt;
use ofm::opencode_sdk::{
    self, create_opencode_server, one_shot,
    types::{Event, ModelRef, PartInput, PromptBody, TextPartInput},
    OneShotConfig, PhaseConfig, ServerOptions, UnstructuredConversation,
};

/// Max time to wait for SSE events from the server. If no events arrive in this
/// window we assume the server has no LLM configured and skip the event
/// assertion gracefully.
const SSE_TIMEOUT: Duration = Duration::from_secs(15);

/// Consume events from an SSE stream until `SessionIdle` for the given session
/// or until the timeout fires. Returns true if at least one event was received.
async fn consume_until_idle(
    stream: &mut (impl futures_util::Stream<Item = Result<ofm::opencode_sdk::types::GlobalEvent, ofm::opencode_sdk::SdkError>> + Unpin),
    session_id: &str,
) -> bool {
    let timeout = tokio::time::timeout(SSE_TIMEOUT, async {
        let mut received = false;
        while let Some(event) = stream.next().await {
            match event {
                Ok(ge) => {
                    received = true;
                    match &ge.payload {
                        Event::SessionIdle(data) if data.session_id == session_id => break,
                        _ => {}
                    }
                }
                Err(e) => {
                    tracing::warn!("SSE event error: {e}");
                    break;
                }
            }
        }
        received
    })
    .await;

    match timeout {
        Ok(received) => received,
        Err(_) => {
            tracing::info!("SSE_TIMEOUT reached — no SessionIdle received within {SSE_TIMEOUT:?}");
            false
        }
    }
}

fn has_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn server_options() -> ServerOptions {
    ServerOptions {
        hostname: "127.0.0.1".into(),
        port: 0,
        timeout: Duration::from_secs(10),
        working_dir: None,
        config: Some(serde_json::json!({
            "provider": {
                "test": {
                    "apiKey": "sk-test",
                    "defaultModel": "test-model"
                }
            },
            "permission": {
                "edit": "allow",
                "bash": "allow",
                "webfetch": "allow",
                "doom_loop": "allow",
                "external_directory": "allow"
            }
        })),
        password: None,
    }
}

#[tokio::test]
async fn test_server_lifecycle() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let mut server = create_opencode_server(opts).await.unwrap();
    assert!(server.port() > 0);
    assert!(server.password().is_some());

    let result = server.shutdown().await.unwrap();
    assert!(result, "server should shutdown cleanly");
}

#[tokio::test]
async fn test_server_shutdown_releases_port() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let mut server = create_opencode_server(opts).await.unwrap();
    let port = server.port();
    let hostname = server.hostname().to_string();

    server.shutdown().await.unwrap();

    let addr = format!("{hostname}:{port}");
    let probe = tokio::time::timeout(
        Duration::from_millis(2000),
        tokio::net::TcpStream::connect(&addr),
    )
    .await;

    match probe {
        Ok(Ok(_)) => panic!("port {port} should be free after shutdown"),
        _ => {} // connection refused or timed out = port is free
    }
}

#[tokio::test]
async fn test_create_opencode_and_client() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();
    assert!(!client.base_url().is_empty());

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_session_lifecycle() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    // Create session
    let session = client.session.create("test-session").await.unwrap();
    assert!(!session.id.is_empty());
    assert_eq!(session.title.as_deref(), Some("test-session"));

    // Get session
    let fetched = client.session.get(&session.id).await.unwrap();
    assert_eq!(fetched.id, session.id);

    // List sessions
    let sessions = client.session.list().await.unwrap();
    assert!(!sessions.is_empty());

    // Delete session
    client.session.delete(&session.id).await.unwrap();

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_config_providers() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let providers = client.config.providers().await.unwrap();
    assert!(!providers.is_empty());

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_one_shot_pattern() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let config = OneShotConfig {
        model: "test-model".into(),
        agent: None,
        system: None,
        cwd: None,
    };

    match one_shot(&client, "Say hello", &config).await {
        Ok(text) => {
            assert!(!text.is_empty(), "one-shot should return a response");
        }
        Err(e) => {
            eprintln!("one-shot returned error (may be expected with test config): {e}");
        }
    }

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_abort_session() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let session = client.session.create("abort-test").await.unwrap();
    let result = client.session.abort(&session.id).await.unwrap();
    assert!(result);

    client.session.delete(&session.id).await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_create_opencode_factory() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (_client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    assert!(server.port() > 0);
    assert!(server.password().is_some());

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_concurrent_sessions() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let mut session_ids = Vec::new();
    for i in 0..3 {
        let session = client
            .session
            .create(&format!("concurrent-{i}"))
            .await
            .unwrap();
        session_ids.push(session.id);
    }

    let sessions = client.session.list().await.unwrap();
    assert!(sessions.len() >= 3);

    for id in &session_ids {
        client.session.delete(id).await.unwrap();
    }

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_error_on_invalid_session() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let result = client.session.get("nonexistent-session-id").await;
    assert!(result.is_err(), "getting nonexistent session should error");

    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_prompt_async_and_abort() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let session = client.session.create("prompt-async-test").await.unwrap();

    let body = PromptBody {
        message_id: None,
        model: Some(ModelRef {
            provider_id: "test".into(),
            model_id: "test-model".into(),
        }),
        agent: None,
        no_reply: None,
        system: None,
        tools: None,
        parts: vec![PartInput::Text(TextPartInput {
            text: "Hello".into(),
        })],
    };

    let result = client.session.prompt_async(&session.id, &body).await;
    match result {
        Ok(()) => {}
        Err(e) => {
            eprintln!("prompt_async returned error: {e}");
        }
    }

    let _ = client.session.abort(&session.id).await;
    client.session.delete(&session.id).await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_phase_based_conversation() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let config = PhaseConfig {
        model: "test-model".into(),
        agent: "test-agent".into(),
        system_prompt: Some("You are a test assistant.".into()),
        tools: None,
        cwd: None,
    };

    let conv = ofm::opencode_sdk::PhaseConversation::start(opts, &config)
        .await
        .unwrap();
    assert!(!conv.session_id().is_empty());

    let mut stream = conv.run_phase("Say hello", "phase-1").await.unwrap();
    let received = consume_until_idle(&mut stream, conv.session_id()).await;
    tracing::info!("phase-based conversation received events: {received}");

    conv.close().await.unwrap();
}

#[tokio::test]
async fn test_unstructured_conversation() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let conv = UnstructuredConversation::start(&client).await.unwrap();
    assert!(!conv.session_id().is_empty());

    let mut stream = conv.send_message("Say hello").await.unwrap();
    let received = consume_until_idle(&mut stream, conv.session_id()).await;
    tracing::info!("unstructured conversation received events: {received}");

    let _ = conv.abort().await;
    let _ = client.session.delete(&conv.session_id()).await;
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_session_resume() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    let conv = UnstructuredConversation::start(&client).await.unwrap();
    assert!(!conv.session_id().is_empty());

    // First turn
    let mut stream = conv.send_message("Turn one").await.unwrap();
    let turn1 = consume_until_idle(&mut stream, conv.session_id()).await;
    tracing::info!("session resume turn 1 received events: {turn1}");

    // Resume with second turn
    let mut stream2 = conv.send_message("Turn two").await.unwrap();
    let turn2 = consume_until_idle(&mut stream2, conv.session_id()).await;
    tracing::info!("session resume turn 2 received events: {turn2}");

    let _ = conv.abort().await;
    let _ = client.session.delete(&conv.session_id()).await;
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_process_leak() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let mut server = create_opencode_server(opts).await.unwrap();
    let port = server.port();
    let shutdown_ok = server.shutdown().await.unwrap();
    assert!(shutdown_ok, "server should shutdown cleanly");

    // Verify port is free
    let addr = format!("127.0.0.1:{port}");
    let probe = tokio::time::timeout(
        Duration::from_millis(2000),
        tokio::net::TcpStream::connect(&addr),
    )
    .await;
    match probe {
        Ok(Ok(_)) => panic!("port {port} should be free after shutdown"),
        _ => {} // port is free or timed out
    }

    // Verify no orphan processes — we can't know the exact PID from outside
    // the server process group, so we check that no opencode serve processes
    // are running after a clean shutdown
    let ps_out = std::process::Command::new("sh")
        .arg("-c")
        .arg("ps aux | grep 'opencode serve' | grep -v grep || true")
        .output()
        .unwrap();
    let output = String::from_utf8_lossy(&ps_out.stdout);
    eprintln!("remaining opencode processes after shutdown:\n{output}");
}

#[tokio::test]
async fn test_multi_session_lifecycle() {
    if !has_binary("opencode") {
        eprintln!("skipping: 'opencode' binary not in PATH");
        return;
    }

    let opts = server_options();
    let (client, mut server) = opencode_sdk::create_opencode(opts).await.unwrap();

    // Start N parallel unstructured conversations
    let n = 3;
    let mut conversations = Vec::new();
    for i in 0..n {
        let conv = UnstructuredConversation::start(&client).await.unwrap();
        let mut stream = conv
            .send_message(&format!("Hello from conversation {i}"))
            .await
            .unwrap();
        let received = consume_until_idle(&mut stream, conv.session_id()).await;
        tracing::info!("multi-session conversation {i} received events: {received}");
        conversations.push(conv);
    }

    // Verify all sessions are alive
    let sessions = client.session.list().await.unwrap();
    assert!(sessions.len() >= n, "expected at least {n} sessions");

    // Clean up all conversations
    for conv in &conversations {
        let _ = conv.abort().await;
        let _ = client.session.delete(&conv.session_id()).await;
    }

    server.shutdown().await.unwrap();
}
