use crate::archive::paths::expand_tilde;
use serde::{Deserialize, Serialize};

// ── YAML config file structs ──────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OfmConfigFile {
    pub server: Option<GroupServer>,
    pub auth: Option<GroupAuth>,
    pub raft: Option<GroupRaft>,
    pub rauthy: Option<GroupRauthy>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupServer {
    #[serde(rename = "HOSTNAME")]
    pub hostname: Option<String>,
    #[serde(rename = "PORT")]
    pub port: Option<u16>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupAuth {
    #[serde(rename = "OIDC_ISSUER_URL")]
    pub oidc_issuer_url: Option<String>,
    #[serde(rename = "OIDC_CLIENT_ID")]
    pub oidc_client_id: Option<String>,
    #[serde(rename = "BASE_URL")]
    pub base_url: Option<String>,
    #[serde(rename = "OIDC_REDIRECT_URI")]
    pub oidc_redirect_uri: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupRaft {
    #[serde(rename = "HIQLITE_RAFT_PORT")]
    pub hiqlite_raft_port: Option<u16>,
    #[serde(rename = "HIQLITE_API_PORT")]
    pub hiqlite_api_port: Option<u16>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupRauthy {
    #[serde(rename = "RAUTHY_ENABLED")]
    pub rauthy_enabled: Option<bool>,
    #[serde(rename = "RAUTHY_PORT")]
    pub rauthy_port: Option<u16>,
}

// ── Main config struct ────────────────────────────────────────────────────

pub struct OfmConfig {
    pub hostname: String,
    pub port: u16,
    pub url: String,
    pub footprint: String,
    pub archive_root: String,
    pub data_dir: String,
    pub api_key: Option<String>,
    pub config_root: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
    pub oidc_client_secret: Option<String>,
    pub base_url: Option<String>,
    pub oidc_redirect_uri: Option<String>,
    pub hiqlite_raft_port: u16,
    pub hiqlite_api_port: u16,
    pub rauthy_enabled: bool,
    pub rauthy_port: u16,
}

const OFM_OIDC_ISSUER_URL: &str = "OFM_OIDC_ISSUER_URL";
const OFM_OIDC_CLIENT_ID: &str = "OFM_OIDC_CLIENT_ID";

impl OfmConfig {
    pub fn auth_enabled(&self) -> bool {
        self.oidc_issuer_url.is_some()
    }

    pub fn from_env() -> Self {
        let footprint_raw = std::env::var("OFM_FOOTPRINT").unwrap_or_else(|_| "~/.ofm".into());
        let footprint = expand_tilde(&footprint_raw);
        let api_key = std::env::var("OFM_API_KEY").ok();
        if let Some(key) = &api_key {
            if key.len() < 16 {
                tracing::warn!("OFM_API_KEY is set but too short (< 16 chars) — auth will be trivially bypassed");
            }
        }
        let hostname = std::env::var("OFM_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("OFM_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3183);
        let base_url = std::env::var("OM_PRINT_BASE_URL").ok();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").ok().or_else(|| {
            base_url
                .as_ref()
                .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
        });
        let url =
            std::env::var("OFM_URL").unwrap_or_else(|_| format!("http://{}:{}", hostname, port));
        Self {
            hostname,
            port,
            url,
            archive_root: format!("{footprint}/archive"),
            data_dir: format!("{footprint}/hiqlite"),
            config_root: format!("{footprint}/config"),
            footprint,
            api_key,
            oidc_issuer_url: std::env::var(OFM_OIDC_ISSUER_URL).ok(),
            oidc_client_id: std::env::var(OFM_OIDC_CLIENT_ID).ok(),
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET").ok(),
            base_url,
            oidc_redirect_uri: redirect_uri,
            hiqlite_raft_port: std::env::var("OFM_HIQLITE_RAFT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8100),
            hiqlite_api_port: std::env::var("OFM_HIQLITE_API_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8200),
            rauthy_enabled: std::env::var("OFM_RAUTHY_ENABLED")
                .ok()
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false),
            rauthy_port: std::env::var("OFM_RAUTHY_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        }
    }

    pub fn load() -> Self {
        let footprint_raw = std::env::var("OFM_FOOTPRINT").unwrap_or_else(|_| "~/.ofm".into());
        let footprint = expand_tilde(&footprint_raw);
        let config_root = format!("{footprint}/config");

        let yaml_path = find_yaml_path(&config_root);
        let yaml_cfg: Option<OfmConfigFile> = yaml_path.as_ref().and_then(|p| {
            let content = std::fs::read_to_string(p).ok()?;
            serde_yaml::from_str(&content).ok()
        });

        let api_key = std::env::var("OFM_API_KEY").ok();
        if let Some(key) = &api_key {
            if key.len() < 16 {
                tracing::warn!("OFM_API_KEY is set but too short (< 16 chars) — auth will be trivially bypassed");
            }
        }

        let hostname = env_opt_or("OFM_HOSTNAME")
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.server.as_ref()?.hostname.clone())
            })
            .unwrap_or_else(|| "127.0.0.1".into());

        let port = std::env::var("OFM_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| yaml_cfg.as_ref().and_then(|y| y.server.as_ref()?.port))
            .unwrap_or(3183u16);

        let url = env_opt_or("OFM_URL")
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.server.as_ref()?.url.clone())
            })
            .unwrap_or_else(|| format!("http://{hostname}:{port}"));

        let base_url = env_opt_or("OM_PRINT_BASE_URL").or_else(|| {
            yaml_cfg
                .as_ref()
                .and_then(|y| y.auth.as_ref()?.base_url.clone())
        });

        let oidc_redirect_uri = env_opt_or("OIDC_REDIRECT_URI")
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.auth.as_ref()?.oidc_redirect_uri.clone())
            })
            .or_else(|| {
                base_url
                    .as_ref()
                    .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
            });

        let oidc_issuer_url = env_opt_or(OFM_OIDC_ISSUER_URL).or_else(|| {
            yaml_cfg
                .as_ref()
                .and_then(|y| y.auth.as_ref()?.oidc_issuer_url.clone())
        });

        let oidc_client_id = env_opt_or(OFM_OIDC_CLIENT_ID).or_else(|| {
            yaml_cfg
                .as_ref()
                .and_then(|y| y.auth.as_ref()?.oidc_client_id.clone())
        });

        let oidc_client_secret = env_opt_or("OIDC_CLIENT_SECRET");

        let hiqlite_raft_port = std::env::var("OFM_HIQLITE_RAFT_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.raft.as_ref()?.hiqlite_raft_port)
            })
            .unwrap_or(8100u16);

        let hiqlite_api_port = std::env::var("OFM_HIQLITE_API_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.raft.as_ref()?.hiqlite_api_port)
            })
            .unwrap_or(8200u16);

        let rauthy_enabled = std::env::var("OFM_RAUTHY_ENABLED")
            .ok()
            .map(|s| s == "true" || s == "1")
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.rauthy.as_ref()?.rauthy_enabled)
            })
            .unwrap_or(false);

        let rauthy_port = std::env::var("OFM_RAUTHY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                yaml_cfg
                    .as_ref()
                    .and_then(|y| y.rauthy.as_ref()?.rauthy_port)
            })
            .unwrap_or(0u16);

        let archive_root = format!("{footprint}/archive");
        let data_dir = format!("{footprint}/hiqlite");

        if yaml_path.is_none() {
            let yaml_out = OfmConfigFile {
                server: Some(GroupServer {
                    hostname: Some(hostname.clone()),
                    port: Some(port),
                    url: Some(url.clone()),
                }),
                auth: Some(GroupAuth {
                    oidc_issuer_url: oidc_issuer_url.clone(),
                    oidc_client_id: oidc_client_id.clone(),
                    base_url: base_url.clone(),
                    oidc_redirect_uri: oidc_redirect_uri.clone(),
                }),
                raft: Some(GroupRaft {
                    hiqlite_raft_port: Some(hiqlite_raft_port),
                    hiqlite_api_port: Some(hiqlite_api_port),
                }),
                rauthy: Some(GroupRauthy {
                    rauthy_enabled: Some(rauthy_enabled),
                    rauthy_port: Some(rauthy_port),
                }),
            };
            let template = generate_yaml_template(&yaml_out);
            let _ = std::fs::create_dir_all(&config_root);
            let _ = std::fs::write(format!("{config_root}/ofm.yml"), &template);
        }

        Self {
            hostname,
            port,
            url,
            footprint,
            archive_root,
            data_dir,
            config_root,
            api_key,
            oidc_issuer_url,
            oidc_client_id,
            oidc_client_secret,
            base_url,
            oidc_redirect_uri,
            hiqlite_raft_port,
            hiqlite_api_port,
            rauthy_enabled,
            rauthy_port,
        }
    }
}

