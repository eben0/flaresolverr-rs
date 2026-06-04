use std::collections::HashMap;
use std::sync::Arc;

use chaser_cf::{ChaserCF, Cookie, ProxyConfig};

use crate::error::{FlareSolverError, Result};
use crate::models::{RequestCookie, ResponseCookie, Solution};
use crate::session::SessionStore;

/// Parse "scheme://[user:pass@]host:port" into a chaser_cf ProxyConfig.
pub fn parse_proxy_url(url: &str) -> Result<ProxyConfig> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| FlareSolverError::Browser(format!("invalid proxy URL: {url}")))?;

    let (creds, hostport) = if let Some(at) = rest.rfind('@') {
        (Some(&rest[..at]), &rest[at + 1..])
    } else {
        (None, rest)
    };

    let (host, port_str) = hostport
        .rsplit_once(':')
        .ok_or_else(|| FlareSolverError::Browser(format!("proxy URL missing port: {url}")))?;
    let port = port_str
        .parse::<u16>()
        .map_err(|_| FlareSolverError::Browser(format!("invalid proxy port: {port_str}")))?;

    let mut cfg = ProxyConfig::new(host, port).with_scheme(scheme);
    if let Some(creds) = creds {
        if let Some((user, pass)) = creds.split_once(':') {
            cfg = cfg.with_auth(user, pass);
        }
    }
    Ok(cfg)
}

/// Convert a chaser_cf Cookie into a FlareSolverr ResponseCookie.
pub fn chaser_cookie_to_response(c: Cookie) -> ResponseCookie {
    let is_session = c.expires.is_none();
    ResponseCookie {
        name: c.name,
        value: c.value,
        domain: c.domain.unwrap_or_default(),
        path: c.path.unwrap_or_else(|| "/".into()),
        expiry: c.expires,
        http_only: c.http_only.unwrap_or(false),
        secure: c.secure.unwrap_or(false),
        session: is_session,
        same_party: false,
        store_id: "0".into(),
    }
}

/// Return true if response headers / body indicate a Cloudflare challenge page.
fn is_cf_challenge(status: u16, headers: &HashMap<String, String>, body: &str) -> bool {
    // CF managed challenge: server returns 403 with cf-mitigated header
    if status == 403 && headers.get("cf-mitigated").map(|v| v.as_str()) == Some("challenge") {
        return true;
    }
    // JS challenge or browser check page: 503 or 403 with CF markers in body
    if matches!(status, 403 | 503) {
        let cf_markers = [
            "cf-browser-verification",
            "cf_chl_opt",
            "jschl_vc",
            "cf-please-wait",
            "Checking your browser",
            "__cf_bm",
        ];
        if cf_markers.iter().any(|m| body.contains(m)) {
            return true;
        }
    }
    false
}

/// Build a reqwest client with optional proxy.
fn build_client(
    user_agent: &str,
    proxy_url: Option<&str>,
) -> std::result::Result<reqwest::Client, FlareSolverError> {
    let mut builder = reqwest::Client::builder().user_agent(user_agent);
    if let Some(pu) = proxy_url.filter(|p| !p.is_empty()) {
        let proxy =
            reqwest::Proxy::all(pu).map_err(|e| FlareSolverError::Http(e.to_string()))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| FlareSolverError::Http(e.to_string()))
}

