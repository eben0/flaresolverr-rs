use axum::{Router, routing::post};
use crate::handlers::{solve, AppState};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/v1", post(solve))
        .with_state(state)
}
