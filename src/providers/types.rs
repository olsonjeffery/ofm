use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskedQuestion {
    pub question: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderEvent {
    SessionStart {
        session_id: String,
    },
    Ready,
    UserText {
        text: String,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    Text {
        text: String,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    TextChunk {
        delta: String,
    },
    ToolUse {
        tool_name: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        input: serde_json::Value,
        #[serde(default)]
        message_id: Option<String>,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    ToolResult {
        #[serde(default)]
        tool_use_id: Option<String>,
        result: String,
        #[serde(default)]
        message_id: Option<String>,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    ThinkingChunk {
        delta: String,
    },
    ContextUsage(serde_json::Value),
    ExtensionUiRequest(serde_json::Value),
    AvailableCommandsUpdate(serde_json::Value),
    Response(serde_json::Value),
    Error {
        error: String,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    Done {
        data: serde_json::Value,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
    QuestionAsked {
        session_id: String,
        questions: Vec<AskedQuestion>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default)]
        timestamp: NaiveDateTime,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TurnInput {
    #[serde(rename = "type")]
    r#type: &'static str,
    pub session_id: Option<String>,
    pub prompt: String,
    pub cwd: String,
    pub model: String,
    pub effort: String,
    pub permission_mode: String,
    pub disallowed_tools: Vec<String>,
    pub models_config: String,
}

impl TurnInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        prompt: String,
        cwd: String,
        model: String,
        effort: String,
        permission_mode: String,
        disallowed_tools: Vec<String>,
        models_config: String,
    ) -> Self {
        Self {
            r#type: "start",
            session_id: None,
            prompt,
            cwd,
            model,
            effort,
            permission_mode,
            disallowed_tools,
            models_config,
        }
    }

    pub fn session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResumeInput {
    #[serde(rename = "type")]
    r#type: &'static str,
    pub session_id: String,
    pub messages: serde_json::Value,
}

impl ResumeInput {
    pub fn new(session_id: String, messages: serde_json::Value) -> Self {
        Self {
            r#type: "resume",
            session_id,
            messages,
        }
    }
}

impl ProviderEvent {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            ProviderEvent::SessionStart { session_id } => Some(session_id),
            ProviderEvent::QuestionAsked { session_id, .. } => Some(session_id),
            _ => None,
        }
    }

    pub fn to_ws_event(&self) -> (String, serde_json::Value) {
        fn ts_string(ts: NaiveDateTime) -> String {
            ts.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        match self {
            ProviderEvent::SessionStart { session_id } => (
                "session_start".to_string(),
                serde_json::json!({"session_id": session_id}),
            ),
            ProviderEvent::UserText {
                text, timestamp, ..
            } => (
                "user_text".to_string(),
                serde_json::json!({"text": text, "timestamp": ts_string(*timestamp)}),
            ),
            ProviderEvent::Text {
                text, timestamp, ..
            } => (
                "text".to_string(),
                serde_json::json!({"text": text, "timestamp": ts_string(*timestamp)}),
            ),
            ProviderEvent::TextChunk { delta } => (
                "text_chunk".to_string(),
                serde_json::json!({"delta": delta}),
            ),
            ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
                message_id,
                timestamp,
            } => (
                "tool_use".to_string(),
                serde_json::json!({
                    "tool_name": tool_name,
                    "tool_use_id": tool_use_id,
                    "input": input,
                    "message_id": message_id,
                    "timestamp": ts_string(*timestamp),
                }),
            ),
            ProviderEvent::ToolResult {
                tool_use_id,
                result,
                message_id,
                timestamp,
            } => (
                "tool_result".to_string(),
                serde_json::json!({
                    "tool_use_id": tool_use_id,
                    "result": result,
                    "message_id": message_id,
                    "timestamp": ts_string(*timestamp),
                }),
            ),
            ProviderEvent::Thinking {
                thinking,
                timestamp,
                ..
            } => (
                "thinking".to_string(),
                serde_json::json!({"thinking": thinking, "timestamp": ts_string(*timestamp)}),
            ),
            ProviderEvent::ThinkingChunk { delta } => (
                "thinking_chunk".to_string(),
                serde_json::json!({"delta": delta}),
            ),
            ProviderEvent::ContextUsage(usage) => (
                "context_usage".to_string(),
                serde_json::json!({"usage": usage}),
            ),
            ProviderEvent::ExtensionUiRequest(data) => {
                ("extension_ui_request".to_string(), data.clone())
            }
            ProviderEvent::AvailableCommandsUpdate(data) => {
                ("available_commands_update".to_string(), data.clone())
            }
            ProviderEvent::Response(data) => ("response".to_string(), data.clone()),
            ProviderEvent::Error {
                error, timestamp, ..
            } => (
                "error".to_string(),
                serde_json::json!({"error": error, "timestamp": ts_string(*timestamp)}),
            ),
            ProviderEvent::Done {
                data, timestamp, ..
            } => (
                "done".to_string(),
                serde_json::json!({"data": data, "timestamp": ts_string(*timestamp)}),
            ),
            ProviderEvent::QuestionAsked {
                questions,
                tool_call_id,
                message_id,
                timestamp,
                ..
            } => (
                "question_asked".to_string(),
                serde_json::json!({
                    "questions": questions,
                    "tool_call_id": tool_call_id,
                    "message_id": message_id,
                    "timestamp": ts_string(*timestamp),
                }),
            ),
            ProviderEvent::Ready => ("ready".to_string(), serde_json::json!({})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_event_without_timestamp_deserializes() {
        let old_json = r#"{"type": "text", "text": "hello"}"#;
        let event: ProviderEvent = serde_json::from_str(old_json).unwrap();
        assert_eq!(
            event,
            ProviderEvent::Text {
                text: "hello".into(),
                timestamp: NaiveDateTime::default(),
            }
        );
    }

    #[test]
    fn test_new_event_with_timestamp_deserializes() {
        let json = r#"{"type": "text", "text": "hello", "timestamp": "2024-01-15T12:00:00"}"#;
        let event: ProviderEvent = serde_json::from_str(json).unwrap();
        let ts = NaiveDateTime::parse_from_str("2024-01-15 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(
            event,
            ProviderEvent::Text {
                text: "hello".into(),
                timestamp: ts,
            }
        );
    }

    #[test]
    fn test_question_asked_to_ws_event() {
        let ts = NaiveDateTime::parse_from_str("2024-01-15 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let event = ProviderEvent::QuestionAsked {
            session_id: "sess-1".into(),
            questions: vec![AskedQuestion {
                question: "What model?".into(),
                header: Some("Choose".into()),
                options: vec![
                    QuestionOption {
                        label: "gpt-4".into(),
                        description: Some("Fast".into()),
                    },
                    QuestionOption {
                        label: "claude-3".into(),
                        description: None,
                    },
                ],
            }],
            tool_call_id: Some("call_123".into()),
            message_id: Some("msg_456".into()),
            timestamp: ts,
        };
        let (event_type, payload) = event.to_ws_event();
        assert_eq!(event_type, "question_asked");
        let qs = payload["questions"].as_array().unwrap();
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0]["question"], "What model?");
        assert_eq!(qs[0]["header"], "Choose");
        assert_eq!(qs[0]["options"].as_array().unwrap().len(), 2);
        assert_eq!(qs[0]["options"][0]["label"], "gpt-4");
        assert_eq!(qs[0]["options"][1]["label"], "claude-3");
        assert_eq!(payload["tool_call_id"], "call_123");
        assert_eq!(payload["message_id"], "msg_456");
    }
}
