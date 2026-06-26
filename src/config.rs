pub struct OmprintConfig {
    pub hostname: String,
    pub port: u16,
    pub archive_root: String,
    pub db_path: String,
}

impl OmprintConfig {
    pub fn from_env() -> Self {
        Self {
            hostname: std::env::var("OMPRINT_HOSTNAME").unwrap_or("127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3183),
            archive_root: std::env::var("OMPRINT_ARCHIVE_ROOT")
                .unwrap_or("storage/".into()),
            db_path: std::env::var("OMPRINT_DB_PATH")
                .unwrap_or("data/omprint.db".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "127.0.0.1");
        assert_eq!(cfg.port, 3183);
        assert_eq!(cfg.archive_root, "storage/");
        assert_eq!(cfg.db_path, "data/omprint.db");
    }

    #[test]
    fn test_env_override() {
        std::env::set_var("OMPRINT_HOSTNAME", "0.0.0.0");
        std::env::set_var("PORT", "9090");
        std::env::set_var("OMPRINT_ARCHIVE_ROOT", "/tmp/storage/");
        std::env::set_var("OMPRINT_DB_PATH", "/tmp/omprint.db");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.hostname, "0.0.0.0");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.archive_root, "/tmp/storage/");
        assert_eq!(cfg.db_path, "/tmp/omprint.db");

        std::env::remove_var("OMPRINT_HOSTNAME");
        std::env::remove_var("PORT");
        std::env::remove_var("OMPRINT_ARCHIVE_ROOT");
        std::env::remove_var("OMPRINT_DB_PATH");
    }

    #[test]
    fn test_port_invalid_fallback() {
        std::env::set_var("PORT", "not-a-number");

        let cfg = OmprintConfig::from_env();
        assert_eq!(cfg.port, 3183);

        std::env::remove_var("PORT");
    }
}
