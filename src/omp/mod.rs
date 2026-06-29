use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc;

mod protocol;
pub use protocol::*;

pub struct OmpSession {
    pid: u32,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pair: portable_pty::PtyPair,
}

pub fn spawn_omp(
    binary: &str,
    cwd: &str,
    env_vars: HashMap<String, String>,
) -> Result<OmpSession, Box<dyn std::error::Error + Send + Sync>> {
    let system = native_pty_system();
    let pair = system.openpty(PtySize::default())?;

    let mut cmd = CommandBuilder::new(binary);
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

    Ok(OmpSession { pid, child, pair })
}

impl OmpSession {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn start_turn(
        &mut self,
        input: &TurnInput,
        tx: mpsc::Sender<OmpRpcEvent>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_input_and_read(input, tx)
    }

    pub fn resume_turn(
        &mut self,
        input: &ResumeInput,
        tx: mpsc::Sender<OmpRpcEvent>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_input_and_read(input, tx)
    }

    fn send_input_and_read<T: serde::Serialize>(
        &mut self,
        input: &T,
        tx: mpsc::Sender<OmpRpcEvent>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut writer = self.pair.master.take_writer()?;
        let json = serde_json::to_string(input)?;
        writeln!(writer, "{json}")?;
        writer.flush()?;

        let reader = self.pair.master.try_clone_reader()?;
        let killer = self.child.clone_killer();
        spawn_reader(reader, killer, tx);
        Ok(())
    }
}

impl Drop for OmpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_reader(
    reader: Box<dyn std::io::Read + Send>,
    mut killer: Box<dyn portable_pty::ChildKiller + Send>,
    tx: mpsc::Sender<OmpRpcEvent>,
) {
    const MAX_LINE_LEN: usize = 10 * 1024 * 1024;
    const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            if start.elapsed() > READ_TIMEOUT {
                tracing::warn!("omp: read timeout exceeded, killing subprocess");
                let _ = killer.kill();
                return;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("omp: read error: {e}");
                    break;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            if line.len() > MAX_LINE_LEN {
                tracing::warn!("omp: line too long ({} bytes), skipping", line.len());
                continue;
            }

            match serde_json::from_str::<OmpRpcEvent>(&line) {
                Ok(event) => {
                    let is_done = matches!(event, OmpRpcEvent::Done(_));
                    if tx.blocking_send(event).is_err() {
                        let _ = killer.kill();
                        return;
                    }
                    if is_done {
                        return;
                    }
                }
                Err(e) => {
                    tracing::warn!("omp: parse error on line of length {}: {e}", line.len());
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
        let cases: Vec<(&str, fn(&OmpRpcEvent) -> bool)> = vec![
            (
                r#"{"type":"session_start","session_id":"sess-1"}"#,
                |e| matches!(e, OmpRpcEvent::SessionStart { session_id } if session_id == "sess-1"),
            ),
            (
                r#"{"type":"text","text":"hello"}"#,
                |e| matches!(e, OmpRpcEvent::Text { text } if text == "hello"),
            ),
            (
                r#"{"type":"text_chunk","delta":"wor"}"#,
                |e| matches!(e, OmpRpcEvent::TextChunk { delta } if delta == "wor"),
            ),
            (
                r#"{"type":"tool_use","tool_name":"read","tool_use_id":"id1","input":{}}"#,
                |e| {
                    matches!(e, OmpRpcEvent::ToolUse { tool_name, tool_use_id: Some(id), .. }
                        if tool_name == "read" && id == "id1")
                },
            ),
            (
                r#"{"type":"tool_result","tool_use_id":"id1","result":"ok"}"#,
                |e| {
                    matches!(e, OmpRpcEvent::ToolResult { tool_use_id: Some(id), result }
                        if id == "id1" && result == "ok")
                },
            ),
            (
                r#"{"type":"thinking","thinking":"hmm"}"#,
                |e| matches!(e, OmpRpcEvent::Thinking { thinking } if thinking == "hmm"),
            ),
            (
                r#"{"type":"thinking_chunk","delta":"hmm"}"#,
                |e| matches!(e, OmpRpcEvent::ThinkingChunk { delta } if delta == "hmm"),
            ),
            (
                r#"{"type":"context_usage","tokens_in":10,"tokens_out":20}"#,
                |e| matches!(e, OmpRpcEvent::ContextUsage(_)),
            ),
            (
                r#"{"type":"error","error":"fail"}"#,
                |e| matches!(e, OmpRpcEvent::Error { error } if error == "fail"),
            ),
            (r#"{"type":"done"}"#, |e| matches!(e, OmpRpcEvent::Done(_))),
        ];

        for (json, validator) in cases {
            let event: OmpRpcEvent = serde_json::from_str(json).unwrap();
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
        let mut session = spawn_omp(bin.to_str().unwrap(), "/tmp", env).unwrap();
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
             printf '{\"type\":\"text\",\"text\":\"second\"}\\n'\n\
             printf '{\"type\":\"done\"}\\n'",
        );

        let env = HashMap::new();
        let mut session = spawn_omp(bin.to_str().unwrap(), "/tmp", env).unwrap();
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

        assert_eq!(events.len(), 3, "expected 3 events before channel close");
        assert!(matches!(events[0], OmpRpcEvent::Text { .. }));
        assert!(matches!(events[1], OmpRpcEvent::Text { .. }));
        assert!(matches!(events[2], OmpRpcEvent::Done(_)));
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
        let mut session = spawn_omp(bin.to_str().unwrap(), "/tmp", env).unwrap();
        let pid = session.pid();
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

        // Receive the session_start event first
        let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected session_start");
        assert_eq!(ev.session_id(), Some("mock-sess-1"));

        // Drop the receiver -> blocking_send on next event will fail
        drop(rx);

        // Drop the session -> Drop impl kills the child
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
        let mut session = spawn_omp(bin.to_str().unwrap(), "/tmp", env).unwrap();
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

        assert_eq!(
            events.len(),
            3,
            "expected 3 valid events, got {}",
            events.len()
        );
        assert!(matches!(events[0], OmpRpcEvent::Text { .. }));
        assert!(matches!(events[1], OmpRpcEvent::Text { .. }));
        assert!(matches!(events[2], OmpRpcEvent::Done(_)));
    }

    #[test]
    fn spawn_failure() {
        let env = HashMap::new();
        let result = spawn_omp("/nonexistent/path/to/omp", "/tmp", env);
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
        let mut session = spawn_omp(bin.to_str().unwrap(), "/tmp", env).unwrap();
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
            OmpRpcEvent::Text { text } => {
                assert_eq!(text, "stdin-ok", "expected stdin confirmation text");
            }
            other => panic!("expected Text event, got: {other:?}"),
        }

        // Verify done event follows
        let done = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .unwrap()
            .expect("expected Done event");
        assert!(matches!(done, OmpRpcEvent::Done(_)));
    }
}
