pub mod config;
pub mod opencode_sdk_provider;
pub mod ramalama_provider;
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

/// Sentinel UUID identifying the virtual "ramalama-mini" model config.
///
/// The entry is not stored in `user_model_configs`; it is injected into the
/// config-list API response at runtime when `ramalama_phi4_mini_enabled` is
/// set. Server-side code checks for this sentinel to skip file writes, and
/// the `loadConfigList()` JS filters it out of the Model Configurations tab.
pub const RLML_MINI_SENTINEL_ID: &str = "00000000-0000-0000-0000-00000000dead";

const RESPONSE_FOLLOWS_TOKEN: &str = "<<RESPONSE FOLLOWS>>";

pub async fn generate_conversation_title(
    db: &hiqlite::Client,
    config_root: &std::path::Path,
    harness_config: &HarnessConfig,
    conversation_id: Uuid,
    first_message: &str,
    log_data: bool,
    _footprint: &std::path::Path,
) {
    tracing::info!(
        conversation_id = %conversation_id,
        first_message_len = first_message.len(),
        first_message_preview = %first_message.chars().take(120).collect::<String>(),
        provider_config_ref = %harness_config.provider_config_ref,
        model = ?harness_config.model,
        harness = %harness_config.harness,
        "generate_conversation_title: starting"
    );

    let truncated: String = first_message.chars().take(500).collect();
    tracing::info!(
        conversation_id = %conversation_id,
        truncated_len = truncated.len(),
        "generate_conversation_title: truncated input"
    );

    let title_prompt = format!(
        "Generate a 1-3 word title summarizing this message. Output ONLY the title, nothing else. What follows is context for creating the title: {truncated} {RESPONSE_FOLLOWS_TOKEN}"
    );
    tracing::info!(
        conversation_id = %conversation_id,
        title_prompt_len = title_prompt.len(),
        title_prompt_preview = %title_prompt.chars().take(200).collect::<String>(),
        "generate_conversation_title: built prompt"
    );

    let provider =
        match registry::resolve_provider(harness_config, config_root, log_data, _footprint).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    error = %e,
                    "generate_conversation_title: resolve_provider failed"
                );
                return;
            }
        };
    tracing::info!(
        conversation_id = %conversation_id,
        "generate_conversation_title: provider resolved"
    );

    let model = harness_config.model.as_deref().unwrap_or("default");
    tracing::info!(
        conversation_id = %conversation_id,
        model = %model,
        "generate_conversation_title: calling one_shot_prompt"
    );

    match provider.one_shot_prompt(&title_prompt, model).await {
        Ok(response) => {
            tracing::info!(
                conversation_id = %conversation_id,
                response_len = response.len(),
                response_preview = %response.chars().take(200).collect::<String>(),
                "generate_conversation_title: one_shot_prompt succeeded"
            );

            let split_resp: Vec<&str> = response.split(RESPONSE_FOLLOWS_TOKEN).collect();
            let resp_chunk = split_resp.last().copied().unwrap_or("None");
            tracing::info!(
                conversation_id = %conversation_id,
                split_parts = split_resp.len(),
                resp_chunk = %resp_chunk.chars().take(100).collect::<String>(),
                "generate_conversation_title: after RESPONSE_FOLLOWS split"
            );

            if let Some(title) = sanitize_title(resp_chunk) {
                tracing::info!(
                    conversation_id = %conversation_id,
                    title = %title,
                    "generate_conversation_title: sanitized title, updating DB"
                );
                let _ = db
                    .execute(
                        "UPDATE conversations SET name = $1 WHERE id = $2",
                        hiqlite::params!(title, conversation_id.to_string()),
                    )
                    .await;
            } else {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    resp_chunk = %resp_chunk.chars().take(100).collect::<String>(),
                    "generate_conversation_title: sanitize_title returned None"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                conversation_id = %conversation_id,
                error = %e,
                "generate_conversation_title: one_shot_prompt failed"
            );
        }
    }
}

const MAX_TITLE_LENGTH: usize = 50;

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
    let result = if stripped.len() > MAX_TITLE_LENGTH {
        format!("{}...", &stripped[..MAX_TITLE_LENGTH.saturating_sub(3)])
    } else {
        stripped.to_owned()
    };
    Some(result)
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
    fn test_sanitize_title_truncates_at_max() {
        let long = "a".repeat(60);
        let result = sanitize_title(&long);
        assert_eq!(
            result.as_deref(),
            Some(format!("{}...", "a".repeat(47)).as_str())
        );
    }

    #[test]
    fn test_sanitize_title_at_max_boundary() {
        let exact = "a".repeat(50);
        let result = sanitize_title(&exact);
        assert_eq!(result.as_deref(), Some(exact.as_str()));
    }

    #[test]
    fn test_sanitize_title_single_quoted() {
        assert_eq!(sanitize_title("'Refactor DB'"), Some("Refactor DB".into()));
    }
}
