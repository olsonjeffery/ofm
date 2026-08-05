use chrono::NaiveDateTime;

/// Machine-readable RFC 3339 UTC value emitted into `data-utc` attributes.
/// Mirrors the DB contract (timestamps are stored and sent as UTC).
pub fn utc_attr(ts: &NaiveDateTime) -> String {
    ts.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utc_attr_standard() {
        let ts = NaiveDateTime::parse_from_str("2024-06-15 14:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(utc_attr(&ts), "2024-06-15T14:30:00Z");
    }

    #[test]
    fn test_utc_attr_midnight() {
        let ts = NaiveDateTime::parse_from_str("2024-06-15 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(utc_attr(&ts), "2024-06-15T00:00:00Z");
    }

    #[test]
    fn test_utc_attr_epoch() {
        let ts = NaiveDateTime::parse_from_str("1970-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(utc_attr(&ts), "1970-01-01T00:00:00Z");
    }
}
