use serde_json::Value;

const INVALID_INPUT_MSG: &str = "input is neither valid JSON nor YAML";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Yaml,
    Json,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigFormatError {
    #[error("invalid config: {0}")]
    InvalidInput(String),
}

pub fn detect_format(input: &str) -> Option<ConfigFormat> {
    if serde_json::from_str::<Value>(input).is_ok() {
        Some(ConfigFormat::Json)
    } else if serde_yaml::from_str::<Value>(input).is_ok() {
        Some(ConfigFormat::Yaml)
    } else {
        None
    }
}

pub fn to_yaml(input: &str) -> Result<String, ConfigFormatError> {
    match detect_format(input) {
        Some(ConfigFormat::Yaml) => Ok(input.to_string()),
        Some(ConfigFormat::Json) => {
            let value: Value = serde_json::from_str(input)
                .map_err(|e| ConfigFormatError::InvalidInput(e.to_string()))?;
            serde_yaml::to_string(&value)
                .map_err(|e| ConfigFormatError::InvalidInput(e.to_string()))
        }
        None => Err(ConfigFormatError::InvalidInput(INVALID_INPUT_MSG.into())),
    }
}

pub fn to_json(input: &str) -> Result<String, ConfigFormatError> {
    match detect_format(input) {
        Some(ConfigFormat::Json) => Ok(input.to_string()),
        Some(ConfigFormat::Yaml) => {
            let value: Value = serde_yaml::from_str(input)
                .map_err(|e| ConfigFormatError::InvalidInput(e.to_string()))?;
            serde_json::to_string_pretty(&value)
                .map_err(|e| ConfigFormatError::InvalidInput(e.to_string()))
        }
        None => Err(ConfigFormatError::InvalidInput(INVALID_INPUT_MSG.into())),
    }
}

pub fn validate(input: &str) -> Result<(), ConfigFormatError> {
    if detect_format(input).is_none() {
        return Err(ConfigFormatError::InvalidInput(INVALID_INPUT_MSG.into()));
    }
    Ok(())
}

pub fn validate_for_harness(input: &str, harness: &str) -> Result<(), ConfigFormatError> {
    match harness {
        "opencode" => {
            if serde_json::from_str::<Value>(input).is_err() {
                return Err(ConfigFormatError::InvalidInput(
                    "config body must be valid JSON for opencode harness".into(),
                ));
            }
        }
        "oh-my-pi" => {
            if serde_yaml::from_str::<Value>(input).is_err() {
                return Err(ConfigFormatError::InvalidInput(
                    "config body must be valid YAML for oh-my-pi harness".into(),
                ));
            }
        }
        _ => {
            // Unknown harness — accept anything that is valid JSON or YAML
            if detect_format(input).is_none() {
                return Err(ConfigFormatError::InvalidInput(INVALID_INPUT_MSG.into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_json() {
        assert_eq!(
            detect_format(r#"{"key": "value"}"#),
            Some(ConfigFormat::Json)
        );
        assert_eq!(detect_format("null"), Some(ConfigFormat::Json));
        assert_eq!(detect_format("42"), Some(ConfigFormat::Json));
    }

    #[test]
    fn test_detect_yaml() {
        assert_eq!(detect_format("key: value"), Some(ConfigFormat::Yaml));
        assert_eq!(
            detect_format("list:\n  - item1\n  - item2"),
            Some(ConfigFormat::Yaml)
        );
    }

    #[test]
    fn test_detect_invalid() {
        assert_eq!(detect_format("{{{"), None);
    }

    #[test]
    fn test_to_yaml_with_json() {
        let input = r#"{"name": "test", "value": 42}"#;
        let result = to_yaml(input).unwrap();
        assert!(result.contains("name: test"));
        assert!(result.contains("value: 42"));
    }

    #[test]
    fn test_to_yaml_with_yaml() {
        let input = "key: value\nnested:\n  inner: 1";
        let result = to_yaml(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_to_json_with_yaml() {
        let input = "name: test\nvalue: 42";
        let result = to_json(input).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["value"], 42);
    }

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
        assert!(validate("key: value").is_ok());
        assert!(validate(r#"{"a": 1}"#).is_ok());
    }

    #[test]
    fn test_validate_for_harness_opencode() {
        assert!(validate_for_harness(r#"{"a": 1}"#, "opencode").is_ok());
        assert!(validate_for_harness(r#"42"#, "opencode").is_ok());
        assert!(validate_for_harness(r#""hello""#, "opencode").is_ok());
        assert!(
            validate_for_harness(r#""provider": {"key": "val"}"#, "opencode").is_err(),
            "bare key-value pair should be rejected for opencode harness"
        );
        assert!(
            validate_for_harness("key: value", "opencode").is_err(),
            "YAML should be rejected for opencode harness"
        );
    }

    #[test]
    fn test_validate_for_harness_oh_my_pi() {
        assert!(validate_for_harness("key: value", "oh-my-pi").is_ok());
        assert!(
            validate_for_harness(r#"{"a": 1}"#, "oh-my-pi").is_ok(),
            "JSON is valid YAML so should be accepted for oh-my-pi harness"
        );
    }

    #[test]
    fn test_validate_for_harness_unknown() {
        assert!(validate_for_harness(r#"{"a": 1}"#, "unknown").is_ok());
        assert!(validate_for_harness("key: value", "unknown").is_ok());
        assert!(validate_for_harness("{{{", "unknown").is_err());
    }
}