/// GET or POST: try direct reqwest first; fall back to Chrome WAF bypass if CF challenge detected.
pub async fn fetch(
    chaser: &ChaserCF,
    url: &str,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    extra_cookies: &[RequestCookie],
) -> Result<Solution> {
    const DEFAULT_UA: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    let extra_cookie_header: String = extra_cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");

    // ── Pass 1: direct request (fast path for non-CF URLs) ──────────────────
    let client = build_client(DEFAULT_UA, proxy_url)?;
    let direct_req = if is_post {
        client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data.unwrap_or("").to_string())
    } else {
        client.get(url)
    };
    let direct_req = if extra_cookie_header.is_empty() {
        direct_req
    } else {
        direct_req.header("Cookie", &extra_cookie_header)
    };

    let direct_resp = direct_req
        .send()
        .await
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;

    let direct_status = direct_resp.status().as_u16();
    let direct_headers: HashMap<String, String> = direct_resp
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let direct_final_url = direct_resp.url().to_string();
    let direct_body = direct_resp
        .text()
        .await
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;

    if !is_cf_challenge(direct_status, &direct_headers, &direct_body) {
        // Non-CF response — return immediately without touching Chrome.
        return Ok(Solution {
            url: direct_final_url,
            status: direct_status,
            headers: direct_headers,
            response: direct_body,
            cookies: vec![],
            user_agent: DEFAULT_UA.to_string(),
            screenshot: None,
        });
    }

    tracing::info!(url, "CF challenge detected — engaging browser bypass");

    // ── Pass 2: CF challenge — use chaser-cf browser to get clearance ────────
    let chaser_proxy = proxy_url
        .filter(|p| !p.is_empty())
        .map(parse_proxy_url)
        .transpose()?;

    let waf = chaser
        .solve_waf_session(url, chaser_proxy)
        .await
        .map_err(FlareSolverError::from)?;

    let user_agent = waf
        .headers
        .get("user-agent")
        .cloned()
        .unwrap_or_else(|| DEFAULT_UA.to_string());

    let cookie_header = {
        let waf_str = waf.cookies_string();
        if extra_cookie_header.is_empty() {
            waf_str
        } else {
            format!("{waf_str}; {extra_cookie_header}")
        }
    };

    let client2 = build_client(&user_agent, proxy_url)?;
    let request2 = if is_post {
        client2
            .post(url)
            .header("Cookie", &cookie_header)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data.unwrap_or("").to_string())
    } else {
        client2.get(url).header("Cookie", &cookie_header)
    };

    let response2 = request2
        .send()
        .await
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;
    let final_url = response2.url().to_string();
    let status = response2.status().as_u16();
    let headers: HashMap<String, String> = response2
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let html = response2
        .text()
        .await
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;
    let cookies = waf.cookies.into_iter().map(chaser_cookie_to_response).collect();

    Ok(Solution {
        url: final_url,
        status,
        headers,
        response: html,
        cookies,
        user_agent,
        screenshot: None,
    })
}

/// Dispatch a FlareSolverr command.
pub async fn dispatch(
    store: &Arc<SessionStore>,
    cmd: &str,
    url: Option<&str>,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    session_id: Option<&str>,
    extra_cookies: &[RequestCookie],
) -> Result<(String, String, Option<Solution>)> {
    match cmd {
        "request.get" | "request.post" => {
            let url =
                url.ok_or_else(|| FlareSolverError::MissingField("url".into()))?;
            let session_proxy = session_id.and_then(|id| store.registry.get_proxy(id));
            let effective_proxy = session_proxy.as_deref().or(proxy_url);
            let solution = fetch(
                &store.chaser,
                url,
                is_post,
                post_data,
                effective_proxy,
                extra_cookies,
            )
            .await?;
            Ok(("ok".into(), String::new(), Some(solution)))
        }
        "sessions.create" => {
            let id = store
                .registry
                .create(session_id.map(|s| s.to_string()), proxy_url)?;
            Ok(("ok".into(), format!("Session created: {id}"), None))
        }
        "sessions.list" => {
            let ids = store.registry.list();
            Ok((
                "ok".into(),
                serde_json::to_string(&ids).unwrap_or_default(),
                None,
            ))
        }
        "sessions.destroy" => {
            let id =
                session_id.ok_or_else(|| FlareSolverError::MissingField("session".into()))?;
            store.registry.destroy(id)?;
            Ok(("ok".into(), format!("Session destroyed: {id}"), None))
        }
        other => Err(FlareSolverError::UnsupportedCommand(other.to_string())),
    }
}
