use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc;

pub mod provider;
pub mod session;

use crate::providers::types::{ProviderEvent, ResumeInput, TurnInput};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub struct OhMyPiSession {
    pub pid: u32,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    pub pair: portable_pty::PtyPair,
    writer: Option<Box<dyn Write + Send>>,
}

pub fn spawn_oh_my_pi(
    binary: &str,
    cwd: &str,
    env_vars: HashMap<String, String>,
) -> Result<OhMyPiSession, BoxError> {
    let system = native_pty_system();
    let pair = system.openpty(PtySize::default())?;

    let mut cmd = CommandBuilder::new(binary);
    cmd.arg("--mode");
    cmd.arg("rpc");
    cmd.cwd(cwd);
    for (key, val) in &env_vars {
        cmd.env(key, val);
    }
    if !env_vars.contains_key("PATH") {
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", &path);
        }
    }

    let child = pair.slave.spawn_command(cmd)?;
    let pid = child.process_id().unwrap_or(0);

    Ok(OhMyPiSession {
        pid,
        child,
        pair,
        writer: None,
    })
}

impl OhMyPiSession {
    pub fn start_turn(
        &mut self,
        input: &TurnInput,
        tx: mpsc::Sender<ProviderEvent>,
    ) -> Result<(), BoxError> {
        let cmd = serde_json::json!({
            "id": "req_1",
            "type": "prompt",
            "message": input.prompt,
            "images": [],
        });
        self.send_raw(&serde_json::to_string(&cmd)?, tx)
    }

    pub fn resume_turn(
        &mut self,
        input: &ResumeInput,
        tx: mpsc::Sender<ProviderEvent>,
    ) -> Result<(), BoxError> {
        let last_message = input
            .messages
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|m| m.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        let cmd = serde_json::json!({
            "id": "req_resume",
            "type": "prompt",
            "message": last_message,
            "streamingBehavior": "steer",
        });
        self.send_raw(&serde_json::to_string(&cmd)?, tx)
    }

    pub fn abort_turn(&mut self) -> Result<(), BoxError> {
        if let Some(writer) = self.writer.as_mut() {
            writeln!(writer, r#"{{"type":"abort"}}"#)?;
            writer.flush()?;
        }
        Ok(())
    }

    fn send_raw(&mut self, json: &str, tx: mpsc::Sender<ProviderEvent>) -> Result<(), BoxError> {
        if self.writer.is_none() {
            self.writer = Some(self.pair.master.take_writer()?);
        }
        let writer = self.writer.as_mut().unwrap();
        tracing::debug!("omp >> {}", json);
        writeln!(writer, "{json}")?;
        writer.flush()?;

        let reader = self.pair.master.try_clone_reader()?;
        let killer = self.child.clone_killer();
        spawn_reader(reader, killer, tx);
        Ok(())
    }
}

impl Drop for OhMyPiSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_provider_event(line: &str) -> Option<ProviderEvent> {
    if let Ok(event) = serde_json::from_str::<ProviderEvent>(line) {
        return Some(event);
    }

    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    let type_name = val.get("type")?.as_str()?;

