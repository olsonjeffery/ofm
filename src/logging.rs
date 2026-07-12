use std::path::PathBuf;
use tracing_subscriber::{fmt, EnvFilter};

pub fn init() {
    init_with_config(None);
}

/// Initialize logging with optional config file for STDOUT logging of chat activity.
///
/// Config file format (JSON):
/// ```json
/// {
///   "stdout_logging": {
///     "enabled": true,
///     "modules": ["ofm::server::routes::conversations", "ofm::server::routes::agent_runs"],
///     "level": "debug"
///   }
/// }
/// ```
pub fn init_with_config(config_path: Option<&PathBuf>) {
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Check if config file exists and load STDOUT logging settings
    if let Some(path) = config_path {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if let Ok(config) = serde_json::from_str::<LoggingConfig>(&content) {
                        if config.stdout_logging.enabled {
                            // Add specific module filters for chat activity
                            for module in &config.stdout_logging.modules {
                                let level = &config.stdout_logging.level;
                                let directive = format!("{}={}", module, level);
                                if let Ok(d) = directive.parse() {
                                    filter = filter.add_directive(d);
                                }
                            }
                            tracing::info!(
                                config_path = %path.display(),
                                modules = ?config.stdout_logging.modules,
                                level = %config.stdout_logging.level,
                                "STDOUT logging enabled for chat activity"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        config_path = %path.display(),
                        error = %e,
                        "Failed to read logging config file"
                    );
                }
            }
        }
    }

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LoggingConfig {
    stdout_logging: StdoutLoggingConfig,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StdoutLoggingConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    modules: Vec<String>,
    #[serde(default = "default_level")]
    level: String,
}

fn default_level() -> String {
    "debug".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_config_serde() {
        let config = LoggingConfig {
            stdout_logging: StdoutLoggingConfig {
                enabled: true,
                modules: vec!["ofm::server::routes::conversations".to_string()],
                level: "debug".to_string(),
            },
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("enabled"));
        assert!(json.contains("conversations"));
    }

    #[test]
    fn test_config_parsing() {
        let json = r#"{
            "stdout_logging": {
                "enabled": true,
                "modules": ["ofm::server::routes::conversations", "ofm::server::routes::agent_runs"],
                "level": "debug"
            }
        }"#;
        let config: LoggingConfig = serde_json::from_str(json).unwrap();
        assert!(config.stdout_logging.enabled);
        assert_eq!(config.stdout_logging.modules.len(), 2);
        assert_eq!(config.stdout_logging.level, "debug");
    }

    #[test]
    fn test_config_defaults() {
        let json = r#"{
            "stdout_logging": {
                "enabled": false
            }
        }"#;
        let config: LoggingConfig = serde_json::from_str(json).unwrap();
        assert!(!config.stdout_logging.enabled);
        assert!(config.stdout_logging.modules.is_empty());
        assert_eq!(config.stdout_logging.level, "debug");
    }
}
