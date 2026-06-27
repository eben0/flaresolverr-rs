use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{extract::State, Json};

use crate::error::FlareSolverError;
use crate::models::{Solution, SolveRequest, SolveResponse};
use crate::session::SessionStore;
use crate::solver::dispatch;

pub type AppState = Arc<SessionStore>;

pub async fn solve_v1(
    State(store): State<AppState>,
    Json(req): Json<SolveRequest>,
) -> std::result::Result<Json<SolveResponse>, FlareSolverError> {
    let start = unix_ms();
    log_request(&req);

    let proxy_url = req.proxy.as_ref().and_then(|p| p.url.as_deref());
    let (status, message, solution) = dispatch(
        &store,
        &req.cmd,
        req.url.as_deref(),
        req.cmd == "request.post",
        req.post_data.as_deref(),
        proxy_url,
        req.session.as_deref(),
        req.cookies.as_deref().unwrap_or(&[]),
        req.max_timeout,
    )
    .await?;

    Ok(Json(SolveResponse {
        status,
        message,
        solution: solution.map(Solution::from),
        start_timestamp: start,
        end_timestamp: unix_ms(),
        version: env!("CARGO_PKG_VERSION").into(),
    }))
}

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn index() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "msg": "FlareSolverr is ready!",
        "version": env!("CARGO_PKG_VERSION"),
        "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    }))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn log_request(req: &SolveRequest) {
    let url_part = req
        .url
        .as_deref()
        .map(|u| format!(", 'url': '{u}'"))
        .unwrap_or_default();
    let session_part = req
        .session
        .as_deref()
        .map(|s| format!(", 'session': '{s}'"))
        .unwrap_or_default();
    let proxy_part = req
        .proxy
        .as_ref()
        .map(|p| match p.url.as_deref() {
            Some(u) => {
                let redacted = match (u.find("://"), u.find('@')) {
                    (Some(s), Some(a)) if a > s => {
                        format!("{}://***@{}", &u[..s], &u[a + 1..])
                    }
                    _ => u.to_string(),
                };
                format!(", 'proxy': {{'url': '{redacted}'}}")
            }
            None => ", 'proxy': {}".to_string(),
        })
        .unwrap_or_default();
    tracing::info!(
        "Incoming request POST /v1 body: {{'cmd': '{}'{url_part}, 'maxTimeout': {}{session_part}{proxy_part}}}",
        req.cmd,
        req.max_timeout,
    );
}
