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
    pub behavior: Option<GroupBehavior>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupBehavior {
    #[serde(rename = "INFO_LOG_CLIENT_DATA")]
    log_data: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GroupServer {
    #[serde(rename = "HOSTNAME")]
    pub hostname: Option<String>,
    #[serde(rename = "PORT")]
    pub port: Option<u16>,
    #[serde(rename = "PUB_URL")]
    pub pub_url: Option<String>,
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
    #[serde(rename = "PROXY_MODE")]
    pub proxy_mode: Option<bool>,
    #[serde(rename = "TRUSTED_PROXIES")]
    pub trusted_proxies: Option<String>,
}

// ── Main config struct ────────────────────────────────────────────────────

#[derive(Clone, Default, Debug)]
pub struct OfmConfig {
    pub hostname: String,
    pub port: u16,
    pub pub_url: String,
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
    pub rauthy_proxy_mode: bool,
    pub rauthy_trusted_proxies: Option<String>,
    pub logging_config_path: Option<String>,
    pub info_log_client_data: bool,
}

const OFM_OIDC_ISSUER_URL: &str = "OFM_OIDC_ISSUER_URL";
const OFM_OIDC_CLIENT_ID: &str = "OFM_OIDC_CLIENT_ID";

/// Map a listen hostname to the host agents/loopback can actually connect to.
/// When OFM binds `0.0.0.0` (or an IPv6 wildcard), the connectable host for
/// locally-spawned agents is loopback — the wildcard is not connectable.
fn connectable_host(hostname: &str) -> &str {
    match hostname {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        other => other,
    }
}

/// Resolve the public-facing URL of the deployment. Precedence:
/// `OFM_PUB_URL` → YAML `server.PUB_URL` → legacy `OFM_URL` → legacy YAML
/// `server.URL` → derived `http://{connectable_host}:{port}`.
fn resolve_pub_url(
    env: Option<String>,
    yaml: Option<String>,
    legacy_env: Option<String>,
    legacy_yaml: Option<String>,
    hostname: &str,
    port: u16,
) -> String {
    env.or(yaml)
        .or(legacy_env)
        .or(legacy_yaml)
        .unwrap_or_else(|| format!("http://{}:{}", connectable_host(hostname), port))
}

impl OfmConfig {
    pub fn auth_enabled(&self) -> bool {
        self.oidc_issuer_url.is_some()
    }

    /// The host locally-spawned agents use to reach this OFM instance. Maps
    /// wildcard listen hostnames to loopback so agent-facing URLs never point
    /// at `0.0.0.0`/`::`.
    pub fn agent_host(&self) -> String {
        connectable_host(&self.hostname).to_string()
    }

    pub fn from_env() -> Self {
        let footprint_raw = std::env::var("OFM_FOOTPRINT").unwrap_or_else(|_| "~/.ofm".into());
        let footprint = expand_tilde(&footprint_raw);
        let api_key = std::env::var("OFM_API_KEY").ok();
        warn_if_short_api_key(&api_key);
        let hostname = std::env::var("OFM_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into());
        let port = env_u16("OFM_PORT").unwrap_or(3183);
        let base_url = std::env::var("OM_PRINT_BASE_URL").ok();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").ok().or_else(|| {
            base_url
                .as_ref()
                .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
        });
        let pub_url = resolve_pub_url(
            env_opt_or("OFM_PUB_URL"),
            None,
            env_opt_or("OFM_URL"),
            None,
            &hostname,
            port,
        );
        let url = pub_url.clone();
        let logging_config_path = env_opt_or("OFM_LOGGING_CONFIG");
        Self {
            hostname,
            port,
            pub_url,
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
            hiqlite_raft_port: env_u16("OFM_HIQLITE_RAFT_PORT").unwrap_or(8100),
            hiqlite_api_port: env_u16("OFM_HIQLITE_API_PORT").unwrap_or(8200),
            rauthy_enabled: env_bool("OFM_RAUTHY_ENABLED").unwrap_or(false),
            rauthy_port: env_u16("OFM_RAUTHY_PORT").unwrap_or(0),
            rauthy_proxy_mode: env_bool("OFM_RAUTHY_PROXY_MODE").unwrap_or(false),
            rauthy_trusted_proxies: env_opt_or("OFM_RAUTHY_TRUSTED_PROXIES"),
            logging_config_path,
            info_log_client_data: env_bool("OFM_INFO_LOG_CLIENT_DATA").unwrap_or(false),
        }
    }

    pub fn load() -> Self {
        let footprint_raw = std::env::var("OFM_FOOTPRINT").unwrap_or_else(|_| "~/.ofm".into());
        let footprint = expand_tilde(&footprint_raw);
        let config_root = format!("{footprint}/config");

        let yaml_path = find_yaml_path(&config_root);
        let yaml_cfg: Option<OfmConfigFile> = yaml_path.as_ref().and_then(|p| {
            let content = std::fs::read_to_string(p).ok()?;
            match serde_yaml::from_str(&content) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    // A malformed file (e.g. a typo'd boolean such as `truw`)
                    // must not silently discard the whole config — that would
                    // reset `pub_url`/hostname to loopback defaults and break
                    // login behind a reverse proxy with no visible cause. Log
                    // it loudly so the operator knows the file is being
                    // ignored in favour of env vars / defaults.
                    tracing::error!(
                        "Failed to parse config file '{p}': {e}. Ignoring it and using environment/defaults."
                    );
                    None
                }
            }
        });

        let api_key = std::env::var("OFM_API_KEY").ok();
        warn_if_short_api_key(&api_key);

        let yaml_server = yaml_cfg.as_ref().and_then(|y| y.server.as_ref());
        let yaml_auth = yaml_cfg.as_ref().and_then(|y| y.auth.as_ref());
        let yaml_raft = yaml_cfg.as_ref().and_then(|y| y.raft.as_ref());
        let yaml_rauthy = yaml_cfg.as_ref().and_then(|y| y.rauthy.as_ref());
        let yaml_behavior = yaml_cfg.as_ref().and_then(|y| y.behavior.as_ref());

        let hostname = env_opt_or("OFM_HOSTNAME")
            .or_else(|| yaml_server.and_then(|s| s.hostname.clone()))
            .unwrap_or_else(|| "127.0.0.1".into());

        let port = env_u16("OFM_PORT")
            .or_else(|| yaml_server.and_then(|s| s.port))
            .unwrap_or(3183u16);

        let pub_url = resolve_pub_url(
            env_opt_or("OFM_PUB_URL"),
            yaml_server.and_then(|s| s.pub_url.clone()),
            env_opt_or("OFM_URL"),
            yaml_server.and_then(|s| s.url.clone()),
            &hostname,
            port,
        );
        let url = pub_url.clone();

        let base_url =
            env_opt_or("OM_PRINT_BASE_URL").or_else(|| yaml_auth.and_then(|g| g.base_url.clone()));

        let oidc_redirect_uri = env_opt_or("OIDC_REDIRECT_URI")
            .or_else(|| yaml_auth.and_then(|g| g.oidc_redirect_uri.clone()))
            .or_else(|| {
                base_url
                    .as_ref()
                    .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
            });

        let oidc_issuer_url = env_opt_or(OFM_OIDC_ISSUER_URL)
            .or_else(|| yaml_auth.and_then(|g| g.oidc_issuer_url.clone()));

        let oidc_client_id = env_opt_or(OFM_OIDC_CLIENT_ID)
            .or_else(|| yaml_auth.and_then(|g| g.oidc_client_id.clone()));

        let oidc_client_secret = env_opt_or("OIDC_CLIENT_SECRET");

        let hiqlite_raft_port = env_u16("OFM_HIQLITE_RAFT_PORT")
            .or_else(|| yaml_raft.and_then(|g| g.hiqlite_raft_port))
            .unwrap_or(8100u16);

        let hiqlite_api_port = env_u16("OFM_HIQLITE_API_PORT")
            .or_else(|| yaml_raft.and_then(|g| g.hiqlite_api_port))
            .unwrap_or(8200u16);

        let rauthy_enabled = env_bool("OFM_RAUTHY_ENABLED")
            .or_else(|| yaml_rauthy.and_then(|g| g.rauthy_enabled))
            .unwrap_or(false);

        let rauthy_port = env_u16("OFM_RAUTHY_PORT")
            .or_else(|| yaml_rauthy.and_then(|g| g.rauthy_port))
            .unwrap_or(0u16);

        let rauthy_proxy_mode = env_bool("OFM_RAUTHY_PROXY_MODE")
            .or_else(|| yaml_rauthy.and_then(|g| g.proxy_mode))
            .unwrap_or(false);

        let rauthy_trusted_proxies = env_opt_or("OFM_RAUTHY_TRUSTED_PROXIES")
            .or_else(|| yaml_rauthy.and_then(|g| g.trusted_proxies.clone()));

        let archive_root = format!("{footprint}/archive");
        let data_dir = format!("{footprint}/hiqlite");

        let info_log_client_data =
            env_bool("OFM_INFO_LOG_CLIENT_DATA").or_else(|| yaml_behavior.and_then(|g| g.log_data));

        // Check for logging config file
        let logging_config_path = env_opt_or("OFM_LOGGING_CONFIG").or_else(|| {
            let log_config = format!("{config_root}/logging.json");
            if std::path::Path::new(&log_config).exists() {
                Some(log_config)
            } else {
                None
            }
        });

        if yaml_path.is_none() {
            let yaml_out = OfmConfigFile {
                server: Some(GroupServer {
                    hostname: Some(hostname.clone()),
                    port: Some(port),
                    pub_url: Some(pub_url.clone()),
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
                    proxy_mode: Some(rauthy_proxy_mode),
                    trusted_proxies: rauthy_trusted_proxies.clone(),
                }),
                behavior: Some(GroupBehavior {
                    log_data: info_log_client_data,
                }),
            };
            let template = generate_yaml_template(&yaml_out);
            let _ = std::fs::create_dir_all(&config_root);
            let _ = std::fs::write(format!("{config_root}/ofm.yml"), &template);
        }

        Self {
            hostname,
            port,
            pub_url,
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
            rauthy_proxy_mode,
            rauthy_trusted_proxies,
            logging_config_path,
            info_log_client_data: info_log_client_data.unwrap_or(false),
        }
    }
}

