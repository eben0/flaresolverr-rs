use std::collections::HashMap;
use futures::StreamExt;
use tokio::time::{timeout, Duration};
use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use chaser_oxide::cdp::browser_protocol::network::CookieParam;

use crate::error::{FlareSolverError, Result};
use crate::models::{RequestCookie, ResponseCookie};

// ── Internal cookie type for unit-testable mapping logic ─────────────────────

pub struct RawCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires: f64, // -1.0 means no expiry (session cookie)
}

pub fn map_cookie(c: RawCookie) -> ResponseCookie {
    ResponseCookie {
        name: c.name,
        value: c.value,
        domain: c.domain,
        path: c.path,
        secure: c.secure,
        http_only: c.http_only,
        expiry: if c.expires < 0.0 { None } else { Some(c.expires) },
        session: c.expires < 0.0,
        same_party: false,
        store_id: "0".into(),
    }
}

// ── PageResult ────────────────────────────────────────────────────────────────

pub struct PageResult {
    pub url: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub html: String,
    pub cookies: Vec<ResponseCookie>,
    pub user_agent: String,
}

// ── BrowserSession ────────────────────────────────────────────────────────────

pub struct BrowserSession {
    browser: Browser,
}

impl BrowserSession {
    pub async fn new(headless: bool) -> Result<Self> {
        let config = if headless {
            BrowserConfig::builder().new_headless_mode().build()
        } else {
            BrowserConfig::builder().with_head().build()
        }
        .map_err(|e| FlareSolverError::Browser(e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        tokio::spawn(async move {
            loop {
                if handler.next().await.is_none() {
                    break;
                }
            }
        });

        Ok(Self { browser })
    }

    pub async fn fetch(
        &self,
        url: &str,
        cookies: &[RequestCookie],
        timeout_ms: u64,
        post_data: Option<&str>,
    ) -> Result<PageResult> {
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let chaser = ChaserPage::new(page);
        let profile = ChaserProfile::native().build();
        chaser
            .apply_profile(&profile)
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        // Inject caller-supplied cookies before navigation.
        if !cookies.is_empty() {
            let params: Vec<CookieParam> = cookies
                .iter()
                .map(|c| CookieParam::new(c.name.clone(), c.value.clone()))
                .collect();
            chaser
                .raw_page()
                .set_cookies(params)
                .await
                .map_err(|e| FlareSolverError::Browser(e.to_string()))?;
        }

        // For POST: use a self-submitting form via JS navigation.
        let nav_url: String = if let Some(data) = post_data {
            let escaped_url = url.replace('"', "\\\"");
            let escaped_data = data.replace('"', "\\\"");
            format!(
                "javascript:(function(){{var f=document.createElement('form');\
                 f.method='POST';f.action=\"{escaped_url}\";\
                 \"{escaped_data}\".split('&').forEach(function(p){{\
                 var kv=p.split('=');var i=document.createElement('input');\
                 i.name=decodeURIComponent(kv[0]||'');\
                 i.value=decodeURIComponent(kv[1]||'');f.appendChild(i);}});\
                 document.body.appendChild(f);f.submit();}})();"
            )
        } else {
            url.to_string()
        };

        timeout(Duration::from_millis(timeout_ms), chaser.goto(&nav_url))
            .await
            .map_err(|_| FlareSolverError::Timeout(timeout_ms))?
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let html = chaser
            .content()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let final_url = chaser
            .url()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?
            .unwrap_or_else(|| url.to_string());

        // Use dedicated user_agent() — safe CDP call, no Runtime.enable leak.
        let user_agent = chaser
            .raw_page()
            .user_agent()
            .await
            .unwrap_or_else(|_| "Mozilla/5.0".into());

        // responseStatus from Navigation Timing API (Chrome 109+).
        let status: u16 = chaser
            .evaluate(
                "window.performance.getEntriesByType('navigation')[0]?.responseStatus || 200",
            )
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?
            .and_then(|v| v.as_u64())
            .map(|n| n as u16)
            .unwrap_or(200);

        let raw_cookies = chaser
            .raw_page()
            .get_cookies()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let mapped_cookies: Vec<ResponseCookie> = raw_cookies
            .into_iter()
            .map(|c| {
                map_cookie(RawCookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain,
                    path: c.path,
                    secure: c.secure,
                    http_only: c.http_only,
                    expires: c.expires,
                })
            })
            .collect();

        Ok(PageResult {
            url: final_url,
            status,
            headers: HashMap::new(),
            html,
            cookies: mapped_cookies,
            user_agent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_cookie_with_expiry() {
        let raw = RawCookie {
            name: "cf_clearance".into(),
            value: "abc123".into(),
            domain: ".bloomberg.com".into(),
            path: "/".into(),
            secure: true,
            http_only: false,
            expires: 1_700_000_000.0,
        };
        let mapped = map_cookie(raw);
        assert_eq!(mapped.name, "cf_clearance");
        assert_eq!(mapped.domain, ".bloomberg.com");
        assert!(mapped.secure);
        assert!(!mapped.session);
        assert_eq!(mapped.expiry, Some(1_700_000_000.0));
        assert_eq!(mapped.store_id, "0");
    }

    #[test]
    fn test_map_cookie_session() {
        let raw = RawCookie {
            name: "sess".into(),
            value: "xyz".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: true,
            expires: -1.0, // no expiry = session cookie
        };
        let mapped = map_cookie(raw);
        assert!(mapped.session);
        assert!(mapped.expiry.is_none());
        assert!(mapped.http_only);
    }
}
