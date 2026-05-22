use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlareSolverError {
    #[error("browser error: {0}")]
    Browser(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("request timed out after {0}ms")]
    Timeout(u64),
    #[error("config error: {0}")]
    Config(String),
}

impl IntoResponse for FlareSolverError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "status": "error",
            "message": self.to_string(),
            "solution": null,
            "startTimestamp": 0u64,
            "endTimestamp": 0u64,
            "version": env!("CARGO_PKG_VERSION"),
        });
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, FlareSolverError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
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
    }
}