fn env_opt_or(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn find_yaml_path(config_root: &str) -> Option<String> {
    let yml = format!("{config_root}/ofm.yml");
    if std::path::Path::new(&yml).exists() {
        return Some(yml);
    }
    let yaml = format!("{config_root}/ofm.yaml");
    if std::path::Path::new(&yaml).exists() {
        return Some(yaml);
    }
    None
}

fn generate_yaml_template(cfg: &OfmConfigFile) -> String {
    let mut s = String::new();

    s.push_str("# OFM configuration file\n");
    s.push_str("# Environment variables always take precedence over values in this file.\n");
    s.push_str(
        "# This file was auto-generated. You can edit it; changes are preserved on restart.\n",
    );
    s.push_str("#\n");
    s.push_str("# Secrets (API_KEY, RAFT_SECRET, API_SECRET, OIDC_CLIENT_SECRET) are never\n");
    s.push_str("# written to this file \u{2014} they must be set via environment variables.\n\n");

    // server
    s.push_str("server:\n");
    s.push_str("  # IP address or hostname to bind the HTTP server on.\n");
    s.push_str("  # Env: OFM_HOSTNAME (default: 127.0.0.1)\n");
    if let Some(v) = cfg.server.as_ref().and_then(|g| g.hostname.as_ref()) {
        s.push_str(&format!("  HOSTNAME: {v}\n"));
    } else {
        s.push_str("  HOSTNAME: 127.0.0.1\n");
    }
    s.push('\n');

    s.push_str("  # TCP port to bind the HTTP server on.\n");
    s.push_str("  # Env: OFM_PORT (default: 3183)\n");
    if let Some(v) = cfg.server.as_ref().and_then(|g| g.port) {
        s.push_str(&format!("  PORT: {v}\n"));
    } else {
        s.push_str("  PORT: 3183\n");
    }
    s.push('\n');

    s.push_str("  # Public-facing URL (used by the CLI agent subcommand).\n");
    s.push_str("  # Env: OFM_URL (default: http://127.0.0.1:3183)\n");
    if let Some(v) = cfg.server.as_ref().and_then(|g| g.url.as_ref()) {
        s.push_str(&format!("  URL: {v}\n"));
    } else {
        s.push_str("  URL: http://127.0.0.1:3183\n");
    }
    s.push('\n');

    // auth
    s.push_str("auth:\n");
    s.push_str("  # OIDC issuer URL for external authentication.\n");
    s.push_str("  # Env: OFM_OIDC_ISSUER_URL (optional)\n");
    if let Some(v) = cfg.auth.as_ref().and_then(|g| g.oidc_issuer_url.as_ref()) {
        s.push_str(&format!("  OIDC_ISSUER_URL: {v}\n"));
    } else {
        s.push_str("  # OIDC_ISSUER_URL: https://auth.example.com\n");
    }
    s.push('\n');

    s.push_str("  # OIDC client ID registered with the issuer.\n");
    s.push_str("  # Env: OFM_OIDC_CLIENT_ID (optional)\n");
    if let Some(v) = cfg.auth.as_ref().and_then(|g| g.oidc_client_id.as_ref()) {
        s.push_str(&format!("  OIDC_CLIENT_ID: {v}\n"));
    } else {
        s.push_str("  # OIDC_CLIENT_ID: my-client\n");
    }
    s.push('\n');

    s.push_str("  # Base URL for OAuth redirects.\n");
    s.push_str("  # Env: OM_PRINT_BASE_URL (optional)\n");
    if let Some(v) = cfg.auth.as_ref().and_then(|g| g.base_url.as_ref()) {
        s.push_str(&format!("  BASE_URL: {v}\n"));
    } else {
        s.push_str("  # BASE_URL: http://localhost:3183\n");
    }
    s.push('\n');

    s.push_str("  # Explicit OIDC redirect URI. Computed from BASE_URL if not set.\n");
    s.push_str("  # Env: OIDC_REDIRECT_URI (optional)\n");
    if let Some(v) = cfg.auth.as_ref().and_then(|g| g.oidc_redirect_uri.as_ref()) {
        s.push_str(&format!("  OIDC_REDIRECT_URI: {v}\n"));
    } else {
        s.push_str("  # OIDC_REDIRECT_URI: http://localhost:3183/api/auth/callback\n");
    }
    s.push('\n');

    // raft
    s.push_str("raft:\n");
    s.push_str("  # Raft port for hiqlite cluster communication.\n");
    s.push_str("  # Env: OFM_HIQLITE_RAFT_PORT (default: 8100)\n");
    if let Some(v) = cfg.raft.as_ref().and_then(|g| g.hiqlite_raft_port) {
        s.push_str(&format!("  HIQLITE_RAFT_PORT: {v}\n"));
    } else {
        s.push_str("  HIQLITE_RAFT_PORT: 8100\n");
    }
    s.push('\n');

    s.push_str("  # API port for hiqlite client connections.\n");
    s.push_str("  # Env: OFM_HIQLITE_API_PORT (default: 8200)\n");
    if let Some(v) = cfg.raft.as_ref().and_then(|g| g.hiqlite_api_port) {
        s.push_str(&format!("  HIQLITE_API_PORT: {v}\n"));
    } else {
        s.push_str("  HIQLITE_API_PORT: 8200\n");
    }
    s.push('\n');

    // rauthy
    s.push_str("rauthy:\n");
    s.push_str("  # Enable the embedded rauthy OIDC provider.\n");
    s.push_str("  # Env: OFM_RAUTHY_ENABLED (default: false)\n");
    if let Some(v) = cfg.rauthy.as_ref().and_then(|g| g.rauthy_enabled) {
        s.push_str(&format!("  RAUTHY_ENABLED: {v}\n"));
    } else {
        s.push_str("  RAUTHY_ENABLED: false\n");
    }
    s.push('\n');

    s.push_str("  # Port for the embedded rauthy instance (0 = random available port).\n");
    s.push_str("  # Env: OFM_RAUTHY_PORT (default: 0)\n");
    if let Some(v) = cfg.rauthy.as_ref().and_then(|g| g.rauthy_port) {
        s.push_str(&format!("  RAUTHY_PORT: {v}\n"));
    } else {
        s.push_str("  RAUTHY_PORT: 0\n");
    }

    s
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    static ENV_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));

    fn clear_ofm_env() {
        for key in [
            "OFM_HOSTNAME",
            "OFM_PORT",
            "OFM_URL",
            "OFM_FOOTPRINT",
            "OFM_API_KEY",
            OFM_OIDC_ISSUER_URL,
            OFM_OIDC_CLIENT_ID,
            "OIDC_CLIENT_SECRET",
            "OM_PRINT_BASE_URL",
            "OIDC_REDIRECT_URI",
            "OFM_HIQLITE_RAFT_PORT",
            "OFM_HIQLITE_API_PORT",
            "OFM_RAUTHY_ENABLED",
            "OFM_RAUTHY_PORT",
        ] {
            std::env::remove_var(key);
        }
    }

    fn set_ofm_env(kvs: &[(&str, &str)]) {
        clear_ofm_env();
        for (k, v) in kvs {
            std::env::set_var(k, v);
        }
    }

    // ── Existing from_env tests ──────────────────────────────────────────

    #[test]
    fn test_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[]);
        let home = std::env::var("HOME").unwrap();
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.hostname, "127.0.0.1");
        assert_eq!(cfg.port, 3183);
        assert_eq!(cfg.url, "http://127.0.0.1:3183");
        assert_eq!(cfg.footprint, format!("{home}/.ofm"));
        assert_eq!(cfg.archive_root, format!("{home}/.ofm/archive"));
        assert_eq!(cfg.data_dir, format!("{home}/.ofm/hiqlite"));
        assert_eq!(cfg.config_root, format!("{home}/.ofm/config"));
    }

    #[test]
    fn test_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[
            ("OFM_HOSTNAME", "0.0.0.0"),
            ("OFM_PORT", "9090"),
            ("OFM_URL", "http://custom.url:9090"),
            ("OFM_FOOTPRINT", "/tmp/ofm"),
        ]);

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.url, "http://custom.url:9090");
        assert_eq!(cfg.footprint, "/tmp/ofm");
        assert_eq!(cfg.archive_root, "/tmp/ofm/archive");
        assert_eq!(cfg.data_dir, "/tmp/ofm/hiqlite");
        assert_eq!(cfg.config_root, "/tmp/ofm/config");

        clear_ofm_env();
    }

    #[test]
    fn test_oidc_config_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[]);

        let cfg = OfmConfig::from_env();
        assert!(cfg.oidc_issuer_url.is_none());
        assert!(cfg.oidc_client_id.is_none());
    }

    #[test]
    fn test_oidc_config_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[
            (OFM_OIDC_ISSUER_URL, "https://auth.example.com"),
            (OFM_OIDC_CLIENT_ID, "my-client"),
        ]);

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.oidc_issuer_url, Some("https://auth.example.com".into()));
        assert_eq!(cfg.oidc_client_id, Some("my-client".into()));
    }

    #[test]
    fn test_auth_enabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[]);
        let cfg = OfmConfig::from_env();
        assert!(!cfg.auth_enabled());

        set_ofm_env(&[(OFM_OIDC_ISSUER_URL, "https://auth.example.com")]);
        let cfg = OfmConfig::from_env();
        assert!(cfg.auth_enabled());
    }

    #[test]
    fn test_port_invalid_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[("OFM_PORT", "not-a-number")]);

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.port, 3183);
    }

    // ── YAML tests ───────────────────────────────────────────────────────

    #[test]
    fn test_yaml_roundtrip() {
        let server = GroupServer {
            hostname: Some("0.0.0.0".into()),
            port: Some(5500),
            url: Some("http://0.0.0.0:5500".into()),
        };
        let auth = GroupAuth {
            oidc_issuer_url: Some("https://auth.example.com".into()),
            oidc_client_id: Some("test-client".into()),
            base_url: Some("http://localhost:5500".into()),
            oidc_redirect_uri: Some("http://localhost:5500/callback".into()),
        };
        let raft = GroupRaft {
            hiqlite_raft_port: Some(9100),
            hiqlite_api_port: Some(9200),
        };
        let rauthy = GroupRauthy {
            rauthy_enabled: Some(true),
            rauthy_port: Some(4444),
        };
        let cfg = OfmConfigFile {
            server: Some(server),
            auth: Some(auth),
            raft: Some(raft),
            rauthy: Some(rauthy),
        };

        let yaml_str = serde_yaml::to_string(&cfg).unwrap();
        let deserialized: OfmConfigFile = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(cfg, deserialized);
    }

    #[test]
    fn test_yaml_parse_valid() {
        let yaml = r#"
server:
  HOSTNAME: 0.0.0.0
  PORT: 5500
  URL: http://0.0.0.0:5500
auth:
  OIDC_ISSUER_URL: https://auth.example.com
  OIDC_CLIENT_ID: test-client
  BASE_URL: http://localhost:5500
  OIDC_REDIRECT_URI: http://localhost:5500/callback
raft:
  HIQLITE_RAFT_PORT: 9100
  HIQLITE_API_PORT: 9200
rauthy:
  RAUTHY_ENABLED: true
  RAUTHY_PORT: 4444
"#;
        let cfg: OfmConfigFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            cfg.server.as_ref().unwrap().hostname,
            Some("0.0.0.0".into())
        );
        assert_eq!(cfg.server.as_ref().unwrap().port, Some(5500));
        assert_eq!(
            cfg.server.as_ref().unwrap().url,
            Some("http://0.0.0.0:5500".into())
        );
        assert_eq!(
            cfg.auth.as_ref().unwrap().oidc_issuer_url,
            Some("https://auth.example.com".into())
        );
        assert_eq!(
            cfg.auth.as_ref().unwrap().oidc_client_id,
            Some("test-client".into())
        );
        assert_eq!(cfg.raft.as_ref().unwrap().hiqlite_raft_port, Some(9100));
        assert_eq!(cfg.raft.as_ref().unwrap().hiqlite_api_port, Some(9200));
        assert_eq!(cfg.rauthy.as_ref().unwrap().rauthy_enabled, Some(true));
        assert_eq!(cfg.rauthy.as_ref().unwrap().rauthy_port, Some(4444));
    }

    #[test]
    fn test_yaml_parse_empty() {
        let yaml = "{}";
        let cfg: OfmConfigFile = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.server.is_none());
        assert!(cfg.auth.is_none());
        assert!(cfg.raft.is_none());
        assert!(cfg.rauthy.is_none());
    }

    #[test]
    fn test_yaml_parse_partial() {
        let yaml = "server:\n  PORT: 5500\n";
        let cfg: OfmConfigFile = serde_yaml::from_str(yaml).unwrap();
        let server = cfg.server.as_ref().unwrap();
        assert_eq!(server.port, Some(5500));
        assert!(server.hostname.is_none());
        assert!(server.url.is_none());
        assert!(cfg.auth.is_none());
    }

    #[test]
    fn test_env_overrides_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();
        let yaml_path = config_root.join("ofm.yml");

        let yaml = r#"
