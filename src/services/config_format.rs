use serde_json::Value;

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
    let msg = match harness {
        "opencode" => "config body must be valid JSON for opencode harness",
        _ => INVALID_INPUT_MSG,
    };
    if serde_json::from_str::<Value>(input).is_err() {
        return Err(ConfigFormatError::InvalidInput(msg.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
