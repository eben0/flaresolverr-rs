use flaresolverr_rs::config::FlareSolverConfig;

#[test]
fn test_defaults() {
    let cfg: FlareSolverConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(cfg.host, "0.0.0.0");
    assert_eq!(cfg.port, 8191);
    assert_eq!(cfg.max_timeout_ms, 60_000);
    assert_eq!(cfg.context_limit, 20);
    assert!(cfg.headless);
    assert!(!cfg.virtual_display);
    assert!(cfg.no_sandbox);
}

#[test]
fn test_override() {
    let cfg: FlareSolverConfig =
        serde_json::from_str(r#"{"port":9000,"headless":false,"contextLimit":5}"#).unwrap();
    assert_eq!(cfg.port, 9000);
    assert!(!cfg.headless);
    assert_eq!(cfg.context_limit, 5);
    // unset fields keep defaults
    assert_eq!(cfg.host, "0.0.0.0");
}
