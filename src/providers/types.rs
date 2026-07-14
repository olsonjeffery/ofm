use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    },
    Text {
        text: String,
    },
    TextChunk {
        delta: String,
    },
    ToolUse {
        tool_name: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        input: serde_json::Value,
    },
    ToolResult {
        #[serde(default)]
        tool_use_id: Option<String>,
        result: String,
    },
    Thinking {
        thinking: String,
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
    },
    Done(serde_json::Value),
    QuestionAsked {
        session_id: String,
        question: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        header: Option<String>,
        options: Vec<QuestionOption>,
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
        match self {
            ProviderEvent::SessionStart { session_id } => (
                "session_start".to_string(),
                serde_json::json!({"session_id": session_id}),
            ),
            ProviderEvent::UserText { text } => {
                ("user_text".to_string(), serde_json::json!({"text": text}))
            }
            ProviderEvent::Text { text } => ("text".to_string(), serde_json::json!({"text": text})),
            ProviderEvent::TextChunk { delta } => (
                "text_chunk".to_string(),
                serde_json::json!({"delta": delta}),
            ),
            ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
            } => (
                "tool_use".to_string(),
                serde_json::json!({
                    "tool_name": tool_name,
                    "tool_use_id": tool_use_id,
                    "input": input,
                }),
            ),
            ProviderEvent::ToolResult {
                tool_use_id,
                result,
            } => (
                "tool_result".to_string(),
                serde_json::json!({
                    "tool_use_id": tool_use_id,
                    "result": result,
                }),
            ),
            ProviderEvent::Thinking { thinking } => (
                "thinking".to_string(),
                serde_json::json!({"thinking": thinking}),
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
            ProviderEvent::Error { error } => {
                ("error".to_string(), serde_json::json!({"error": error}))
            }
            ProviderEvent::Done(data) => ("done".to_string(), serde_json::json!({"data": data})),
            ProviderEvent::QuestionAsked {
                question,
                header,
                options,
                ..
            } => (
                "question_asked".to_string(),
                serde_json::json!({
                    "question": question,
                    "header": header,
                    "options": options,
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
    fn test_question_asked_to_ws_event() {
        let event = ProviderEvent::QuestionAsked {
            session_id: "sess-1".into(),
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
        };
        let (event_type, payload) = event.to_ws_event();
        assert_eq!(event_type, "question_asked");
        assert_eq!(payload["question"], "What model?");
        assert_eq!(payload["header"], "Choose");
        assert_eq!(payload["options"].as_array().unwrap().len(), 2);
        assert_eq!(payload["options"][0]["label"], "gpt-4");
        assert_eq!(payload["options"][1]["label"], "claude-3");
    }
}
