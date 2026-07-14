use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::ProviderError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub harness: String,
    pub config_ref: String,
    pub raw_snippet: String,
}

pub struct ProviderConfigDir {
    root: PathBuf,
}

impl ProviderConfigDir {
    pub fn new(config_root: &std::path::Path) -> Self {
        Self {
            root: config_root.join("provider-configs"),
        }
    }

    pub fn ensure_exists(&self) -> Result<(), ProviderError> {
        std::fs::create_dir_all(&self.root).map_err(ProviderError::Io)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.root
    }

    pub fn list_configs(&self) -> Result<Vec<String>, ProviderError> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&self.root) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        entries.push(name.to_string());
                    }
                }
            }
        }
        entries.sort();
        Ok(entries)
    }

    pub fn config_path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn load_provider_config(&self, name: &str) -> Result<ProviderConfig, ProviderError> {
        let path = self.config_path(name);
        let raw_snippet =
            std::fs::read_to_string(&path).map_err(|e| ProviderError::Config(e.to_string()))?;
        let raw_snippet = if name.ends_with(".json") {
            trim_json_input(&raw_snippet).to_string()
        } else {
            raw_snippet
        };
        let harness = if name.ends_with(".yaml") || name.ends_with(".yml") {
            "oh-my-pi"
        } else if name.ends_with(".json") {
            "opencode"
        } else {
            return Err(ProviderError::Config(format!(
                "unknown config type for '{name}': expected .json or .yaml/.yml extension"
            )));
        };
        Ok(ProviderConfig {
            harness: harness.to_string(),
            config_ref: name.to_string(),
            raw_snippet,
        })
    }

    pub fn write_provider_config(&self, name: &str, content: &str) -> Result<(), ProviderError> {
        self.ensure_exists()?;
        let path = self.config_path(name);
        std::fs::write(&path, content).map_err(|e| ProviderError::Config(e.to_string()))
    }

    pub fn delete_provider_config(&self, name: &str) -> Result<(), ProviderError> {
        let path = self.config_path(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| ProviderError::Config(e.to_string()))
        } else {
            Ok(())
        }
    }
}

pub fn merge_configs(base: &str, snippet: &ProviderConfig) -> Result<String, ProviderError> {
    match snippet.harness.as_str() {
        "opencode" => merge_json_configs(base, &snippet.raw_snippet),
        "oh-my-pi" => merge_yaml_configs(base, &snippet.raw_snippet),
        other => Err(ProviderError::Config(format!(
            "unsupported harness for config merge: {other}"
        ))),
    }
}

fn trim_json_input(input: &str) -> &str {
    input.trim_start_matches('\u{feff}').trim()
}

fn merge_json_configs(base: &str, overlay: &str) -> Result<String, ProviderError> {
    let mut base_val: serde_json::Value =
        serde_json::from_str(base).map_err(|e| ProviderError::Config(e.to_string()))?;
    let overlay = trim_json_input(overlay);
    let overlay_val: serde_json::Value = serde_json::from_str(overlay).map_err(|e| {
        let preview: String = overlay.chars().take(80).collect();
        ProviderError::Config(format!("{e} — raw content preview: {preview:?}"))
    })?;
    deep_merge(&mut base_val, &overlay_val);
    serde_json::to_string_pretty(&base_val).map_err(|e| ProviderError::Config(e.to_string()))
}

fn merge_yaml_configs(base: &str, overlay: &str) -> Result<String, ProviderError> {
    let mut base_val: serde_yaml::Value =
        serde_yaml::from_str(base).map_err(|e| ProviderError::Config(e.to_string()))?;
    let overlay_val: serde_yaml::Value =
        serde_yaml::from_str(overlay).map_err(|e| ProviderError::Config(e.to_string()))?;
    deep_merge_yaml(&mut base_val, &overlay_val);
    serde_yaml::to_string(&base_val).map_err(|e| ProviderError::Config(e.to_string()))
}

fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

