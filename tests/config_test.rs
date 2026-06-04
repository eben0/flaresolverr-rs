use flaresolverr_rs::config::FlareSolverConfig;

#[test]
fn test_defaults() {
    let cfg = FlareSolverConfig::default();
    assert!(cfg.headless);
    assert!(!cfg.virtual_display);
    assert!(!cfg.lazy_init);
    assert_eq!(cfg.max_timeout_ms, 60_000);
    assert_eq!(cfg.context_limit, 20);
    assert!(cfg.no_sandbox);
}

#[test]
fn test_deserialize_overrides() {
    let cfg: FlareSolverConfig =
        serde_json::from_str(r#"{"headless":false,"context_limit":5}"#).unwrap();
    assert!(!cfg.headless);
    assert_eq!(cfg.context_limit, 5);
    // unset fields keep defaults
    assert_eq!(cfg.max_timeout_ms, 60_000);
    assert!(cfg.no_sandbox);
}

#[cfg(feature = "server")]
mod server_tests {
    use flaresolverr_rs::config::ServerConfig;

    #[test]
    fn test_server_defaults() {
        let s = ServerConfig::default();
        assert_eq!(s.host, "0.0.0.0");
        assert_eq!(s.port, 8191);
        assert_eq!(s.log_level, "info");
    }

    #[test]
    fn test_server_deserialize() {
        let s: ServerConfig =
            serde_json::from_str(r#"{"port":9000,"log_level":"debug"}"#).unwrap();
        assert_eq!(s.port, 9000);
        assert_eq!(s.log_level, "debug");
        assert_eq!(s.host, "0.0.0.0");
    }
}
