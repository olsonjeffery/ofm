use serde::{Deserialize, Serialize};

// ── Global Event ──────────────────────────────────────────────────────────
//
// The opencode server emits SSE events as flat JSON objects:
//   {"id":"evt_...","type":"session.idle","properties":{"sessionID":"s1"}}
//
// The `id` is an SSE event identifier (optional, used for reconnection).
// The `type` and `properties` fields are flattened into the `Event` enum
// via serde's flatten attribute.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub payload: Event,
}

// ── Event ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "properties", rename_all = "snake_case")]
pub enum Event {
    #[serde(rename = "message.part.updated")]
    MessagePartUpdated(MessagePartUpdatedData),
    #[serde(rename = "message.updated")]
    MessageUpdated(MessageUpdatedData),
    #[serde(rename = "message.removed")]
    MessageRemoved(MessageRemovedData),
    #[serde(rename = "message.part.removed")]
    MessagePartRemoved(MessagePartRemovedData),

    #[serde(rename = "session.status")]
    SessionStatus(SessionStatusData),
    #[serde(rename = "session.idle")]
    SessionIdle(SessionIdData),
    #[serde(rename = "session.created")]
    SessionCreated(SessionCreatedData),
    #[serde(rename = "session.updated")]
    SessionUpdated(SessionUpdatedData),
    #[serde(rename = "session.deleted")]
    SessionDeleted(SessionIdData),
    #[serde(rename = "session.error")]
    SessionError(SessionErrorData),
    #[serde(rename = "session.compacted")]
    SessionCompacted(SessionIdData),
    #[serde(rename = "session.diff")]
    SessionDiff(SessionDiffData),

    #[serde(rename = "server.connected")]
    ServerConnected(ServerConnectedData),
    #[serde(rename = "server.instance.disposed")]
    ServerInstanceDisposed(serde_json::Value),

    #[serde(rename = "file.edited")]
    FileEdited(serde_json::Value),
    #[serde(rename = "todo.updated")]
    TodoUpdated(serde_json::Value),
    #[serde(rename = "command.executed")]
    CommandExecuted(serde_json::Value),
    #[serde(rename = "file_watcher.updated")]
    FileWatcherUpdated(serde_json::Value),
    #[serde(rename = "vcs.branch.updated")]
    VcsBranchUpdated(serde_json::Value),

    #[serde(rename = "pty.created")]
    PtyCreated(PtyEventData),
    #[serde(rename = "pty.updated")]
    PtyUpdated(PtyOutputData),
    #[serde(rename = "pty.exited")]
    PtyExited(PtyExitData),
    #[serde(rename = "pty.deleted")]
    PtyDeleted(PtyIdData),

    #[serde(rename = "installation.updated")]
    InstallationUpdated(serde_json::Value),
    #[serde(rename = "installation.update_available")]
    InstallationUpdateAvailable(serde_json::Value),

    #[serde(rename = "lsp.client_diagnostics")]
    LspClientDiagnostics(serde_json::Value),
    #[serde(rename = "lsp.updated")]
    LspUpdated(serde_json::Value),

    #[serde(rename = "permission.updated")]
    PermissionUpdated(PermissionData),
    #[serde(rename = "permission.replied")]
    PermissionReplied(PermissionReplyData),

    #[serde(rename = "tui.prompt_append")]
    TuiPromptAppend(serde_json::Value),
    #[serde(rename = "tui.command_execute")]
    TuiCommandExecute(serde_json::Value),
    #[serde(rename = "tui.toast_show")]
    TuiToastShow(TuiToastData),
}

// ── Event property types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePartUpdatedData {
    pub part: Part,
    #[serde(default)]
    pub delta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUpdatedData {
    pub info: AssistantMessage,
    #[serde(default)]
    pub parts: Option<Vec<Part>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRemovedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePartRemovedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(rename = "partID")]
    pub part_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub status: SessionStatusValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusValue {
    #[serde(rename = "type")]
    pub status_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIdData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreatedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUpdatedData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionErrorData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// The opencode server sends this as a nested object
    /// `{"name":"UnknownError","data":{"message":"..."}}` but some
    /// older paths may send a plain string. We accept both.
    pub error: serde_json::Value,
}

