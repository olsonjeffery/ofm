use serde_json::Value;

use crate::providers::rig_config::RigProviderConfig;

const INVALID_INPUT_MSG: &str = "input is not valid JSON";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigFormatError {
    #[error("invalid config: {0}")]
    InvalidInput(String),
}

pub fn to_json(input: &str) -> Result<String, ConfigFormatError> {
    let _ = serde_json::from_str::<Value>(input)
        .map_err(|e| ConfigFormatError::InvalidInput(e.to_string()))?;
    Ok(input.to_string())
}

pub fn validate(input: &str) -> Result<(), ConfigFormatError> {
    validate_for_harness(input, "unknown")
}

pub fn validate_for_harness(input: &str, harness: &str) -> Result<(), ConfigFormatError> {
    if harness == "rig" {
        let msg = "config body must be a valid Rig provider config";
        let cfg: RigProviderConfig =
            serde_json::from_str(input).map_err(|_| ConfigFormatError::InvalidInput(msg.into()))?;
        return cfg.validate().map_err(ConfigFormatError::InvalidInput);
    }

    let msg = if harness == "opencode" {
        "config body must be valid JSON for opencode harness"
    } else {
        INVALID_INPUT_MSG
    };
    if serde_json::from_str::<Value>(input).is_err() {
        return Err(ConfigFormatError::InvalidInput(msg.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::rig_config::{ModelListMode, RigVendor};

    #[test]
    fn test_to_json_with_json() {
        let input = r#"{"name": "test"}"#;
        let result = to_json(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_validate_invalid() {
        assert!(validate("{{{").is_err());
    }

    #[test]
    fn test_validate_valid() {
        assert!(validate(r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn test_validate_for_harness_opencode() {
        assert!(validate_for_harness(r#"{"a": 1}"#, "opencode").is_ok());
        assert!(validate_for_harness(r#"42"#, "opencode").is_ok());
        assert!(validate_for_harness(r#""hello""#, "opencode").is_ok());
        assert!(
            validate_for_harness("key: value", "opencode").is_err(),
            "YAML should be rejected for opencode harness"
        );
    }

    #[test]
    fn test_validate_for_harness_unknown() {
        assert!(validate_for_harness(r#"{"a": 1}"#, "unknown").is_ok());
        assert!(validate_for_harness("{{{", "unknown").is_err());
    }

    fn valid_rig_json(vendor: RigVendor) -> String {
        serde_json::json!({
            "name": "test-rig",
            "vendor": vendor,
            "base_url": "https://example.com/v1",
            "api_key": "sk-123",
            "model_list_mode": { "manual": ["gpt-4"] },
            "models": ["gpt-4"]
        })
        .to_string()
    }

    #[test]
    fn test_validate_for_harness_rig_accepts_valid() {
        assert!(validate_for_harness(&valid_rig_json(RigVendor::Anthropic), "rig").is_ok());
        assert!(validate_for_harness(&valid_rig_json(RigVendor::OpenAiCompatible), "rig").is_ok());
    }

    #[test]
    fn test_validate_for_harness_rig_rejects_malformed() {
        // Not JSON at all
        assert!(validate_for_harness("not json", "rig").is_err());
        // Missing required fields
        assert!(validate_for_harness(r#"{"name": "x"}"#, "rig").is_err());
        // Bad vendor tag
        assert!(validate_for_harness(
            r#"{"name":"x","vendor":"bogus","model_list_mode":{"manual":["m"]}}"#,
            "rig"
        )
        .is_err());
        // Manual mode with empty model list
        let bad_manual = serde_json::json!({
            "name": "x",
            "vendor": "openai",
            "api_key": "sk-123",
            "model_list_mode": { "manual": [] },
            "models": []
        })
        .to_string();
        assert!(validate_for_harness(&bad_manual, "rig").is_err());
    }

    #[test]
    fn test_validate_for_harness_rig_rejects_missing_required_fields() {
        // OpenAI-compatible requires a base_url
        let missing_base = serde_json::json!({
            "name": "x",
            "vendor": "open_ai_compatible",
            "api_key": "sk-123",
            "model_list_mode": { "manual": ["m"] },
            "models": ["m"]
        })
        .to_string();
        assert!(validate_for_harness(&missing_base, "rig").is_err());
        // OpenAI (service) requires an api key
        let missing_key = serde_json::json!({
            "name": "x",
            "vendor": "open_ai",
            "api_key": null,
            "model_list_mode": { "manual": ["m"] },
            "models": ["m"]
        })
        .to_string();
        assert!(validate_for_harness(&missing_key, "rig").is_err());
    }

    #[test]
    fn test_validate_for_harness_rig_manual_mode() {
        let cfg: RigProviderConfig =
            serde_json::from_str(&valid_rig_json(RigVendor::OpenAi)).unwrap();
        assert!(matches!(cfg.model_list_mode, ModelListMode::Manual(_)));
    }
}
