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

/// GET or POST via chaser-cf: solve WAF, then fetch page with clearance cookies.
pub async fn fetch(
    chaser: &ChaserCF,
    url: &str,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    extra_cookies: &[RequestCookie],
) -> Result<Solution> {
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
        .unwrap_or_else(|| "Mozilla/5.0".into());

    let cookie_header = {
        let waf_str = waf.cookies_string();
        let extra: Vec<String> = extra_cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if extra.is_empty() {
            waf_str
        } else {
            format!("{waf_str}; {}", extra.join("; "))
        }
    };

    let mut builder = reqwest::Client::builder().user_agent(&user_agent);
    if let Some(pu) = proxy_url.filter(|p| !p.is_empty()) {
        let proxy = reqwest::Proxy::all(pu)
            .map_err(|e| FlareSolverError::Http(e.to_string()))?;
        builder = builder.proxy(proxy);
    }
    let client = builder
        .build()
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;

    let request = if is_post {
        client
            .post(url)
            .header("Cookie", &cookie_header)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data.unwrap_or("").to_string())
    } else {
        client.get(url).header("Cookie", &cookie_header)
    };

    let response = request
        .send()
        .await
        .map_err(|e| FlareSolverError::Http(e.to_string()))?;
    let final_url = response.url().to_string();
    let status = response.status().as_u16();
    let headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let html = response
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
