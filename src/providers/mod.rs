pub mod config;
pub mod opencode_sdk_provider;
pub mod registry;
pub mod types;

use async_trait::async_trait;
use std::path::Path;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::db::schema::ScopeType;
use crate::providers::types::{ProviderEvent, ResumeInput, TurnInput};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError>;

    async fn start(&mut self, working_dir: &Path) -> Result<(), ProviderError>;

    async fn start_turn(
        &self,
        input: TurnInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError>;

    async fn resume_turn(
        &self,
        input: ResumeInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError>;

    async fn abort_turn(&self) -> Result<(), ProviderError>;

    async fn one_shot_prompt(&self, prompt: &str, model: &str) -> Result<String, ProviderError>;

    async fn shutdown(&mut self) -> Result<bool, ProviderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider not started")]
    NotStarted,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("timeout")]
    Timeout,
    #[error("config error: {0}")]
    Config(String),
}

#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub agent_type: String,
    pub harness: String,
    pub provider_config_ref: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub scope: ScopeType,
}

const RESPONSE_FOLLOWS_TOKEN: &str = "<<RESPONSE FOLLOWS>>";

pub async fn generate_conversation_title(
    db: &hiqlite::Client,
    config_root: &std::path::Path,
    harness_config: &HarnessConfig,
    conversation_id: Uuid,
    first_message: &str,
    log_data: bool,
) {
    let truncated: String = first_message.chars().take(500).collect();
    let title_prompt = format!(
        "Generate a 1-3 word title summarizing this message. Output ONLY the title, nothing else. What follows is context for creating the title: {truncated} {RESPONSE_FOLLOWS_TOKEN}"
    );

    let provider = match registry::resolve_provider(harness_config, config_root, log_data).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to create provider for title generation: {e}");
            return;
        }
    };

    let model = harness_config.model.as_deref().unwrap_or("default");
    match provider.one_shot_prompt(&title_prompt, model).await {
        Ok(response) => {
            let split_resp = response.split(RESPONSE_FOLLOWS_TOKEN);
            let resp_chunk = split_resp.last().unwrap_or("None");
            if let Some(title) = sanitize_title(resp_chunk) {
                tracing::info!("Generated conversation title: {title}");
                tracing::info!("full response: {response}");
                let _ = db
                    .execute(
                        "UPDATE conversations SET name = $1 WHERE id = $2",
                        hiqlite::params!(title, conversation_id.to_string()),
                    )
                    .await;
            }
        }
        Err(e) => {
            tracing::warn!("Failed to generate conversation title: {e}");
        }
    }
}

pub fn sanitize_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let stripped = trimmed
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(['.', '!', '?', ',', ';'])
        .trim();
    if stripped.len() < 2 {
        return None;
    }
    Some(stripped.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_title_normal() {
        assert_eq!(sanitize_title("Hello World"), Some("Hello World".into()));
    }

    #[test]
    fn test_sanitize_title_quoted() {
        assert_eq!(
            sanitize_title("\"Implement Auth\""),
            Some("Implement Auth".into())
        );
    }

    #[test]
    fn test_sanitize_title_trailing_punctuation() {
        assert_eq!(sanitize_title("Fix bug."), Some("Fix bug".into()));
    }

    #[test]
    fn test_sanitize_title_trailing_multiple() {
        assert_eq!(
            sanitize_title("Add feature...!"),
            Some("Add feature".into())
        );
    }

    #[test]
    fn test_sanitize_title_too_short() {
        assert_eq!(sanitize_title("A"), None);
    }

    #[test]
    fn test_sanitize_title_empty() {
        assert_eq!(sanitize_title(""), None);
    }

    #[test]
    fn test_sanitize_title_whitespace_only() {
        assert_eq!(sanitize_title("   "), None);
    }

    #[test]
    fn test_sanitize_title_caps_at_50() {
        let long = "a".repeat(60);
        let result = sanitize_title(&long);
        assert_eq!(result.as_deref(), Some("a".repeat(60).as_str()));
    }

    #[test]
    fn test_sanitize_title_single_quoted() {
        assert_eq!(sanitize_title("'Refactor DB'"), Some("Refactor DB".into()));
    }
}
