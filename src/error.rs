use chaser_cf::ChaserError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlareSolverError {
    #[error("browser error: {0}")]
    Browser(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session already exists: {0}")]
    SessionAlreadyExists(String),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("request timed out after {0}ms")]
    Timeout(u64),
    #[error("http error: {0}")]
    Http(String),
}

impl From<ChaserError> for FlareSolverError {
    fn from(e: ChaserError) -> Self {
        match e {
            ChaserError::Timeout(ms) => FlareSolverError::Timeout(ms),
            other => FlareSolverError::Browser(other.to_string()),
        }
    }
}

#[cfg(feature = "server")]
mod axum_impl {
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    };
    use super::FlareSolverError;

    impl IntoResponse for FlareSolverError {
        fn into_response(self) -> Response {
            tracing::error!(error = %self, "request failed");
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
}

pub type Result<T> = std::result::Result<T, FlareSolverError>;