fn deep_merge_yaml(base: &mut serde_yaml::Value, overlay: &serde_yaml::Value) {
    match (base, overlay) {
        (serde_yaml::Value::Mapping(base_map), serde_yaml::Value::Mapping(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge_yaml(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_merge_json_simple() {
        let base = r#"{"key1": "val1", "key2": "val2"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "test.json".into(),
            raw_snippet: r#"{"key2": "overridden"}"#.into(),
        };
        let result = merge_configs(base, &snippet).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["key1"], "val1");
        assert_eq!(v["key2"], "overridden");
    }

    #[test]
    fn test_merge_json_nested() {
        let base = r#"{"outer": {"inner1": "a", "inner2": "b"}}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "test.json".into(),
            raw_snippet: r#"{"outer": {"inner2": "c", "inner3": "d"}}"#.into(),
        };
        let result = merge_configs(base, &snippet).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["outer"]["inner1"], "a");
        assert_eq!(v["outer"]["inner2"], "c");
        assert_eq!(v["outer"]["inner3"], "d");
    }

    #[test]
    fn test_merge_json_empty_snippet() {
        let base = r#"{"key": "val"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "empty.json".into(),
            raw_snippet: "{}".into(),
        };
        let result = merge_configs(base, &snippet).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["key"], "val");
    }

    #[test]
    fn test_merge_yaml_simple() {
        let base = "key1: val1\nkey2: val2\n";
        let snippet = ProviderConfig {
            harness: "oh-my-pi".into(),
            config_ref: "test.yaml".into(),
            raw_snippet: "key2: overridden\n".into(),
        };
        let result = merge_configs(base, &snippet).unwrap();
        assert!(result.contains("val1"));
        assert!(result.contains("overridden"));
    }

    #[test]
    fn test_merge_yaml_nested() {
        let base = "outer:\n  inner1: a\n  inner2: b\n";
        let snippet = ProviderConfig {
            harness: "oh-my-pi".into(),
            config_ref: "test.yaml".into(),
            raw_snippet: "outer:\n  inner2: c\n  inner3: d\n".into(),
        };
        let result = merge_configs(base, &snippet).unwrap();
        let v: serde_yaml::Value = serde_yaml::from_str(&result).unwrap();
        assert_eq!(v["outer"]["inner1"].as_str().unwrap(), "a");
        assert_eq!(v["outer"]["inner2"].as_str().unwrap(), "c");
        assert_eq!(v["outer"]["inner3"].as_str().unwrap(), "d");
    }

    #[test]
    fn test_provider_config_dir_list_and_load() {
        let tmp = TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();
        cfg_dir
            .write_provider_config("test.json", r#"{"model": "claude-3"}"#)
            .unwrap();
        cfg_dir
            .write_provider_config("test.yaml", "model: gpt-4\n")
            .unwrap();
        let configs = cfg_dir.list_configs().unwrap();
        assert_eq!(configs.len(), 2);
        let loaded = cfg_dir.load_provider_config("test.json").unwrap();
        assert_eq!(loaded.harness, "opencode");
        assert!(loaded.raw_snippet.contains("claude-3"));
    }

    #[test]
    fn test_provider_config_dir_unknown_extension() {
        let tmp = TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();
        cfg_dir
            .write_provider_config("test.txt", "some content")
            .unwrap();
        let result = cfg_dir.load_provider_config("test.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_trim_json_input_bom() {
        let input = "\u{feff}{\"key\": \"val\"}";
        assert_eq!(trim_json_input(input), "{\"key\": \"val\"}");
    }

    #[test]
    fn test_trim_json_input_whitespace() {
        let input = "  {\"key\": \"val\"}  \n";
        assert_eq!(trim_json_input(input), "{\"key\": \"val\"}");
    }

    #[test]
    fn test_trim_json_input_bom_with_whitespace() {
        let input = "\u{feff}  {\"key\": \"val\"}  ";
        assert_eq!(trim_json_input(input), "{\"key\": \"val\"}");
    }

    #[test]
    fn test_trim_json_input_noop() {
        let input = "{\"key\": \"val\"}";
        assert_eq!(trim_json_input(input), "{\"key\": \"val\"}");
    }

    #[test]
    fn test_merge_json_trailing_garbage() {
        let base = r#"{"key": "val"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "bad.json".into(),
            raw_snippet: r#"{"key2": "val2"}extra"#.into(),
        };
        let result = merge_configs(base, &snippet);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("raw content preview"),
            "error should contain content preview, got: {err}"
        );
    }

    #[test]
    fn test_merge_json_bom_prefix() {
        let base = r#"{"key": "val"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "bom.json".into(),
            raw_snippet: "\u{feff}{\"key2\": \"val2\"}".into(),
        };
        let result = merge_configs(base, &snippet);
        assert!(
            result.is_ok(),
            "BOM-prefixed JSON should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_load_provider_config_strips_bom() {
        let tmp = TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();
        cfg_dir
            .write_provider_config("bom.json", "\u{feff}{\"model\": \"gpt-4\"}")
            .unwrap();
        let loaded = cfg_dir.load_provider_config("bom.json").unwrap();
        assert_eq!(loaded.raw_snippet, "{\"model\": \"gpt-4\"}");
    }

    #[test]
    fn test_merge_json_bare_string_trailing() {
        let base = r#"{"key": "val"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "bad2.json".into(),
            raw_snippet: r#""hello" more"#.into(),
        };
        let result = merge_configs(base, &snippet);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("raw content preview"),
            "error should contain content preview, got: {err}"
        );
    }

    #[test]
    fn test_merge_json_truly_invalid() {
        let base = r#"{"key": "val"}"#;
        let snippet = ProviderConfig {
            harness: "opencode".into(),
            config_ref: "totally_bad.json".into(),
            raw_snippet: "{{{".into(),
        };
        let result = merge_configs(base, &snippet);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("raw content preview"),
            "error should contain content preview, got: {err}"
        );
    }
}
