use crate::archive::paths::expand_tilde;

pub struct OmprintConfig {
    pub hostname: String,
    pub port: u16,
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

const OMPRINT_OIDC_ISSUER_URL: &str = "OMPRINT_OIDC_ISSUER_URL";
const OMPRINT_OIDC_CLIENT_ID: &str = "OMPRINT_OIDC_CLIENT_ID";

impl OmprintConfig {
    pub fn auth_enabled(&self) -> bool {
        self.oidc_issuer_url.is_some()
    }

    pub fn from_env() -> Self {
        let footprint_raw =
            std::env::var("OMPRINT_FOOTPRINT").unwrap_or_else(|_| "~/.omprint".into());
        let footprint = expand_tilde(&footprint_raw);
        let api_key = std::env::var("OMPRINT_API_KEY").ok();
        if let Some(key) = &api_key {
            if key.len() < 16 {
                tracing::warn!("OMPRINT_API_KEY is set but too short (< 16 chars) — auth will be trivially bypassed");
            }
        }
        let base_url = std::env::var("OM_PRINT_BASE_URL").ok();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").ok().or_else(|| {
            base_url
                .as_ref()
                .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
        });
        Self {
            hostname: std::env::var("OMPRINT_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3183),
            archive_root: format!("{footprint}/archive"),
            data_dir: format!("{footprint}/hiqlite"),
            config_root: format!("{footprint}/config"),
            footprint,
            api_key,
            oidc_issuer_url: std::env::var(OMPRINT_OIDC_ISSUER_URL).ok(),
            oidc_client_id: std::env::var(OMPRINT_OIDC_CLIENT_ID).ok(),
            oidc_client_secret: std::env::var("OIDC_CLIENT_SECRET").ok(),
            base_url,
            oidc_redirect_uri: redirect_uri,
            hiqlite_raft_port: std::env::var("OMPRINT_HIQLITE_RAFT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8100),
            hiqlite_api_port: std::env::var("OMPRINT_HIQLITE_API_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8200),
            rauthy_enabled: std::env::var("RAUTHY_ENABLED")
                .ok()
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false),
            rauthy_port: std::env::var("RAUTHY_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    /// Serializes tests that manipulate env vars to prevent races.
    static ENV_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));

    #[test]
    fn test_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev_hostname = std::env::var("OMPRINT_HOSTNAME").ok();
        let prev_port = std::env::var("PORT").ok();
        let prev_footprint = std::env::var("OMPRINT_FOOTPRINT").ok();
        std::env::remove_var("OMPRINT_HOSTNAME");
        std::env::remove_var("PORT");
        std::env::remove_var("OMPRINT_FOOTPRINT");

        let home = std::env::var("HOME").unwrap();
        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "127.0.0.1");
        assert_eq!(cfg.port, 3183);
        assert_eq!(cfg.footprint, format!("{home}/.omprint"));
        assert_eq!(cfg.archive_root, format!("{home}/.omprint/archive"));
        assert_eq!(cfg.data_dir, format!("{home}/.omprint/hiqlite"));
        assert_eq!(cfg.config_root, format!("{home}/.omprint/config"));

        if let Some(v) = prev_hostname {
            std::env::set_var("OMPRINT_HOSTNAME", v);
        }
        if let Some(v) = prev_port {
            std::env::set_var("PORT", v);
        }
        if let Some(v) = prev_footprint {
            std::env::set_var("OMPRINT_FOOTPRINT", v);
        }
    }

    #[test]
    fn test_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OMPRINT_HOSTNAME", "0.0.0.0");
        std::env::set_var("PORT", "9090");
        std::env::set_var("OMPRINT_FOOTPRINT", "/tmp/omprint");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.footprint, "/tmp/omprint");
        assert_eq!(cfg.archive_root, "/tmp/omprint/archive");
        assert_eq!(cfg.data_dir, "/tmp/omprint/hiqlite");
        assert_eq!(cfg.config_root, "/tmp/omprint/config");

        std::env::remove_var("OMPRINT_HOSTNAME");
        std::env::remove_var("PORT");
        std::env::remove_var("OMPRINT_FOOTPRINT");
    }

    #[test]
    fn test_oidc_config_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(OMPRINT_OIDC_ISSUER_URL);
        std::env::remove_var(OMPRINT_OIDC_CLIENT_ID);

        let cfg = OmprintConfig::from_env();
        assert!(cfg.oidc_issuer_url.is_none());
        assert!(cfg.oidc_client_id.is_none());

        std::env::remove_var(OMPRINT_OIDC_ISSUER_URL);
        std::env::remove_var(OMPRINT_OIDC_CLIENT_ID);
    }

    #[test]
    fn test_oidc_config_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(OMPRINT_OIDC_ISSUER_URL, "https://auth.example.com");
        std::env::set_var(OMPRINT_OIDC_CLIENT_ID, "my-client");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.oidc_issuer_url, Some("https://auth.example.com".into()));
        assert_eq!(cfg.oidc_client_id, Some("my-client".into()));

        std::env::remove_var(OMPRINT_OIDC_ISSUER_URL);
        std::env::remove_var(OMPRINT_OIDC_CLIENT_ID);
    }

    #[test]
    fn test_auth_enabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(OMPRINT_OIDC_ISSUER_URL);
        let cfg = OmprintConfig::from_env();
        assert!(!cfg.auth_enabled());

        std::env::set_var(OMPRINT_OIDC_ISSUER_URL, "https://auth.example.com");
        let cfg = OmprintConfig::from_env();
        assert!(cfg.auth_enabled());

        std::env::remove_var(OMPRINT_OIDC_ISSUER_URL);
    }

    #[test]
    fn test_port_invalid_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("PORT", "not-a-number");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.port, 3183);

        std::env::remove_var("PORT");
    }
}
