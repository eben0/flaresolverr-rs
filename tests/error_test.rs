use flaresolverr_rs::error::FlareSolverError;

#[test]
fn test_error_display() {
    assert_eq!(
        FlareSolverError::SessionNotFound("abc".into()).to_string(),
        "session not found: abc"
    );
    assert_eq!(
        FlareSolverError::MissingField("url".into()).to_string(),
        "missing required field: url"
    );
    assert_eq!(
        FlareSolverError::UnsupportedCommand("foo".into()).to_string(),
        "unsupported command: foo"
    );
    assert_eq!(
        FlareSolverError::Timeout(5000).to_string(),
        "request timed out after 5000ms"
    );
}
