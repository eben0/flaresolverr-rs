use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::{extract::State, Json};

use crate::browser::FetchRequest;
use crate::error::{FlareSolverError, Result};
use crate::models::{Solution, SolveRequest, SolveResponse};
use crate::session::SessionStore;

pub type AppState = Arc<SessionStore>;

pub async fn solve(
    State(store): State<AppState>,
    Json(req): Json<SolveRequest>,
) -> std::result::Result<Json<SolveResponse>, FlareSolverError> {
    let start = unix_ms();
    log_incoming_request(&req);

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
    let t0 = Instant::now();

    let url = req
        .url
        .ok_or_else(|| FlareSolverError::MissingField("url".into()))?;

    let proxy_url = req.proxy.as_ref().and_then(|p| p.url.as_deref());

    let fetch_req = {
        let base = if is_post {
            FetchRequest::post(&url, req.post_data.as_deref().unwrap_or(""))
        } else {
            FetchRequest::get(&url)
        };
        let base = base
            .with_cookies(req.cookies.unwrap_or_default())
            .timeout(req.max_timeout);
        if req.return_screenshot.unwrap_or(false) { base.screenshot() } else { base }
    };

    let result = if let Some(ref session_id) = req.session {
        let arc = store
            .get(session_id)
            .ok_or_else(|| FlareSolverError::SessionNotFound(session_id.clone()))?;
        let session = arc.lock().await;
        session.fetch(fetch_req).await?
    } else {
        store.ephemeral_fetch(fetch_req, proxy_url).await?
    };

    let elapsed = t0.elapsed().as_secs_f64();

    let cf_solved = result.cookies.iter().any(|c| c.name == "cf_clearance");
    let cf_challenge_page = result.status == 403
        && (result.html.contains("Just a moment") || result.html.contains("cf-please-wait"));
    // Detect challenge bypass without cf_clearance (e.g. Turnstile mode):
    // the final URL differs from the requested URL and the page loaded successfully.
    let cf_bypassed_without_cookie = !cf_solved
        && !cf_challenge_page
        && result.status == 200
        && result.url != url;
    if cf_solved {
        tracing::info!("Challenge solved!");
    } else if cf_challenge_page {
        tracing::warn!("Challenge detected but not solved! (returned 403 challenge page)");
    } else if cf_bypassed_without_cookie {
        tracing::info!("Challenge bypassed (no cf_clearance — likely Turnstile)");
    } else {
        tracing::info!("Challenge not detected!");
    }
    tracing::info!("Response time: {:.2}s", elapsed);

    let solution = Solution {
        url: result.url,
        status: result.status,
        headers: result.headers,
        response: result.html,
        cookies: result.cookies,
        user_agent: result.user_agent,
        screenshot: result.screenshot,
    };

    Ok(("ok".into(), String::new(), Some(solution)))
}

async fn handle_session_create(
    store: Arc<SessionStore>,
    req: SolveRequest,
) -> Result<(String, String, Option<Solution>)> {
    let proxy_url = req.proxy.as_ref().and_then(|p| p.url.as_deref());
    let id = store.create(req.session, proxy_url).await?;
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

fn log_incoming_request(req: &SolveRequest) {
    // Match Python FlareSolverr's log format:
    // POST /v1 body: {'cmd': '...', 'url': '...', 'maxTimeout': ..., 'session': '...'}
    let url_part = req.url.as_deref().map(|u| format!(", 'url': '{u}'")).unwrap_or_default();
    let session_part = req.session.as_deref().map(|s| format!(", 'session': '{s}'")).unwrap_or_default();
    let proxy_part = req.proxy.as_ref().map(|p| {
        let display = p.url.as_deref().map(|u| {
            // Redact credentials (user:pass@) from the log.
            match (u.find("://"), u.find('@')) {
                (Some(s), Some(a)) if a > s => format!("{}://***@{}", &u[..s], &u[a + 1..]),
                _ => u.to_string(),
            }
        });
        match display {
            Some(d) => format!(", 'proxy': {{'url': '{d}'}}"),
            None    => ", 'proxy': {}".to_string(),
        }
    }).unwrap_or_default();
    tracing::info!(
        "Incoming request POST /v1 body: {{'cmd': '{}'{url_part}, 'maxTimeout': {}{session_part}{proxy_part}}}",
        req.cmd,
        req.max_timeout,
    );
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
