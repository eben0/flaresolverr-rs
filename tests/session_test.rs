use flaresolverr_rs::session::SessionRegistry;

#[test]
fn test_list_empty() {
    let r = SessionRegistry::new();
    assert!(r.list().is_empty());
}

#[test]
fn test_create_and_list() {
    let r = SessionRegistry::new();
    r.create(Some("my-session".into()), None).unwrap();
    assert_eq!(r.list(), vec!["my-session"]);
}

#[test]
fn test_create_duplicate_fails() {
    let r = SessionRegistry::new();
    r.create(Some("s1".into()), None).unwrap();
    let err = r.create(Some("s1".into()), None);
    assert!(err.unwrap_err().to_string().contains("s1"));
}

#[test]
fn test_destroy_removes_entry() {
    let r = SessionRegistry::new();
    r.create(Some("s2".into()), None).unwrap();
    r.destroy("s2").unwrap();
    assert!(r.list().is_empty());
}

#[test]
fn test_destroy_missing_fails() {
    let r = SessionRegistry::new();
    assert!(r.destroy("nonexistent").is_err());
}

#[test]
fn test_get_proxy_returns_stored_url() {
    let r = SessionRegistry::new();
    r.create(Some("s3".into()), Some("http://proxy:8080")).unwrap();
    assert_eq!(r.get_proxy("s3"), Some("http://proxy:8080".to_string()));
}