fn env_opt_or(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn env_u16(key: &str) -> Option<u16> {
    std::env::var(key).ok().and_then(|s| s.parse().ok())
}

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key).ok().map(|s| s == "true" || s == "1")
}

fn warn_if_short_api_key(key: &Option<String>) {
    if let Some(key) = key {
        if key.len() < 16 {
            tracing::warn!(
                "OFM_API_KEY is set but too short (< 16 chars) — auth will be trivially bypassed"
            );
        }
    }
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
    use std::fmt::Write;
    let mut s = String::new();

    let _ = writeln!(s, "# OFM configuration file");
    let _ = writeln!(
        s,
        "# Environment variables always take precedence over values in this file."
    );
    let _ = writeln!(
        s,
        "# This file was auto-generated. You can edit it; changes are preserved on restart."
    );
    let _ = writeln!(s, "#");
    let _ = writeln!(
        s,
        "# Secrets (API_KEY, RAFT_SECRET, API_SECRET, OIDC_CLIENT_SECRET) are never"
    );
    let _ = writeln!(
        s,
        "# written to this file \u{2014} they must be set via environment variables."
    );
    s.push('\n');

    let emit = |s: &mut String,
                key: &str,
                comment: &str,
                env: &str,
                default: &str,
                value: Option<String>,
                optional: bool| {
        let _ = writeln!(s, "  # {comment}");
        let _ = writeln!(s, "  # Env: {env} (default: {default})");
        if let Some(v) = value {
            let _ = writeln!(s, "  {key}: {v}");
        } else if optional {
            let _ = writeln!(s, "  # {key}: {default}");
        } else {
            let _ = writeln!(s, "  {key}: {default}");
        }
    };

    let _ = writeln!(s, "server:");
    emit(
        &mut s,
        "HOSTNAME",
        "IP address or hostname to bind the HTTP server on.",
        "OFM_HOSTNAME",
        "127.0.0.1",
        cfg.server.as_ref().and_then(|g| g.hostname.clone()),
        false,
    );
    emit(
        &mut s,
        "PORT",
        "TCP port to bind the HTTP server on.",
        "OFM_PORT",
        "3183",
        cfg.server
            .as_ref()
            .and_then(|g| g.port.map(|v| v.to_string())),
        false,
    );
    emit(
        &mut s,
        "PUB_URL",
        "Public-facing URL of this deployment. Used for all absolute URLs OFM builds\n  (OIDC redirect URI, post-logout URI, rauthy redirect URIs) and for rauthy's own\n  PUB_URL when self-hosting rauthy. Behind a reverse proxy, set this to the\n  externally-visible origin (e.g. https://ofm.example.com).",
        "OFM_PUB_URL",
        "http://127.0.0.1:3183",
        cfg.server.as_ref().and_then(|g| g.pub_url.clone()),
        false,
    );
    emit(
        &mut s,
        "URL",
        "Legacy alias of PUB_URL (used by the CLI agent subcommand). Prefer PUB_URL.",
        "OFM_URL",
        "http://127.0.0.1:3183",
        cfg.server.as_ref().and_then(|g| g.url.clone()),
        false,
    );

    let _ = writeln!(s, "auth:");
    emit(
        &mut s,
        "OIDC_ISSUER_URL",
        "OIDC issuer URL for external authentication.",
        "OFM_OIDC_ISSUER_URL",
        "https://auth.example.com",
        cfg.auth.as_ref().and_then(|g| g.oidc_issuer_url.clone()),
        true,
    );
    emit(
        &mut s,
        "OIDC_CLIENT_ID",
        "OIDC client ID registered with the issuer.",
        "OFM_OIDC_CLIENT_ID",
        "my-client",
        cfg.auth.as_ref().and_then(|g| g.oidc_client_id.clone()),
        true,
    );
    emit(
        &mut s,
        "BASE_URL",
        "Base URL for OAuth redirects.",
        "OM_PRINT_BASE_URL",
        "http://localhost:3183",
        cfg.auth.as_ref().and_then(|g| g.base_url.clone()),
        true,
    );
    emit(
        &mut s,
        "OIDC_REDIRECT_URI",
        "Explicit OIDC redirect URI. Computed from BASE_URL if not set.",
        "OIDC_REDIRECT_URI",
        "http://localhost:3183/api/auth/callback",
        cfg.auth.as_ref().and_then(|g| g.oidc_redirect_uri.clone()),
        true,
    );

    let _ = writeln!(s, "raft:");
    emit(
        &mut s,
        "HIQLITE_RAFT_PORT",
        "Raft port for hiqlite cluster communication.",
        "OFM_HIQLITE_RAFT_PORT",
        "8100",
        cfg.raft
            .as_ref()
            .and_then(|g| g.hiqlite_raft_port.map(|v| v.to_string())),
        false,
    );
    emit(
        &mut s,
        "HIQLITE_API_PORT",
        "API port for hiqlite client connections.",
        "OFM_HIQLITE_API_PORT",
        "8200",
        cfg.raft
            .as_ref()
            .and_then(|g| g.hiqlite_api_port.map(|v| v.to_string())),
        false,
    );

    let _ = writeln!(s, "rauthy:");
    emit(
        &mut s,
        "RAUTHY_ENABLED",
        "Enable the embedded rauthy OIDC provider.",
        "OFM_RAUTHY_ENABLED",
        "false",
        cfg.rauthy
            .as_ref()
            .and_then(|g| g.rauthy_enabled.map(|v| v.to_string())),
        false,
    );
    emit(
        &mut s,
        "RAUTHY_PORT",
        "Port for the embedded rauthy instance (0 = random available port).",
        "OFM_RAUTHY_PORT",
        "0",
        cfg.rauthy
            .as_ref()
            .and_then(|g| g.rauthy_port.map(|v| v.to_string())),
        false,
    );
    emit(
        &mut s,
        "PROXY_MODE",
        "Run rauthy behind a reverse proxy. When true, rauthy blocks every request whose source IP is not in TRUSTED_PROXIES (including health checks) and trusts X-Forwarded-* headers. Defaults to false.",
        "OFM_RAUTHY_PROXY_MODE",
        "false",
        cfg.rauthy
            .as_ref()
            .and_then(|g| g.proxy_mode.map(|v| v.to_string())),
        false,
    );
    emit(
        &mut s,
        "TRUSTED_PROXIES",
        "Newline-separated CIDRs trusted as reverse proxies when PROXY_MODE is true. Only needed for deployments that want real-client-IP hardening.",
        "OFM_RAUTHY_TRUSTED_PROXIES",
        "127.0.0.1/32",
        cfg.rauthy
            .as_ref()
            .and_then(|g| g.trusted_proxies.clone()),
        true,
    );

    let _ = writeln!(s, "behavior:");
    emit(
        &mut s,
        "INFO_LOG_CLIENT_DATA",
        "Log client IP and User-Agent for all requests.",
        "OFM_INFO_LOG_CLIENT_DATA",
        "false",
        cfg.behavior
            .as_ref()
            .and_then(|g| g.log_data.map(|v| v.to_string())),
        false,
    );

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
            "OFM_PUB_URL",
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
            "OFM_RAUTHY_PROXY_MODE",
            "OFM_RAUTHY_TRUSTED_PROXIES",
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
            pub_url: Some("http://ofm.example.com:5500".into()),
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
            proxy_mode: Some(true),
            trusted_proxies: Some("10.0.0.0/8\n127.0.0.1/32".into()),
        };
        let cfg = OfmConfigFile {
            server: Some(server),
            auth: Some(auth),
            raft: Some(raft),
            rauthy: Some(rauthy),
            behavior: Some(GroupBehavior::default()),
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
  PUB_URL: http://ofm.example.com:5500
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
  PROXY_MODE: true
  TRUSTED_PROXIES: "10.0.0.0/8
127.0.0.1/32"
"#;
        let cfg: OfmConfigFile = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            cfg.server.as_ref().unwrap().hostname,
            Some("0.0.0.0".into())
        );
        assert_eq!(cfg.server.as_ref().unwrap().port, Some(5500));
        assert_eq!(
            cfg.server.as_ref().unwrap().pub_url,
            Some("http://ofm.example.com:5500".into())
        );
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
        assert_eq!(cfg.rauthy.as_ref().unwrap().proxy_mode, Some(true));
        assert_eq!(
            cfg.rauthy.as_ref().unwrap().trusted_proxies,
            Some("10.0.0.0/8 127.0.0.1/32".into())
        );
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
        assert!(cfg.rauthy_enabled);
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
        assert_eq!(cfg.url, "http://127.0.0.1:5555");
        assert_eq!(cfg.pub_url, "http://127.0.0.1:5555");

        let yml_path = dir.path().join("config/ofm.yml");
        assert!(
            yml_path.exists(),
            "YAML config file should have been created"
        );

        let content = std::fs::read_to_string(&yml_path).unwrap();
        assert!(content.contains("HOSTNAME: 0.0.0.0"));
        assert!(content.contains("PORT: 5555"));
        assert!(content.contains("PUB_URL: http://127.0.0.1:5555"));

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
            pub_url: Some("http://ofm.example.com:5500".into()),
            url: Some("http://0.0.0.0:5500".into()),
        };
        let cfg = OfmConfigFile {
            server: Some(server),
            auth: None,
            raft: None,
            rauthy: None,
            behavior: None,
        };
        let tpl = generate_yaml_template(&cfg);
        assert!(tpl.contains("server:"));
        assert!(tpl.contains("HOSTNAME: 0.0.0.0"));
        assert!(tpl.contains("PORT: 5500"));
        assert!(tpl.contains("PUB_URL: http://ofm.example.com:5500"));
        assert!(tpl.contains("auth:"));
        assert!(tpl.contains("raft:"));
        assert!(tpl.contains("rauthy:"));
        assert!(tpl.contains("OFM_HOSTNAME"));
        assert!(tpl.contains("OFM_PORT"));
        assert!(tpl.contains("OFM_PUB_URL"));
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
        assert_eq!(cfg.url, "http://127.0.0.1:5555");
        assert_eq!(cfg.pub_url, "http://127.0.0.1:5555");

        clear_ofm_env();
    }

    // ── pub_url resolution ───────────────────────────────────────────────

    #[test]
    fn test_pub_url_defaults_to_http_hostname_port() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.pub_url, "http://127.0.0.1:3183");
        assert_eq!(cfg.url, "http://127.0.0.1:3183");
    }

    #[test]
    fn test_pub_url_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[("OFM_PUB_URL", "https://ofm.example.com")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.pub_url, "https://ofm.example.com");
        assert_eq!(cfg.url, "https://ofm.example.com");
    }

    #[test]
    fn test_pub_url_from_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(
            config_root.join("ofm.yml"),
            "server:\n  PUB_URL: http://yaml.example.com:8080\n",
        )
        .unwrap();

        set_ofm_env(&[("OFM_FOOTPRINT", dir.path().to_str().unwrap())]);

        let cfg = OfmConfig::load();
        assert_eq!(cfg.pub_url, "http://yaml.example.com:8080");

        clear_ofm_env();
    }

    #[test]
    fn test_pub_url_legacy_url_alias() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[("OFM_URL", "http://legacy.example.com:9090")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.pub_url, "http://legacy.example.com:9090");
        assert_eq!(cfg.url, "http://legacy.example.com:9090");
    }

    #[test]
    fn test_pub_url_prefers_pub_url_over_legacy() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[
            ("OFM_PUB_URL", "http://canonical.example.com"),
            ("OFM_URL", "http://legacy.example.com"),
        ]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.pub_url, "http://canonical.example.com");
    }

    #[test]
    fn test_pub_url_0_0_0_0_hostname_defaults_to_127_0_0_1() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[("OFM_HOSTNAME", "0.0.0.0"), ("OFM_PORT", "5555")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.pub_url, "http://127.0.0.1:5555");
    }

    #[test]
    fn test_agent_host_maps_0_0_0_0_to_loopback() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[("OFM_HOSTNAME", "0.0.0.0")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.agent_host(), "127.0.0.1");

        set_ofm_env(&[("OFM_HOSTNAME", "127.0.0.1")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.agent_host(), "127.0.0.1");

        set_ofm_env(&[("OFM_HOSTNAME", "myhost.local")]);
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.agent_host(), "myhost.local");
    }

    // ── rauthy behind-proxy config ───────────────────────────────────────

    #[test]
    fn test_rauthy_proxy_mode_defaults_false() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[]);
        let cfg = OfmConfig::from_env();
        assert!(!cfg.rauthy_proxy_mode);
        assert!(cfg.rauthy_trusted_proxies.is_none());
    }

    #[test]
    fn test_rauthy_trusted_proxies_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_ofm_env(&[
            ("OFM_RAUTHY_PROXY_MODE", "true"),
            ("OFM_RAUTHY_TRUSTED_PROXIES", "10.0.0.0/8\n127.0.0.1/32"),
        ]);
        let cfg = OfmConfig::from_env();
        assert!(cfg.rauthy_proxy_mode);
        assert_eq!(
            cfg.rauthy_trusted_proxies.as_deref(),
            Some("10.0.0.0/8\n127.0.0.1/32")
        );
    }

    #[test]
    fn test_rauthy_proxy_config_from_yaml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config_root = dir.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();

        std::fs::write(
            config_root.join("ofm.yml"),
            "rauthy:\n  PROXY_MODE: true\n  TRUSTED_PROXIES: \"192.168.0.0/16\"\n",
        )
        .unwrap();

        set_ofm_env(&[("OFM_FOOTPRINT", dir.path().to_str().unwrap())]);

        let cfg = OfmConfig::load();
        assert!(cfg.rauthy_proxy_mode);
        assert_eq!(
            cfg.rauthy_trusted_proxies.as_deref(),
            Some("192.168.0.0/16")
        );

        clear_ofm_env();
    }
}
