use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderEvent {
    SessionStart {
        session_id: String,
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
    Error {
        error: String,
    },
    Done(serde_json::Value),
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
            _ => None,
        }
    }
}
