pub struct OmprintConfig {
    pub hostname: String,
    pub port: u16,
    #[allow(dead_code)]
    pub archive_root: String,
    pub data_dir: String,
}

impl OmprintConfig {
    pub fn from_env() -> Self {
        let db_path =
            std::env::var("OMPRINT_DB_PATH").unwrap_or_else(|_| "data/omprint.db".into());
        let data_dir = std::path::Path::new(&db_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "data".into());
        Self {
            hostname: std::env::var("OMPRINT_HOSTNAME").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3183),
            archive_root: std::env::var("OMPRINT_ARCHIVE_ROOT").unwrap_or_else(|_| "storage/".into()),
            data_dir,
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
    fn test_port_invalid_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("PORT", "not-a-number");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.port, 3183);

        std::env::remove_var("PORT");
    }
}
