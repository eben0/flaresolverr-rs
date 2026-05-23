use std::collections::HashMap;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use futures::{FutureExt, StreamExt};
use tokio::time::{timeout, Duration, Instant};
use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use chaser_oxide::cdp::browser_protocol::network::{CookieParam, EventResponseReceived, ResourceType};
use chaser_oxide::page::ScreenshotParams;

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

// ── FetchRequest (builder) ────────────────────────────────────────────────────

pub struct FetchRequest {
    pub url: String,
    pub post_data: Option<String>,
    pub cookies: Vec<RequestCookie>,
    pub timeout_ms: u64,
    pub screenshot: bool,
}

impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            post_data: None,
            cookies: Vec::new(),
            timeout_ms: 60_000,
            screenshot: false,
        }
    }

    pub fn post(url: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            post_data: Some(data.into()),
            cookies: Vec::new(),
            timeout_ms: 60_000,
            screenshot: false,
        }
    }

    pub fn timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    pub fn cookie(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cookies.push(RequestCookie { name: name.into(), value: value.into() });
        self
    }

    pub fn with_cookies(mut self, cookies: Vec<RequestCookie>) -> Self {
        self.cookies = cookies;
        self
    }

    pub fn screenshot(mut self) -> Self {
        self.screenshot = true;
        self
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
    pub screenshot: Option<String>, // base64-encoded PNG
}

// ── BrowserSession ────────────────────────────────────────────────────────────

pub struct BrowserSession {
    browser: Browser,
    profile: ChaserProfile,
}

impl BrowserSession {
    pub async fn new(headless: bool, proxy: Option<&str>) -> Result<Self> {
        let mut builder = BrowserConfig::builder();
        builder = if headless {
            builder.new_headless_mode()
        } else {
            builder.with_head()
        };
        if let Some(p) = proxy.filter(|s| !s.is_empty()) {
            // Warn if credentials are embedded: they appear in /proc/<pid>/cmdline
            // and are visible to all local processes. Chrome ignores embedded auth
            // in --proxy-server; credentials must be handled via CDP auth events.
            if p.contains('@') {
                tracing::warn!("proxy URL contains credentials — they will be visible in the process list and are NOT forwarded by Chrome; use CDP proxy auth instead");
            }
            builder = builder.arg(format!("--proxy-server={p}"));
        }
        let config = builder.build().map_err(|e| FlareSolverError::Browser(e))?;

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

        Ok(Self { browser, profile: ChaserProfile::native().build() })
    }

    /// Open a tab, run the fetch, then always close the tab — success or error.
    /// Prevents tab accumulation: chromiumoxide does not close pages on Drop.
    pub async fn fetch(&self, req: FetchRequest) -> Result<PageResult> {
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let chaser = ChaserPage::new(page);
        // catch_unwind ensures page.close() runs even if do_fetch panics.
        let result = std::panic::AssertUnwindSafe(self.do_fetch(&chaser, req))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(FlareSolverError::Browser("panic in browser fetch".into())));
        let _ = chaser.raw_page().clone().close().await;
        result
    }

    async fn do_fetch(&self, chaser: &ChaserPage, req: FetchRequest) -> Result<PageResult> {
        chaser
            .apply_profile(&self.profile)
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        // Subscribe to response events BEFORE navigation so we don't miss them.
        let mut response_events = chaser
            .raw_page()
            .event_listener::<EventResponseReceived>()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        // Inject caller-supplied cookies before navigation.
        if !req.cookies.is_empty() {
            let params: Vec<CookieParam> = req.cookies
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
        // URL and body are base64-encoded to eliminate any string-injection risk.
        // Empty-name fields (from empty or missing postData) are skipped via `if(!kv[0])return`.
        let nav_url: String = if let Some(ref data) = req.post_data {
            let b64_url  = B64.encode(req.url.as_bytes());
            let b64_data = B64.encode(data.as_bytes());
            format!(
                "javascript:(function(){{\
                 var u=atob('{b64_url}'),d=atob('{b64_data}');\
                 var f=document.createElement('form');\
                 f.method='POST';f.action=u;\
                 d.split('&').forEach(function(p){{\
                 var kv=p.split('=');\
                 if(!kv[0])return;\
                 var i=document.createElement('input');\
                 i.name=decodeURIComponent(kv[0]);\
                 i.value=decodeURIComponent(kv[1]||'');\
                 f.appendChild(i);}});\
                 document.body.appendChild(f);f.submit();}})();"
            )
        } else {
            req.url.clone()
        };

        timeout(Duration::from_millis(req.timeout_ms), chaser.goto(&nav_url))
            .await
            .map_err(|_| FlareSolverError::Timeout(req.timeout_ms))?
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        // Drain response events to find the main-document response headers.
        // Total deadline prevents SPAs that emit continuous sub-resource events
        // from keeping the loop alive until the outer request timeout.
        // Drop the stream immediately after — frees buffered events from page load.
        let mut headers: HashMap<String, String> = HashMap::new();
        let mut http_status = 200u16;
        let mut found_document = false;
        let drain_deadline = Instant::now() + Duration::from_millis(2_000);
        while !found_document && Instant::now() < drain_deadline {
            match timeout(Duration::from_millis(200), response_events.next()).await {
                Ok(Some(ev)) => {
                    if matches!(ev.r#type, ResourceType::Document) {
                        http_status = ev.response.status as u16;
                        if let Some(obj) = ev.response.headers.inner().as_object() {
                            for (k, v) in obj {
                                headers.insert(
                                    k.to_lowercase(),
                                    v.as_str().unwrap_or("").to_string(),
                                );
                            }
                        }
                        found_document = true;
                    }
                }
                _ => break,
            }
        }
        drop(response_events); // free buffered sub-resource events immediately

        let html = chaser
            .content()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let final_url = chaser
            .url()
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?
            .unwrap_or_else(|| req.url.clone());

        // Use Navigation Timing only when CDP gave us nothing at all (no Document event).
        // Do NOT override a non-200 status captured from CDP — that would turn a real
        // 404 into a 200 on browsers that don't expose responseStatus.
        let status = if !found_document {
            chaser
                .evaluate(
                    "window.performance.getEntriesByType('navigation')[0]?.responseStatus || 200",
                )
                .await
                .map_err(|e| FlareSolverError::Browser(e.to_string()))?
                .and_then(|v| v.as_u64())
                .map(|n| n as u16)
                .unwrap_or(http_status)
        } else {
            http_status
        };

        let user_agent = chaser
            .raw_page()
            .user_agent()
            .await
            .unwrap_or_else(|_| "Mozilla/5.0".into());

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

        let screenshot = if req.screenshot {
            let png = chaser
                .raw_page()
                .screenshot(ScreenshotParams::builder().build())
                .await
                .map_err(|e| FlareSolverError::Browser(e.to_string()))?;
            Some(B64.encode(&png))
        } else {
            None
        };

        Ok(PageResult {
            url: final_url,
            status,
            headers,
            html,
            cookies: mapped_cookies,
            user_agent,
            screenshot,
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
            expires: -1.0,
        };
        let mapped = map_cookie(raw);
        assert!(mapped.session);
        assert!(mapped.expiry.is_none());
        assert!(mapped.http_only);
    }
}