    Some(match type_name {
        "response" => ProviderEvent::Response(val),
        "extension_ui_request" => ProviderEvent::ExtensionUiRequest(val),
        "available_commands_update" => ProviderEvent::AvailableCommandsUpdate(val),
        "extension_error" => ProviderEvent::Error {
            error: val
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("extension_error")
                .to_string(),
        },
        "ready" => ProviderEvent::Ready,
        "start" => return None,
        "message_update" => {
            if let Some(event) = val
                .get("assistantMessageEvent")
                .and_then(parse_assistant_message_event)
            {
                return Some(event);
            }
            return None;
        }
        "agent_end" => {
            return Some(ProviderEvent::Done(val));
        }
        "message_end" => {
            if let Some(text) = extract_assistant_text(&val) {
                return Some(ProviderEvent::Text { text });
            }
            return None;
        }
        "message_start"
        | "agent_start"
        | "turn_start"
        | "turn_end"
        | "tool_execution_start"
        | "tool_execution_end"
        | "auto_compaction_start"
        | "auto_compaction_end"
        | "auto_retry_start"
        | "auto_retry_end"
        | "ttsr_triggered"
        | "todo_reminder"
        | "todo_auto_clear"
        | "prompt_result"
        | "command_output"
        | "session_info_update"
        | "config_update"
        | "host_tool_call"
        | "host_tool_cancel"
        | "subagent_lifecycle"
        | "subagent_progress"
        | "subagent_event" => return None,
        _ => {
            tracing::debug!("oh-my-pi: unhandled event type: {type_name}");
            return None;
        }
    })
}

fn parse_assistant_message_event(val: &serde_json::Value) -> Option<ProviderEvent> {
    let event_type = val.get("type")?.as_str()?;
    Some(match event_type {
        "text_delta" => ProviderEvent::TextChunk {
            delta: val.get("delta")?.as_str()?.to_string(),
        },
        "text_end" => ProviderEvent::TextChunk {
            delta: val.get("content")?.as_str()?.to_string(),
        },
        "thinking_delta" => ProviderEvent::ThinkingChunk {
            delta: val.get("delta")?.as_str()?.to_string(),
        },
        "thinking_end" => ProviderEvent::ThinkingChunk {
            delta: val.get("content")?.as_str()?.to_string(),
        },
        "tool_use_delta" => {
            let name_str = val
                .get("name")
                .or_else(|| val.get("toolName"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let id = val
                .get("id")
                .or_else(|| val.get("toolUseId"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string());
            let input = val
                .get("input")
                .or_else(|| val.get("arguments"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            ProviderEvent::ToolUse {
                tool_name: name_str,
                tool_use_id: id,
                input,
            }
        }
        _ => {
            tracing::debug!("oh-my-pi: unhandled assistant message event type: {event_type}");
            return None;
        }
    })
}

fn extract_assistant_text(val: &serde_json::Value) -> Option<String> {
    let content = val.get("message")?.get("content")?.as_array()?;
    let texts: Vec<String> = content
        .iter()
        .filter_map(|block| {
            if block.get("type")?.as_str()? == "text" {
                block.get("text")?.as_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join(""))
    }
}

fn spawn_reader(
    reader: Box<dyn std::io::Read + Send>,
    mut killer: Box<dyn portable_pty::ChildKiller + Send>,
    tx: mpsc::Sender<ProviderEvent>,
) {
    const MAX_LINE_LEN: usize = 10 * 1024 * 1024;
    const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

    tokio::task::spawn_blocking(move || {
        let start = std::time::Instant::now();
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            if start.elapsed() > READ_TIMEOUT {
                tracing::warn!("oh-my-pi: read timeout exceeded, killing subprocess");
                let _ = killer.kill();
                return;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("oh-my-pi: read error: {e}");
                    return;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            if line.len() > MAX_LINE_LEN {
                tracing::warn!("oh-my-pi: line too long ({} bytes), skipping", line.len());
                continue;
            }

            tracing::debug!("omp << {}", &line);
            if let Some(event) = parse_provider_event(&line) {
                let is_done = matches!(&event, ProviderEvent::Done(_));
                if tx.blocking_send(event).is_err() {
                    let _ = killer.kill();
                    return;
                }
                if is_done {
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::sync::mpsc;

    use super::*;

    fn create_mock_binary(script: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let bin_path = dir.path().join("omp");
        let mut file = std::fs::File::create(&bin_path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        let mut perms = std::fs::metadata(&bin_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).unwrap();
        (dir, bin_path)
    }

    #[test]
    fn parse_all_event_types() {
        let cases: Vec<(&str, fn(&ProviderEvent) -> bool)> = vec![
            (
                r#"{"type":"session_start","session_id":"sess-1"}"#,
                |e| matches!(e, ProviderEvent::SessionStart { session_id } if session_id == "sess-1"),
            ),
            (
                r#"{"type":"text","text":"hello"}"#,
                |e| matches!(e, ProviderEvent::Text { text } if text == "hello"),
            ),
            (
                r#"{"type":"text_chunk","delta":"wor"}"#,
                |e| matches!(e, ProviderEvent::TextChunk { delta } if delta == "wor"),
            ),
            (
                r#"{"type":"tool_use","tool_name":"read","tool_use_id":"id1","input":{}}"#,
                |e| {
                    matches!(e, ProviderEvent::ToolUse { tool_name, tool_use_id: Some(id), .. }
                        if tool_name == "read" && id == "id1")
                },
            ),
            (
                r#"{"type":"tool_result","tool_use_id":"id1","result":"ok"}"#,
                |e| {
                    matches!(e, ProviderEvent::ToolResult { tool_use_id: Some(id), result }
                        if id == "id1" && result == "ok")
                },
            ),
            (
                r#"{"type":"thinking","thinking":"hmm"}"#,
                |e| matches!(e, ProviderEvent::Thinking { thinking } if thinking == "hmm"),
            ),
            (
                r#"{"type":"thinking_chunk","delta":"hmm"}"#,
                |e| matches!(e, ProviderEvent::ThinkingChunk { delta } if delta == "hmm"),
            ),
            (
                r#"{"type":"context_usage","tokens_in":10,"tokens_out":20}"#,
                |e| matches!(e, ProviderEvent::ContextUsage(_)),
            ),
            (
                r#"{"type":"error","error":"fail"}"#,
                |e| matches!(e, ProviderEvent::Error { error } if error == "fail"),
            ),
            (r#"{"type":"done"}"#, |e| {
                matches!(e, ProviderEvent::Done(_))
            }),
            (r#"{"type":"ready"}"#, |e| matches!(e, ProviderEvent::Ready)),
            (r#"{"type":"extension_ui_request","key":"val"}"#, |e| {
                matches!(e, ProviderEvent::ExtensionUiRequest(_))
            }),
            (
                r#"{"type":"available_commands_update","commands":[]}"#,
                |e| matches!(e, ProviderEvent::AvailableCommandsUpdate(_)),
            ),
            (r#"{"type":"response","text":"hello"}"#, |e| {
                matches!(e, ProviderEvent::Response(_))
            }),
        ];

        for (json, validator) in cases {
            let event: ProviderEvent = serde_json::from_str(json).unwrap();
            assert!(validator(&event), "failed to parse event: {json}");
        }
    }

    #[test]
    fn turn_input_serialization() {
        let input = TurnInput::new(
            "Hello".into(),
            "/cwd".into(),
            "model-x".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            "models:\n  - name: test".into(),
        );
        let json = serde_json::to_string(&input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "start");
        assert_eq!(parsed["prompt"], "Hello");
        assert_eq!(parsed["cwd"], "/cwd");
        assert_eq!(parsed["model"], "model-x");
        assert_eq!(parsed["effort"], "balanced");
        assert_eq!(parsed["permission_mode"], "auto");
        assert_eq!(parsed["disallowed_tools"], serde_json::json!([]));
        assert_eq!(parsed["models_config"], "models:\n  - name: test");
    }

    #[test]
    fn resume_input_serialization() {
        let input = ResumeInput::new(
            "sess-1".into(),
            serde_json::json!([{"role": "user", "content": "hello"}]),
        );
        let json = serde_json::to_string(&input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "resume");
        assert_eq!(parsed["session_id"], "sess-1");
        assert!(parsed.get("messages").is_some());
    }

    #[tokio::test]
    async fn session_id_capture() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             printf '{\"type\":\"session_start\",\"session_id\":\"mock-sess-1\"}\\n'\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "test".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        let first = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected Some event");

        assert_eq!(first.session_id(), Some("mock-sess-1"));
    }

    #[tokio::test]
    async fn done_detection() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             printf '{\"type\":\"text\",\"text\":\"first\"}\\n'\n\
             sleep 0.1\n\
             printf '{\"type\":\"text\",\"text\":\"second\"}\\n'\n\
             sleep 0.1\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "test".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        let mut events = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
                Ok(Some(event)) => events.push(event),
                _ => break,
            }
        }

        assert!(
            events.len() == 3 || events.len() == 2,
            "expected 2 or 3 events before channel close"
        );
        assert!(matches!(events[0], ProviderEvent::Text { .. }));
        assert!(matches!(events[1], ProviderEvent::Text { .. }));
        assert!(matches!(events[2], ProviderEvent::Done(_)));
    }

    #[tokio::test]
    async fn abort_sigkill() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             printf '{\"type\":\"session_start\",\"session_id\":\"mock-sess-1\"}\\n'\n\
             sleep 30\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let pid = session.pid;
        let (tx, rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "test".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        // Drop the receiver so blocking_send in the reader fails,
        // then drop the session so Drop kills the child.
        drop(rx);
        drop(session);

        // Verify process is gone via /proc/{pid}
        let proc_path = format!("/proc/{pid}");
        let gone = std::iter::repeat(())
            .scan((), |_, _| {
                std::thread::sleep(Duration::from_millis(100));
                Some(std::fs::metadata(&proc_path).is_err())
            })
            .take(200)
            .any(|x| x);

        assert!(gone, "process {pid} should have been killed");
    }

    #[tokio::test]
    async fn unparseable_line_graceful() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             printf '{\"type\":\"text\",\"text\":\"valid1\"}\\n'\n\
             printf 'garbage not json\\n'\n\
             printf '{\"type\":\"text\",\"text\":\"valid2\"}\\n'\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "test".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        let mut events = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
                Ok(Some(event)) => events.push(event),
                _ => break,
            }
        }

        assert!(
            events.len() >= 2,
            "expected at least 2 valid events, got {}",
            events.len()
        );
        assert!(matches!(events[0], ProviderEvent::Text { .. }));
    }

    #[test]
    fn spawn_failure() {
        let env = HashMap::new();
        let result = spawn_oh_my_pi("/nonexistent/path/to/omp", "/tmp", env);
        assert!(result.is_err(), "expected Err for nonexistent binary");
    }

    #[tokio::test]
    async fn stdin_write_before_stream() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             read -r line\n\
             printf '{\"type\":\"text\",\"text\":\"stdin-ok\"}\\n'\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "hello stdin".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        // Read the echoed text event
        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected Some event");

        match event {
            ProviderEvent::Text { text } => {
                assert_eq!(text, "stdin-ok", "expected stdin confirmation text");
            }
            other => panic!("expected Text event, got: {other:?}"),
        }

        // Verify done event follows
        let done = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected Done event");
        assert!(matches!(done, ProviderEvent::Done(_)));
    }

    #[tokio::test]
    async fn verify_mode_rpc_arg() {
        let (_dir, bin) = create_mock_binary(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--mode\" ] && [ \"$2\" = \"rpc\" ]; then\n\
               printf '{\"type\":\"text\",\"text\":\"args-ok\"}\\n'\n\
             else\n\
               printf '{\"type\":\"text\",\"text\":\"args-bad\"}\\n'\n\
             fi\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_oh_my_pi(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let (tx, mut rx) = mpsc::channel(16);
        let input = TurnInput::new(
            "test".into(),
            "/tmp".into(),
            "model".into(),
            "balanced".into(),
            "auto".into(),
            vec![],
            String::new(),
        );
        session.start_turn(&input, tx).unwrap();

        let event = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected Some event");

        match event {
            ProviderEvent::Text { text } => {
                assert_eq!(
                    text, "args-ok",
                    "expected --mode rpc args to be passed, got: {text}"
                );
            }
            other => panic!("expected Text event, got: {other:?}"),
        }
    }
}
