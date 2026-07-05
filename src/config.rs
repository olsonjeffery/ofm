pub struct OmprintConfig {
    pub hostname: String,
    pub port: u16,
    pub archive_root: String,
    pub data_dir: String,
    pub api_key: Option<String>,
    pub config_root: String,
    pub oidc_issuer_url: Option<String>,
    pub oidc_client_id: Option<String>,
}

impl OmprintConfig {
    pub fn auth_enabled(&self) -> bool {
        self.oidc_issuer_url.is_some()
    }

    pub fn from_env() -> Self {
        let db_path = std::env::var("OMPRINT_DB_PATH").unwrap_or_else(|_| "data/omprint.db".into());
        let data_dir = std::path::Path::new(&db_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "data".into());
        let api_key = std::env::var("OMPRINT_API_KEY").ok();
        if let Some(key) = &api_key {
            if key.is_empty() || key.len() < 16 {
                tracing::warn!("OMPRINT_API_KEY is set but too short (< 16 chars) — auth will be trivially bypassed");
            }
        }
        let config_root = std::env::var("OMPRINT_CONFIG").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.config/omprint")
        });
        Self {
            hostname: std::env::var("OMPRINT_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3183),
            archive_root: std::env::var("OMPRINT_ARCHIVE_ROOT")
                .unwrap_or_else(|_| "storage/".into()),
            data_dir,
            api_key,
            config_root,
            oidc_issuer_url: std::env::var("OIDC_ISSUER_URL").ok(),
            oidc_client_id: std::env::var("OIDC_CLIENT_ID").ok(),
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
        // Save env vars that may be set by CI
        let prev_hostname = std::env::var("OMPRINT_HOSTNAME").ok();
        let prev_port = std::env::var("PORT").ok();
        let prev_archive_root = std::env::var("OMPRINT_ARCHIVE_ROOT").ok();
        let prev_db_path = std::env::var("OMPRINT_DB_PATH").ok();
        std::env::remove_var("OMPRINT_HOSTNAME");
        std::env::remove_var("PORT");
        std::env::remove_var("OMPRINT_ARCHIVE_ROOT");
        std::env::remove_var("OMPRINT_DB_PATH");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "127.0.0.1");
        assert_eq!(cfg.port, 3183);
        assert_eq!(cfg.archive_root, "storage/");
        assert_eq!(cfg.data_dir, "data");

        if let Some(v) = prev_hostname {
            std::env::set_var("OMPRINT_HOSTNAME", v);
        }
        if let Some(v) = prev_port {
            std::env::set_var("PORT", v);
        }
        if let Some(v) = prev_archive_root {
            std::env::set_var("OMPRINT_ARCHIVE_ROOT", v);
        }
        if let Some(v) = prev_db_path {
            std::env::set_var("OMPRINT_DB_PATH", v);
        }
    }

    #[test]
    fn test_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OMPRINT_HOSTNAME", "0.0.0.0");
        std::env::set_var("PORT", "9090");
        std::env::set_var("OMPRINT_ARCHIVE_ROOT", "/tmp/storage/");
        std::env::set_var("OMPRINT_DB_PATH", "/tmp/omprint.db");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.archive_root, "/tmp/storage/");
        assert_eq!(cfg.data_dir, "/tmp");

        std::env::remove_var("OMPRINT_HOSTNAME");
        std::env::remove_var("PORT");
        std::env::remove_var("OMPRINT_ARCHIVE_ROOT");
        std::env::remove_var("OMPRINT_DB_PATH");
    }

    #[test]
    fn test_data_dir_default_uses_parent_of_db_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OMPRINT_DB_PATH", "/some/deep/path/db.sqlite3");
        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.data_dir, "/some/deep/path");
        std::env::remove_var("OMPRINT_DB_PATH");
    }

    #[test]
    fn test_oidc_config_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OIDC_ISSUER_URL");
        std::env::remove_var("OIDC_CLIENT_ID");

        let cfg = OmprintConfig::from_env();
        assert!(cfg.oidc_issuer_url.is_none());
        assert!(cfg.oidc_client_id.is_none());

        std::env::remove_var("OIDC_ISSUER_URL");
        std::env::remove_var("OIDC_CLIENT_ID");
    }

    #[test]
    fn test_oidc_config_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        std::env::set_var("OIDC_CLIENT_ID", "my-client");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.oidc_issuer_url, Some("https://auth.example.com".into()));
        assert_eq!(cfg.oidc_client_id, Some("my-client".into()));

        std::env::remove_var("OIDC_ISSUER_URL");
        std::env::remove_var("OIDC_CLIENT_ID");
    }

    #[test]
    fn test_auth_enabled() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OIDC_ISSUER_URL");
        let cfg = OmprintConfig::from_env();
        assert!(!cfg.auth_enabled());

        std::env::set_var("OIDC_ISSUER_URL", "https://auth.example.com");
        let cfg = OmprintConfig::from_env();
        assert!(cfg.auth_enabled());

        std::env::remove_var("OIDC_ISSUER_URL");
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
