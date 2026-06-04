use axum::{
    routing::{get, post},
    Router,
};

use crate::handlers::{health, index, solve_v1, AppState};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/v1", post(solve_v1))
        .route("/health", get(health))
        .route("/", get(index))
        .with_state(state)
}