server:
  HOSTNAME: 0.0.0.0
  PORT: 5500
  URL: http://0.0.0.0:5500
auth:
  OIDC_ISSUER_URL: https://yaml.example.com
raft:
  HIQLITE_RAFT_PORT: 9100
  HIQLITE_API_PORT: 9200
rauthy:
  RAUTHY_ENABLED: true
  RAUTHY_PORT: 4444
"#;
        std::fs::write(&yaml_path, yaml).unwrap();

        set_ofm_env(&[
            ("OFM_FOOTPRINT", dir.path().to_str().unwrap()),
            ("OFM_PORT", "9999"),
            ("OFM_OIDC_ISSUER_URL", "https://env.example.com"),
        ]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.port, 9999);
        assert_eq!(cfg.oidc_issuer_url, Some("https://env.example.com".into()));
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.url, "http://0.0.0.0:5500");
        assert_eq!(cfg.hiqlite_raft_port, 9100);
        assert_eq!(cfg.hiqlite_api_port, 9200);
        assert_eq!(cfg.rauthy_enabled, true);
        assert_eq!(cfg.rauthy_port, 4444);

        clear_ofm_env();
    }

    #[test]
    fn test_yaml_missing_file_creates_it() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        set_ofm_env(&[
            ("OFM_FOOTPRINT", dir.path().to_str().unwrap()),
            ("OFM_HOSTNAME", "0.0.0.0"),
            ("OFM_PORT", "5555"),
        ]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 5555);
        assert_eq!(cfg.url, "http://0.0.0.0:5555");

        let yml_path = dir.path().join("config/ofm.yml");
        assert!(
            yml_path.exists(),
            "YAML config file should have been created"
        );

        let content = std::fs::read_to_string(&yml_path).unwrap();
        assert!(content.contains("HOSTNAME: 0.0.0.0"));
        assert!(content.contains("PORT: 5555"));
        assert!(content.contains("URL: http://0.0.0.0:5555"));

        clear_ofm_env();
    }

    #[test]
    fn test_yaml_invalid_syntax_falls_back_to_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();
        std::fs::write(config_root.join("ofm.yml"), ":::: invalid yaml ::::").unwrap();

        set_ofm_env(&[
            ("OFM_FOOTPRINT", dir.path().to_str().unwrap()),
            ("OFM_HOSTNAME", "1.2.3.4"),
            ("OFM_PORT", "7777"),
        ]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.hostname, "1.2.3.4");
        assert_eq!(cfg.port, 7777);
        assert_eq!(cfg.url, "http://1.2.3.4:7777");

        clear_ofm_env();
    }

    #[test]
    fn test_yaml_yml_vs_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(config_root.join("ofm.yaml"), "server:\n  PORT: 3333\n").unwrap();

        set_ofm_env(&[("OFM_FOOTPRINT", dir.path().to_str().unwrap())]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.port, 3333);

        clear_ofm_env();
    }

    #[test]
    fn test_yaml_yml_preferred_over_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(config_root.join("ofm.yml"), "server:\n  PORT: 1111\n").unwrap();
        std::fs::write(config_root.join("ofm.yaml"), "server:\n  PORT: 2222\n").unwrap();

        set_ofm_env(&[("OFM_FOOTPRINT", dir.path().to_str().unwrap())]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.port, 1111);

        clear_ofm_env();
    }

    #[test]
    fn test_generate_yaml_template_contains_all_sections() {
        let server = GroupServer {
            hostname: Some("0.0.0.0".into()),
            port: Some(5500),
            url: Some("http://0.0.0.0:5500".into()),
        };
        let cfg = OfmConfigFile {
            server: Some(server),
            auth: None,
            raft: None,
            rauthy: None,
        };
        let tpl = generate_yaml_template(&cfg);
        assert!(tpl.contains("server:"));
        assert!(tpl.contains("HOSTNAME: 0.0.0.0"));
        assert!(tpl.contains("PORT: 5500"));
        assert!(tpl.contains("auth:"));
        assert!(tpl.contains("raft:"));
        assert!(tpl.contains("rauthy:"));
        assert!(tpl.contains("OFM_HOSTNAME"));
        assert!(tpl.contains("OFM_PORT"));
        assert!(tpl.contains("OFM_URL"));
    }

    #[test]
    fn test_load_respects_url_from_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(
            config_root.join("ofm.yml"),
            "server:\n  URL: http://custom.url:9999\n",
        )
        .unwrap();

        set_ofm_env(&[
            ("OFM_FOOTPRINT", dir.path().to_str().unwrap()),
            ("OFM_HOSTNAME", "0.0.0.0"),
            ("OFM_PORT", "1234"),
        ]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.url, "http://custom.url:9999");

        clear_ofm_env();
    }

    #[test]
    fn test_load_computes_url_from_hostname_port() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(
            config_root.join("ofm.yml"),
            "server:\n  HOSTNAME: 0.0.0.0\n  PORT: 5555\n",
        )
        .unwrap();

        set_ofm_env(&[("OFM_FOOTPRINT", dir.path().to_str().unwrap())]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.url, "http://0.0.0.0:5555");

        clear_ofm_env();
    }
}
