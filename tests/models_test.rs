use flaresolverr_rs::models::{SolveRequest, SolveResponse};

#[test]
fn test_deserialize_get_request() {
    let json = r#"{"cmd":"request.get","url":"https://example.com","maxTimeout":30000}"#;
    let req: SolveRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.cmd, "request.get");
    assert_eq!(req.url.as_deref(), Some("https://example.com"));
    assert_eq!(req.max_timeout, 30_000);
}

#[test]
fn test_deserialize_post_request() {
    let json = r#"{"cmd":"request.post","url":"https://example.com","postData":"a=1"}"#;
    let req: SolveRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.post_data.as_deref(), Some("a=1"));
    assert_eq!(req.max_timeout, 60_000);
}

#[test]
fn test_deserialize_proxy_empty_object() {
    // Prowlarr sends {} when testing — must not fail
    let json = r#"{"cmd":"sessions.create","proxy":{}}"#;
    let req: SolveRequest = serde_json::from_str(json).unwrap();
    assert!(req.proxy.unwrap().url.is_none());
}

#[test]
fn test_serialize_response_omits_null_solution() {
    let resp = SolveResponse {
        status: "ok".into(),
        message: "".into(),
        solution: None,
        start_timestamp: 1000,
        end_timestamp: 2000,
        version: "0.1.9".into(),
    };
    let s = serde_json::to_string(&resp).unwrap();
    assert!(s.contains(r#""status":"ok""#));
    assert!(s.contains(r#""startTimestamp":1000"#));
    assert!(!s.contains("solution"));
}
