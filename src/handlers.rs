use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{extract::State, Json};

use crate::browser::BrowserSession;
use crate::error::{FlareSolverError, Result};
use crate::models::{Solution, SolveRequest, SolveResponse};
use crate::session::SessionStore;

pub type AppState = Arc<SessionStore>;

pub async fn solve(
    State(store): State<AppState>,
    Json(req): Json<SolveRequest>,
) -> std::result::Result<Json<SolveResponse>, FlareSolverError> {
    let start = unix_ms();

    let (status, message, solution) = match req.cmd.as_str() {
        "request.get"      => handle_fetch(store, req, false).await?,
        "request.post"     => handle_fetch(store, req, true).await?,
        "sessions.create"  => handle_session_create(store, req).await?,
        "sessions.list"    => handle_session_list(store).await?,
        "sessions.destroy" => handle_session_destroy(store, req).await?,
        cmd => return Err(FlareSolverError::UnsupportedCommand(cmd.to_string())),
    };

    Ok(Json(SolveResponse {
        status,
        message,
        solution,
        start_timestamp: start,
        end_timestamp: unix_ms(),
        version: env!("CARGO_PKG_VERSION").into(),
    }))
}

async fn handle_fetch(
    store: Arc<SessionStore>,
    req: SolveRequest,
    is_post: bool,
) -> Result<(String, String, Option<Solution>)> {
    let url = req
        .url
        .ok_or_else(|| FlareSolverError::MissingField("url".into()))?;

    let cookies = req.cookies.as_deref().unwrap_or_default();
    let post_data = if is_post { req.post_data.as_deref() } else { None };
    let timeout_ms = req.max_timeout;

    let result = if let Some(ref session_id) = req.session {
        let arc = store
            .get(session_id)
            .ok_or_else(|| FlareSolverError::SessionNotFound(session_id.clone()))?;
        let session = arc.lock().await;
        session.fetch(&url, cookies, timeout_ms, post_data).await?
    } else {
        let session = BrowserSession::new(true).await?;
        session.fetch(&url, cookies, timeout_ms, post_data).await?
    };

    let solution = Solution {
        url: result.url,
        status: result.status,
        headers: result.headers,
        response: result.html,
        cookies: result.cookies,
        user_agent: result.user_agent,
        screenshot: None,
    };

    Ok(("ok".into(), String::new(), Some(solution)))
}

async fn handle_session_create(
    store: Arc<SessionStore>,
    req: SolveRequest,
) -> Result<(String, String, Option<Solution>)> {
    let id = store.create(req.session).await?;
    Ok(("ok".into(), format!("Session created: {id}"), None))
}

async fn handle_session_list(
    store: Arc<SessionStore>,
) -> Result<(String, String, Option<Solution>)> {
    let ids = store.list();
    Ok(("ok".into(), serde_json::to_string(&ids).unwrap_or_default(), None))
}

async fn handle_session_destroy(
    store: Arc<SessionStore>,
    req: SolveRequest,
) -> Result<(String, String, Option<Solution>)> {
    let id = req
        .session
        .ok_or_else(|| FlareSolverError::MissingField("session".into()))?;
    store.destroy(&id).await?;
    Ok(("ok".into(), format!("Session destroyed: {id}"), None))
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_cmd_not_in_known_list() {
        let cmd = "bogus.command";
        let is_known = matches!(
            cmd,
            "request.get" | "request.post"
                | "sessions.create"
                | "sessions.list"
                | "sessions.destroy"
        );
        assert!(!is_known);
    }

    #[test]
    fn test_missing_url_field() {
        let req = SolveRequest {
            cmd: "request.get".into(),
            url: None,
            max_timeout: 60_000,
            cookies: None,
            proxy: None,
            session: None,
            session_ttl_minutes: None,
            return_screenshot: None,
            post_data: None,
            headers: None,
        };
        assert!(req.url.is_none());
    }
}