impl SessionErrorData {
    /// Extract a human-readable error message from the `error` field,
    /// handling both the nested-object and plain-string formats.
    pub fn error_message(&self) -> String {
        if let Some(s) = self.error.as_str() {
            return s.to_string();
        }
        if let Some(data) = self.error.get("data") {
            if let Some(msg) = data.get("message").and_then(|m| m.as_str()) {
                return msg.to_string();
            }
        }
        if let Some(name) = self.error.get("name").and_then(|n| n.as_str()) {
            return name.to_string();
        }
        self.error.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDiffData {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub diff: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConnectedData {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyEventData {
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyOutputData {
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyExitData {
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyIdData {
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionData {
    #[serde(rename = "permissionID")]
    pub permission_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "type")]
    pub permission_type: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionReplyData {
    #[serde(rename = "permissionID")]
    pub permission_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiToastData {
    pub message: String,
    #[serde(rename = "type")]
    pub toast_type: String,
    #[serde(default)]
    pub duration: Option<u64>,
}

// ── Message ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// The opencode server sends `time` as `{"created": 1784579380826}`
    /// but some older paths may send a plain string. Accept both.
    #[serde(default)]
    pub time: serde_json::Value,
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// The opencode server sends `time` as `{"created": 1784579380826}`
    /// but some older paths may send a plain string. Accept both.
    #[serde(default)]
    pub time: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(rename = "parentID")]
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(rename = "modelID")]
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(rename = "providerID")]
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub tokens: Option<TokenUsage>,
    #[serde(default)]
    pub finish: Option<String>,
    pub parts: Vec<Part>,
}

// ── Part ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Part {
    #[serde(rename = "text")]
    Text(TextPart),
    #[serde(rename = "reasoning")]
    Reasoning(ReasoningPart),
    #[serde(rename = "tool")]
    Tool(ToolPart),
    #[serde(rename = "file")]
    File(FilePart),
    #[serde(rename = "step_start")]
    StepStart(StepStartPart),
    #[serde(rename = "step_finish")]
    StepFinish(StepFinishPart),
    #[serde(rename = "snapshot")]
    Snapshot(SnapshotPart),
    #[serde(rename = "patch")]
    Patch(PatchPart),
    #[serde(rename = "agent")]
    Agent(AgentPart),
    #[serde(rename = "retry")]
    Retry(RetryPart),
    #[serde(rename = "compaction")]
    Compaction(CompactionPart),
    #[serde(rename = "subtask")]
    Subtask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPart {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningPart {
    pub text: String,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPart {
    pub tool: String,
    #[serde(rename = "callID")]
    pub call_id: String,
    pub state: ToolState,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePart {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStartPart {
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepFinishPart {
    pub name: String,
    #[serde(default)]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPart {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchPart {
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPart {
    pub agent: String,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPart {
    pub reason: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionPart {
    pub summary: String,
}

// ── ToolState ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ToolState {
    #[serde(rename = "pending")]
    Pending(ToolStatePending),
    #[serde(rename = "running")]
    Running(ToolStateRunning),
    #[serde(rename = "completed")]
    Completed(ToolStateCompleted),
    #[serde(rename = "error")]
    Error(ToolStateError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatePending {
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateRunning {
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateCompleted {
    pub input: serde_json::Value,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStateError {
    pub input: serde_json::Value,
    pub error: String,
}

// ── Session ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub directory: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model: Option<serde_json::Value>,
    #[serde(default)]
    pub agent: Option<serde_json::Value>,
    #[serde(default)]
    pub created: Option<serde_json::Value>,
    #[serde(default)]
    pub updated: Option<serde_json::Value>,
}

// ── Provider ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub source: String,
    pub env: serde_json::Value,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    pub models: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub default: Option<bool>,
}

// ── PartInput ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PartInput {
    #[serde(rename = "text")]
    Text(TextPartInput),
    #[serde(rename = "file")]
    File(FilePartInput),
    #[serde(rename = "agent")]
    Agent(AgentPartInput),
    #[serde(rename = "subtask")]
    Subtask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPartInput {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePartInput {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPartInput {
    pub agent: String,
    pub prompt: String,
}

// ── PromptBody ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBody {
    #[serde(rename = "messageID", default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(rename = "noReply", default, skip_serializing_if = "Option::is_none")]
    pub no_reply: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    pub parts: Vec<PartInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

// ── TokenUsage ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub reasoning: Option<u64>,
    #[serde(default)]
    pub cache: Option<CacheUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheUsage {
    pub read: u64,
    pub write: u64,
}

// ── PromptResponse ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    pub info: AssistantMessage,
    pub parts: Vec<Part>,
}

// ── Question types (from question.asked events) ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskedQuestion {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn global_event(payload: &str) -> String {
        // The real opencode SSE format is a flat JSON object with `id`,
        // `type`, and `properties` at the top level. We construct it by
        // merging `payload` (which is already `{type, properties}`) with
        // an `id` field.
        let mut v: serde_json::Value = serde_json::from_str(payload).unwrap();
        if let Some(obj) = v.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!("evt_test"));
        }
        serde_json::to_string(&v).unwrap()
    }

    fn parse_event(json: &str) -> Event {
        let ge: GlobalEvent = serde_json::from_str(json).unwrap();
        ge.payload
    }

    #[test]
    fn test_opencode_sdk_event_message_part_updated_text() {
        let json = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"text","text":"Hello"},"delta":"Hello"}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessagePartUpdated(data) => {
                assert_eq!(data.delta.as_deref(), Some("Hello"));
                match &data.part {
                    Part::Text(t) => assert_eq!(t.text, "Hello"),
                    _ => panic!("expected Text part"),
                }
            }
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_message_part_updated_reasoning() {
        let json = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"reasoning","text":"thinking..."},"delta":"thinking..."}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessagePartUpdated(data) => match &data.part {
                Part::Reasoning(r) => assert_eq!(r.text, "thinking..."),
                _ => panic!("expected Reasoning part"),
            },
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_message_part_updated_tool() {
        let json = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"tool","tool":"read","callID":"id1","state":{"status":"pending","input":{"path":"/tmp"}}}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessagePartUpdated(data) => match &data.part {
                Part::Tool(t) => {
                    assert_eq!(t.tool, "read");
                    assert_eq!(t.call_id, "id1");
                    assert!(matches!(t.state, ToolState::Pending(_)));
                }
                _ => panic!("expected Tool part"),
            },
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_message_part_updated_tool_completed() {
        let json = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"tool","tool":"read","callID":"id1","state":{"status":"completed","input":{},"output":"result"}}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessagePartUpdated(data) => match &data.part {
                Part::Tool(t) => {
                    assert!(matches!(t.state, ToolState::Completed(ref s) if s.output == "result"));
                }
                _ => panic!("expected Tool part"),
            },
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_message_part_updated_tool_error() {
        let json = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"tool","tool":"read","callID":"id1","state":{"status":"error","input":{},"error":"failed"}}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessagePartUpdated(data) => match &data.part {
                Part::Tool(t) => {
                    assert!(matches!(t.state, ToolState::Error(ref s) if s.error == "failed"));
                }
                _ => panic!("expected Tool part"),
            },
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_message_updated() {
        let json = global_event(
            r#"{"type":"message.updated","properties":{"info":{"id":"msg1","sessionID":"sess1","role":"assistant","time":"2024-01-01T00:00:00Z","parts":[{"type":"text","text":"Hello"}]}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::MessageUpdated(data) => {
                assert_eq!(data.info.id, "msg1");
                assert_eq!(data.info.session_id, "sess1");
            }
            _ => panic!("expected MessageUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_session_status_idle() {
        let json = global_event(
            r#"{"type":"session.status","properties":{"sessionID":"sess1","status":{"type":"idle"}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::SessionStatus(data) => {
                assert_eq!(data.session_id, "sess1");
                assert_eq!(data.status.status_type, "idle");
            }
            _ => panic!("expected SessionStatus"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_session_created() {
        let json = global_event(
            r#"{"type":"session.created","properties":{"sessionID":"sess1","session":{"id":"sess1","directory":"/tmp"}}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::SessionCreated(data) => {
                assert_eq!(data.session_id, "sess1");
                assert_eq!(data.session.id, "sess1");
            }
            _ => panic!("expected SessionCreated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_session_idle() {
        let json = global_event(r#"{"type":"session.idle","properties":{"sessionID":"sess1"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::SessionIdle(data) if data.session_id == "sess1"));
    }

    #[test]
    fn test_opencode_sdk_event_session_error() {
        let json = global_event(
            r#"{"type":"session.error","properties":{"sessionID":"sess1","error":"something went wrong"}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::SessionError(data) => {
                assert_eq!(data.session_id, "sess1");
                assert_eq!(data.error_message(), "something went wrong");
            }
            _ => panic!("expected SessionError"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_server_connected() {
        let json = global_event(r#"{"type":"server.connected","properties":{"version":"1.15.5"}}"#);
        let event = parse_event(&json);
        assert!(
            matches!(event, Event::ServerConnected(data) if data.version.as_deref() == Some("1.15.5"))
        );
    }

    #[test]
    fn test_opencode_sdk_event_server_instance_disposed() {
        let json = global_event(r#"{"type":"server.instance.disposed","properties":{}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::ServerInstanceDisposed(_)));
    }

    #[test]
    fn test_opencode_sdk_event_file_edited() {
        let json = global_event(r#"{"type":"file.edited","properties":{"path":"/tmp/test.txt"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::FileEdited(_)));
    }

    #[test]
    fn test_opencode_sdk_event_todo_updated() {
        let json = global_event(r#"{"type":"todo.updated","properties":{"items":[]}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::TodoUpdated(_)));
    }

    #[test]
    fn test_opencode_sdk_event_command_executed() {
        let json = global_event(r#"{"type":"command.executed","properties":{"command":"ls"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::CommandExecuted(_)));
    }

    #[test]
    fn test_opencode_sdk_event_file_watcher_updated() {
        let json =
            global_event(r#"{"type":"file_watcher.updated","properties":{"change":"created"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::FileWatcherUpdated(_)));
    }

    #[test]
    fn test_opencode_sdk_event_vcs_branch_updated() {
        let json = global_event(r#"{"type":"vcs.branch.updated","properties":{"branch":"main"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::VcsBranchUpdated(_)));
    }

    #[test]
    fn test_opencode_sdk_event_pty_created() {
        let json = global_event(
            r#"{"type":"pty.created","properties":{"ptyID":"pty1","sessionID":"sess1","cols":80,"rows":24}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::PtyCreated(data) => {
                assert_eq!(data.pty_id, "pty1");
                assert_eq!(data.session_id, "sess1");
            }
            _ => panic!("expected PtyCreated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_pty_updated() {
        let json = global_event(
            r#"{"type":"pty.updated","properties":{"ptyID":"pty1","sessionID":"sess1","data":"output"}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::PtyUpdated(data) if data.data == "output"));
    }

    #[test]
    fn test_opencode_sdk_event_pty_exited() {
        let json = global_event(
            r#"{"type":"pty.exited","properties":{"ptyID":"pty1","sessionID":"sess1","code":0}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::PtyExited(data) if data.code == 0));
    }

    #[test]
    fn test_opencode_sdk_event_pty_deleted() {
        let json = global_event(
            r#"{"type":"pty.deleted","properties":{"ptyID":"pty1","sessionID":"sess1"}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::PtyDeleted(_)));
    }

    #[test]
    fn test_opencode_sdk_event_installation_updated() {
        let json = global_event(r#"{"type":"installation.updated","properties":{"name":"test"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::InstallationUpdated(_)));
    }

    #[test]
    fn test_opencode_sdk_event_installation_update_available() {
        let json = global_event(
            r#"{"type":"installation.update_available","properties":{"version":"1.16.0"}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::InstallationUpdateAvailable(_)));
    }

    #[test]
    fn test_opencode_sdk_event_lsp_client_diagnostics() {
        let json = global_event(
            r#"{"type":"lsp.client_diagnostics","properties":{"file":"test.rs","diagnostics":[]}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::LspClientDiagnostics(_)));
    }

    #[test]
    fn test_opencode_sdk_event_lsp_updated() {
        let json =
            global_event(r#"{"type":"lsp.updated","properties":{"server":"rust-analyzer"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::LspUpdated(_)));
    }

    #[test]
    fn test_opencode_sdk_event_permission_updated() {
        let json = global_event(
            r#"{"type":"permission.updated","properties":{"permissionID":"perm1","sessionID":"sess1","type":"bash"}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::PermissionUpdated(data) => {
                assert_eq!(data.permission_id, "perm1");
                assert_eq!(data.session_id, "sess1");
            }
            _ => panic!("expected PermissionUpdated"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_permission_replied() {
        let json = global_event(
            r#"{"type":"permission.replied","properties":{"permissionID":"perm1","sessionID":"sess1","approved":true}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::PermissionReplied(data) => {
                assert!(data.approved);
            }
            _ => panic!("expected PermissionReplied"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_tui_prompt_append() {
        let json = global_event(r#"{"type":"tui.prompt_append","properties":{"text":"extra"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::TuiPromptAppend(_)));
    }

    #[test]
    fn test_opencode_sdk_event_tui_command_execute() {
        let json =
            global_event(r#"{"type":"tui.command_execute","properties":{"command":"/help"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::TuiCommandExecute(_)));
    }

    #[test]
    fn test_opencode_sdk_event_tui_toast_show() {
        let json = global_event(
            r#"{"type":"tui.toast_show","properties":{"message":"Hello","type":"info","duration":3000}}"#,
        );
        let event = parse_event(&json);
        match event {
            Event::TuiToastShow(data) => {
                assert_eq!(data.message, "Hello");
                assert_eq!(data.toast_type, "info");
            }
            _ => panic!("expected TuiToastShow"),
        }
    }

    #[test]
    fn test_opencode_sdk_event_session_deleted() {
        let json = global_event(r#"{"type":"session.deleted","properties":{"sessionID":"sess1"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::SessionDeleted(data) if data.session_id == "sess1"));
    }

    #[test]
    fn test_opencode_sdk_event_session_compacted() {
        let json =
            global_event(r#"{"type":"session.compacted","properties":{"sessionID":"sess1"}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::SessionCompacted(data) if data.session_id == "sess1"));
    }

    #[test]
    fn test_opencode_sdk_event_session_diff() {
        let json =
            global_event(r#"{"type":"session.diff","properties":{"sessionID":"sess1","diff":{}}}"#);
        let event = parse_event(&json);
        assert!(matches!(event, Event::SessionDiff(_)));
    }

    #[test]
    fn test_opencode_sdk_event_message_removed() {
        let json = global_event(
            r#"{"type":"message.removed","properties":{"sessionID":"sess1","messageID":"msg1"}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::MessageRemoved(_)));
    }

    #[test]
    fn test_opencode_sdk_event_message_part_removed() {
        let json = global_event(
            r#"{"type":"message.part.removed","properties":{"sessionID":"sess1","messageID":"msg1","partID":"part1"}}"#,
        );
        let event = parse_event(&json);
        assert!(matches!(event, Event::MessagePartRemoved(_)));
    }

    #[test]
    fn test_opencode_sdk_event_roundtrip_all_variants() {
        let cases = [
            (
                r#"{"type":"message.part.updated","properties":{"part":{"type":"text","text":"hi"},"delta":"hi"}}"#,
                "message.part.updated",
            ),
            (
                r#"{"type":"message.updated","properties":{"info":{"id":"m1","sessionID":"s1","role":"assistant","time":"t","parts":[{"type":"text","text":"hi"}]}}}"#,
                "message.updated",
            ),
            (
                r#"{"type":"message.removed","properties":{"sessionID":"s1","messageID":"m1"}}"#,
                "message.removed",
            ),
            (
                r#"{"type":"message.part.removed","properties":{"sessionID":"s1","messageID":"m1","partID":"p1"}}"#,
                "message.part.removed",
            ),
            (
                r#"{"type":"session.status","properties":{"sessionID":"s1","status":{"type":"idle"}}}"#,
                "session.status",
            ),
            (
                r#"{"type":"session.idle","properties":{"sessionID":"s1"}}"#,
                "session.idle",
            ),
            (
                r#"{"type":"session.created","properties":{"sessionID":"s1","session":{"id":"s1","directory":"/tmp"}}}"#,
                "session.created",
            ),
            (
                r#"{"type":"session.updated","properties":{"sessionID":"s1","session":{"id":"s1","directory":"/tmp"}}}"#,
                "session.updated",
            ),
            (
                r#"{"type":"session.deleted","properties":{"sessionID":"s1"}}"#,
                "session.deleted",
            ),
            (
                r#"{"type":"session.error","properties":{"sessionID":"s1","error":"err"}}"#,
                "session.error",
            ),
            (
                r#"{"type":"session.compacted","properties":{"sessionID":"s1"}}"#,
                "session.compacted",
            ),
            (
                r#"{"type":"session.diff","properties":{"sessionID":"s1","diff":{}}}"#,
                "session.diff",
            ),
            (
                r#"{"type":"server.connected","properties":{}}"#,
                "server.connected",
            ),
            (
                r#"{"type":"server.instance.disposed","properties":{}}"#,
                "server.instance.disposed",
            ),
            (r#"{"type":"file.edited","properties":{}}"#, "file.edited"),
            (r#"{"type":"todo.updated","properties":{}}"#, "todo.updated"),
            (
                r#"{"type":"command.executed","properties":{}}"#,
                "command.executed",
            ),
            (
                r#"{"type":"file_watcher.updated","properties":{}}"#,
                "file_watcher.updated",
            ),
            (
                r#"{"type":"vcs.branch.updated","properties":{}}"#,
                "vcs.branch.updated",
            ),
            (
                r#"{"type":"pty.created","properties":{"ptyID":"p1","sessionID":"s1"}}"#,
                "pty.created",
            ),
            (
                r#"{"type":"pty.updated","properties":{"ptyID":"p1","sessionID":"s1","data":"o"}}"#,
                "pty.updated",
            ),
            (
                r#"{"type":"pty.exited","properties":{"ptyID":"p1","sessionID":"s1","code":0}}"#,
                "pty.exited",
            ),
            (
                r#"{"type":"pty.deleted","properties":{"ptyID":"p1","sessionID":"s1"}}"#,
                "pty.deleted",
            ),
            (
                r#"{"type":"installation.updated","properties":{}}"#,
                "installation.updated",
            ),
            (
                r#"{"type":"installation.update_available","properties":{}}"#,
                "installation.update_available",
            ),
            (
                r#"{"type":"lsp.client_diagnostics","properties":{}}"#,
                "lsp.client_diagnostics",
            ),
            (r#"{"type":"lsp.updated","properties":{}}"#, "lsp.updated"),
            (
                r#"{"type":"permission.updated","properties":{"permissionID":"p1","sessionID":"s1","type":"bash"}}"#,
                "permission.updated",
            ),
            (
                r#"{"type":"permission.replied","properties":{"permissionID":"p1","sessionID":"s1","approved":true}}"#,
                "permission.replied",
            ),
            (
                r#"{"type":"tui.prompt_append","properties":{}}"#,
                "tui.prompt_append",
            ),
            (
                r#"{"type":"tui.command_execute","properties":{}}"#,
                "tui.command_execute",
            ),
            (
                r#"{"type":"tui.toast_show","properties":{"message":"m","type":"info"}}"#,
                "tui.toast_show",
            ),
        ];
        for (props_json, expected_type) in &cases {
            let full = global_event(props_json);
            let ge: GlobalEvent = serde_json::from_str(&full)
                .unwrap_or_else(|e| panic!("Deserialize failed for {expected_type}: {e}"));
            let re_serialized = serde_json::to_value(&ge.payload).unwrap();
            assert_eq!(
                re_serialized["type"], *expected_type,
                "type mismatch for {expected_type}"
            );
            assert!(
                re_serialized.get("properties").is_some(),
                "missing properties for {expected_type}"
            );
        }
    }

    #[test]
    fn test_opencode_sdk_part_text() {
        let json = r#"{"type":"text","text":"Hello"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        match part {
            Part::Text(t) => assert_eq!(t.text, "Hello"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_opencode_sdk_part_reasoning() {
        let json = r#"{"type":"reasoning","text":"thinking..."}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        match part {
            Part::Reasoning(r) => assert_eq!(r.text, "thinking..."),
            _ => panic!("expected Reasoning"),
        }
    }

    #[test]
    fn test_opencode_sdk_part_tool() {
        let json = r#"{"type":"tool","tool":"read","callID":"id1","state":{"status":"pending","input":{}}}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        match part {
            Part::Tool(t) => {
                assert_eq!(t.tool, "read");
            }
            _ => panic!("expected Tool"),
        }
    }

    #[test]
    fn test_opencode_sdk_part_file() {
        let json = r#"{"type":"file","path":"/tmp/test.txt","content":"data"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        match part {
            Part::File(f) => {
                assert_eq!(f.path, "/tmp/test.txt");
                assert_eq!(f.content.as_deref(), Some("data"));
            }
            _ => panic!("expected File"),
        }
    }

    #[test]
    fn test_opencode_sdk_part_step_start() {
        let json = r#"{"type":"step_start","name":"analyze"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::StepStart(s) if s.name == "analyze"));
    }

    #[test]
    fn test_opencode_sdk_part_step_finish() {
        let json = r#"{"type":"step_finish","name":"analyze"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::StepFinish(s) if s.name == "analyze"));
    }

    #[test]
    fn test_opencode_sdk_part_snapshot() {
        let json = r#"{"type":"snapshot","path":"/tmp/test.rs","content":"fn main() {}"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Snapshot(_)));
    }

    #[test]
    fn test_opencode_sdk_part_patch() {
        let json = r#"{"type":"patch","path":"/tmp/test.rs","diff":"@@ -1 +1 @@"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Patch(_)));
    }

    #[test]
    fn test_opencode_sdk_part_agent() {
        let json = r#"{"type":"agent","agent":"coder","prompt":"fix this"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Agent(a) if a.agent == "coder"));
    }

    #[test]
    fn test_opencode_sdk_part_retry() {
        let json = r#"{"type":"retry","reason":"timeout"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Retry(r) if r.reason == "timeout"));
    }

    #[test]
    fn test_opencode_sdk_part_compaction() {
        let json = r#"{"type":"compaction","summary":"summarized previous turns"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Compaction(c) if c.summary == "summarized previous turns"));
    }

    #[test]
    fn test_opencode_sdk_part_subtask() {
        let json = r#"{"type":"subtask"}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert!(matches!(part, Part::Subtask));
    }

    #[test]
    fn test_opencode_sdk_part_roundtrip() {
        let parts = [
            r#"{"type":"text","text":"hi"}"#,
            r#"{"type":"reasoning","text":"thinking..."}"#,
            r#"{"type":"tool","tool":"read","callID":"id1","state":{"status":"pending","input":{}}}"#,
            r#"{"type":"file","path":"/tmp/f","content":"data"}"#,
            r#"{"type":"step_start","name":"analyze"}"#,
            r#"{"type":"step_finish","name":"analyze"}"#,
            r#"{"type":"snapshot","path":"/tmp/f","content":"code"}"#,
            r#"{"type":"patch","path":"/tmp/f","diff":"diff"}"#,
            r#"{"type":"agent","agent":"coder","prompt":"fix"}"#,
            r#"{"type":"retry","reason":"timeout"}"#,
            r#"{"type":"compaction","summary":"sum"}"#,
            r#"{"type":"subtask"}"#,
        ];
        for json in &parts {
            let part: Part = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("Deserialize failed for {json}: {e}"));
            let re = serde_json::to_value(&part).unwrap();
            assert_eq!(
                re["type"],
                serde_json::from_str::<serde_json::Value>(json).unwrap()["type"],
                "type roundtrip mismatch"
            );
        }
    }

    #[test]
    fn test_opencode_sdk_message_user() {
        let json = r#"{"role":"user","id":"u1","sessionID":"s1","time":"t","agent":"human","parts":[{"type":"text","text":"hi"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        match msg {
            Message::User(u) => {
                assert_eq!(u.id, "u1");
                assert_eq!(u.agent, "human");
            }
            _ => panic!("expected User"),
        }
    }

    #[test]
    fn test_opencode_sdk_message_assistant() {
        let json = r#"{"role":"assistant","id":"a1","sessionID":"s1","time":"t","parts":[{"type":"text","text":"hello"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        match msg {
            Message::Assistant(a) => {
                assert_eq!(a.id, "a1");
            }
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn test_opencode_sdk_tool_state_pending() {
        let ts: ToolState = serde_json::from_str(r#"{"status":"pending","input":{}}"#).unwrap();
        assert!(matches!(ts, ToolState::Pending(_)));
    }

    #[test]
    fn test_opencode_sdk_tool_state_running() {
        let ts: ToolState = serde_json::from_str(r#"{"status":"running","input":{}}"#).unwrap();
        assert!(matches!(ts, ToolState::Running(_)));
    }

    #[test]
    fn test_opencode_sdk_tool_state_completed() {
        let ts: ToolState =
            serde_json::from_str(r#"{"status":"completed","input":{},"output":"done"}"#).unwrap();
        match ts {
            ToolState::Completed(s) => assert_eq!(s.output, "done"),
            _ => panic!("expected Completed"),
        }
    }

    #[test]
    fn test_opencode_sdk_tool_state_error() {
        let ts: ToolState =
            serde_json::from_str(r#"{"status":"error","input":{},"error":"failed"}"#).unwrap();
        match ts {
            ToolState::Error(s) => assert_eq!(s.error, "failed"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_opencode_sdk_session() {
        let json = r#"{"id":"s1","directory":"/tmp","title":"test","model":"gpt-4","agent":"coder","created":"now"}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.id, "s1");
        assert_eq!(session.title.as_deref(), Some("test"));
    }

    #[test]
    fn test_opencode_sdk_provider() {
        let json = r#"{"id":"p1","name":"OpenAI","source":"openai","env":{},"models":{ "gpt-4": { "name":"GPT-4"}}}"#;
        let provider: Provider = serde_json::from_str(json).unwrap();
        assert_eq!(provider.id, "p1");
        assert_eq!(provider.models.len(), 1);
    }

    #[test]
    fn test_opencode_sdk_part_input_text() {
        let pi: PartInput = serde_json::from_str(r#"{"type":"text","text":"Hello"}"#).unwrap();
        assert!(matches!(pi, PartInput::Text(t) if t.text == "Hello"));
    }

    #[test]
    fn test_opencode_sdk_part_input_file() {
        let pi: PartInput =
            serde_json::from_str(r#"{"type":"file","path":"/tmp/test.txt"}"#).unwrap();
        assert!(matches!(pi, PartInput::File(_)));
    }

    #[test]
    fn test_opencode_sdk_part_input_agent() {
        let pi: PartInput =
            serde_json::from_str(r#"{"type":"agent","agent":"coder","prompt":"fix"}"#).unwrap();
        assert!(matches!(pi, PartInput::Agent(_)));
    }

    #[test]
    fn test_opencode_sdk_part_input_subtask() {
        let pi: PartInput = serde_json::from_str(r#"{"type":"subtask"}"#).unwrap();
        assert!(matches!(pi, PartInput::Subtask));
    }

    #[test]
    fn test_opencode_sdk_prompt_body() {
        let json = r#"{"parts":[{"type":"text","text":"Hello"}]}"#;
        let body: PromptBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.parts.len(), 1);
    }

    #[test]
    fn test_opencode_sdk_prompt_body_with_model() {
        let json = r#"{"model":{"providerID":"openai","modelID":"gpt-4"},"parts":[{"type":"text","text":"Hello"}]}"#;
        let body: PromptBody = serde_json::from_str(json).unwrap();
        let model = body.model.unwrap();
        assert_eq!(model.provider_id, "openai");
        assert_eq!(model.model_id, "gpt-4");
    }

    #[test]
    fn test_opencode_sdk_token_usage() {
        let json = r#"{"input":100,"output":50,"reasoning":10,"cache":{"read":5,"write":3}}"#;
        let usage: TokenUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 50);
        let cache = usage.cache.unwrap();
        assert_eq!(cache.read, 5);
        assert_eq!(cache.write, 3);
    }

    #[test]
    fn test_opencode_sdk_prompt_response() {
        let json = r#"{"info":{"id":"a1","sessionID":"s1","role":"assistant","time":"t","parts":[{"type":"text","text":"hello"}]},"parts":[{"type":"text","text":"hello"}]}"#;
        let resp: PromptResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.info.id, "a1");
        assert_eq!(resp.parts.len(), 1);
    }

    #[test]
    fn test_opencode_sdk_global_event_roundtrip() {
        let json = r#"{"id":"evt_test","type":"session.idle","properties":{"sessionID":"s1"}}"#;
        let ge: GlobalEvent = serde_json::from_str(json).unwrap();
        assert_eq!(ge.id.as_deref(), Some("evt_test"));
        match &ge.payload {
            Event::SessionIdle(data) => assert_eq!(data.session_id, "s1"),
            _ => panic!("expected SessionIdle"),
        }
        let re = serde_json::to_string(&ge).unwrap();
        let ge2: GlobalEvent = serde_json::from_str(&re).unwrap();
        assert_eq!(ge2.id, ge.id);
    }
}
