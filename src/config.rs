use crate::archive::paths::expand_tilde;

pub struct OfmConfig {
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
        let base_url = std::env::var("OM_PRINT_BASE_URL").ok();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").ok().or_else(|| {
            base_url
                .as_ref()
                .map(|base| format!("{}/api/auth/callback", base.trim_end_matches('/')))
        });
        Self {
            hostname: std::env::var("OFM_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("OFM_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3183),
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
        let prev_hostname = std::env::var("OFM_HOSTNAME").ok();
        let prev_port = std::env::var("OFM_PORT").ok();
        let prev_footprint = std::env::var("OFM_FOOTPRINT").ok();
        std::env::remove_var("OFM_HOSTNAME");
        std::env::remove_var("OFM_PORT");
        std::env::remove_var("OFM_FOOTPRINT");

        let home = std::env::var("HOME").unwrap();
        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.hostname, "127.0.0.1");
        assert_eq!(cfg.port, 3183);
        assert_eq!(cfg.footprint, format!("{home}/.ofm"));
        assert_eq!(cfg.archive_root, format!("{home}/.ofm/archive"));
        assert_eq!(cfg.data_dir, format!("{home}/.ofm/hiqlite"));
        assert_eq!(cfg.config_root, format!("{home}/.ofm/config"));

        if let Some(v) = prev_hostname {
            std::env::set_var("OFM_HOSTNAME", v);
        }
        if let Some(v) = prev_port {
            std::env::set_var("OFM_PORT", v);
        }
        if let Some(v) = prev_footprint {
            std::env::set_var("OFM_FOOTPRINT", v);
        }
    }

    #[test]
    fn test_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OFM_HOSTNAME", "0.0.0.0");
        std::env::set_var("OFM_PORT", "9090");
        std::env::set_var("OFM_FOOTPRINT", "/tmp/ofm");

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.footprint, "/tmp/ofm");
        assert_eq!(cfg.archive_root, "/tmp/ofm/archive");
        assert_eq!(cfg.data_dir, "/tmp/ofm/hiqlite");
        assert_eq!(cfg.config_root, "/tmp/ofm/config");

        std::env::remove_var("OFM_HOSTNAME");
        std::env::remove_var("OFM_PORT");
        std::env::remove_var("OFM_FOOTPRINT");
    }

    #[test]
    fn test_oidc_config_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(OFM_OIDC_ISSUER_URL);
        std::env::remove_var(OFM_OIDC_CLIENT_ID);

        let cfg = OfmConfig::from_env();
        assert!(cfg.oidc_issuer_url.is_none());
        assert!(cfg.oidc_client_id.is_none());

        std::env::remove_var(OFM_OIDC_ISSUER_URL);
        std::env::remove_var(OFM_OIDC_CLIENT_ID);
    }

    #[test]
    fn test_oidc_config_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(OFM_OIDC_ISSUER_URL, "https://auth.example.com");
        std::env::set_var(OFM_OIDC_CLIENT_ID, "my-client");

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.oidc_issuer_url, Some("https://auth.example.com".into()));
        assert_eq!(cfg.oidc_client_id, Some("my-client".into()));

        std::env::remove_var(OFM_OIDC_ISSUER_URL);
        std::env::remove_var(OFM_OIDC_CLIENT_ID);
    }

    #[test]
    fn test_auth_enabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(OFM_OIDC_ISSUER_URL);
        let cfg = OfmConfig::from_env();
        assert!(!cfg.auth_enabled());

        std::env::set_var(OFM_OIDC_ISSUER_URL, "https://auth.example.com");
        let cfg = OfmConfig::from_env();
        assert!(cfg.auth_enabled());

        std::env::remove_var(OFM_OIDC_ISSUER_URL);
    }

    #[test]
    fn test_port_invalid_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OFM_PORT", "not-a-number");

        let cfg = OfmConfig::from_env();
        assert_eq!(cfg.port, 3183);

        std::env::remove_var("OFM_PORT");
    }
}
