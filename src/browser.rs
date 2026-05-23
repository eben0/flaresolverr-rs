use std::collections::HashMap;
use std::path::PathBuf;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use futures::{FutureExt, StreamExt};
use tokio::time::{timeout, Duration, Instant};
use chaser_oxide::{Browser, BrowserConfig, ChaserPage, ChaserProfile};
use chaser_oxide::cdp::browser_protocol::fetch::{
    EnableParams as FetchEnableParams,
    EventAuthRequired,
    ContinueWithAuthParams,
    AuthChallengeResponse,
    AuthChallengeResponseResponse,
};
use chaser_oxide::cdp::browser_protocol::network::{CookieParam, EventResponseReceived, ResourceType};
use chaser_oxide::cdp::browser_protocol::target::{CreateBrowserContextParams, CreateTargetParams};
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

// ── Proxy credential parsing ──────────────────────────────────────────────────

/// Strips `user:pass@` from a proxy URL and returns the credentials separately.
/// Chrome ignores embedded credentials in `--proxy-server`; they must be
/// forwarded via CDP `Fetch.authRequired` events instead.
fn parse_proxy_credentials(url: &str) -> (Option<(String, String)>, String) {
    let Some(scheme_end) = url.find("://") else {
        return (None, url.to_string());
    };
    let after_scheme = &url[scheme_end + 3..];
    let Some(at_pos) = after_scheme.find('@') else {
        return (None, url.to_string());
    };
    let creds_str = &after_scheme[..at_pos];
    let host_part = &after_scheme[at_pos + 1..];
    let scheme_prefix = &url[..scheme_end + 3];
    let Some(colon_pos) = creds_str.find(':') else {
        return (None, url.to_string());
    };
    let username = creds_str[..colon_pos].to_string();
    let password = creds_str[colon_pos + 1..].to_string();
    (Some((username, password)), format!("{}{}", scheme_prefix, host_part))
}

// ── BrowserSession ────────────────────────────────────────────────────────────

pub struct BrowserSession {
    browser: Browser,
    profile: ChaserProfile,
    data_dir: PathBuf,
    proxy_credentials: Option<(String, String)>,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        // Only remove dirs we created under our own temp prefix.
        let expected_base = std::env::temp_dir().join("flaresolverr-rs");
        if self.data_dir.starts_with(&expected_base) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}

impl BrowserSession {
    pub async fn new(headless: bool, no_sandbox: bool, proxy: Option<&str>) -> Result<Self> {
        let data_dir = std::env::temp_dir()
            .join("flaresolverr-rs")
            .join(uuid::Uuid::new_v4().to_string());
        let mut builder = BrowserConfig::builder().user_data_dir(&data_dir);
        builder = if headless {
            builder.new_headless_mode()
        } else {
            builder.with_head()
        };
        if no_sandbox {
            builder = builder.no_sandbox();
        }
        let proxy_credentials = if let Some(p) = proxy.filter(|s| !s.is_empty()) {
            let (creds, clean_url) = parse_proxy_credentials(p);
            if creds.is_some() {
                tracing::info!(proxy = %clean_url, "browser launched with proxy (credentials forwarded via CDP auth)");
            } else {
                tracing::info!(proxy = %clean_url, "browser launched with proxy");
            }
            builder = builder.arg(format!("--proxy-server={clean_url}"));
            creds
        } else {
            None
        };
        let config = builder.build().map_err(FlareSolverError::Browser)?;

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

        Ok(Self { browser, profile: ChaserProfile::native().build(), data_dir, proxy_credentials })
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

    /// Fetch in an isolated browser context (no new Chrome process).
    ///
    /// Creates a fresh context (isolated cookies/storage/cache), opens a tab,
    /// fetches, closes the tab, then destroys the context. This gives the same
    /// isolation as a fresh browser without the ~500ms Chrome launch overhead.
    pub async fn fetch_isolated(&self, req: FetchRequest) -> Result<PageResult> {
        let ctx_id = self
            .browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let tab_params = CreateTargetParams::builder()
            .url("about:blank")
            .browser_context_id(ctx_id.clone())
            .build()
            .map_err(FlareSolverError::Browser)?;

        let page = self
            .browser
            .new_page(tab_params)
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        let chaser = ChaserPage::new(page);
        let result = std::panic::AssertUnwindSafe(self.do_fetch(&chaser, req))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(FlareSolverError::Browser("panic in browser fetch".into())));
        let _ = chaser.raw_page().clone().close().await;
        // Destroy the context — this discards all cookies/storage/cache for this request.
        let _ = self.browser.dispose_browser_context(ctx_id).await;
        result
    }

    async fn do_fetch(&self, chaser: &ChaserPage, req: FetchRequest) -> Result<PageResult> {
        chaser
            .apply_profile(&self.profile)
            .await
            .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

        // If the proxy requires authentication, enable the CDP Fetch domain so
        // Chrome pauses auth challenges and we can respond with credentials.
        // Credentials are stripped from --proxy-server (Chrome ignores them there
        // and they'd be visible in the process list).
        if let Some((ref username, ref password)) = self.proxy_credentials {
            chaser
                .raw_page()
                .execute(FetchEnableParams {
                    patterns: None,
                    handle_auth_requests: Some(true),
                })
                .await
                .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

            let mut auth_events = chaser
                .raw_page()
                .event_listener::<EventAuthRequired>()
                .await
                .map_err(|e| FlareSolverError::Browser(e.to_string()))?;

            let auth_page = chaser.raw_page().clone();
            let user = username.clone();
            let pass = password.clone();

            tokio::spawn(async move {
                while let Some(ev) = auth_events.next().await {
                    let _ = auth_page
                        .execute(ContinueWithAuthParams {
                            request_id: ev.request_id.clone(),
                            auth_challenge_response: AuthChallengeResponse {
                                response: AuthChallengeResponseResponse::ProvideCredentials,
                                username: Some(user.clone()),
                                password: Some(pass.clone()),
                            },
                        })
                        .await;
                }
            });
        }

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

        // Single event-driven loop — no evaluate() polling.
        //
        // goto() returns at DOMContentLoaded, which may land on a CF challenge page (403).
        // We stay in the loop watching Document response events:
        //   • Non-CF response (200, 404, …) or non-cloudflare server  → stop immediately
        //   • CF 403 (server: cloudflare)                             → CF is running JS;
        //     keep reading events until the challenge redirects to the real page
        //   • 200ms silence after settling + found_document           → page is done
        //   • Overall deadline (maxTimeout - 2 s)                     → hard stop
        //
        // This replaces both the old 500 ms evaluate() poll and the fixed 2 s drain,
        // eliminating ~N×2 CDP round-trips and reacting instantly to each navigation.
        let event_deadline = Instant::now()
            + Duration::from_millis(req.timeout_ms.saturating_sub(2_000).max(5_000));
        let mut headers: HashMap<String, String> = HashMap::new();
        let mut http_status = 200u16;
        let mut found_document = false;

        'events: loop {
            if Instant::now() >= event_deadline {
                break;
            }
            match timeout(Duration::from_millis(200), response_events.next()).await {
                Ok(Some(ev)) => {
                    if matches!(ev.r#type, ResourceType::Document) {
                        let status = ev.response.status as u16;
                        let is_cf = ev.response.headers.inner()
                            .as_object()
                            .and_then(|m| m.get("server").or_else(|| m.get("Server")))
                            .and_then(|v| v.as_str())
                            .map(|s| s.eq_ignore_ascii_case("cloudflare"))
                            .unwrap_or(false);

                        // Overwrite — always keep the most recent Document response.
                        http_status = status;
                        headers.clear();
                        if let Some(obj) = ev.response.headers.inner().as_object() {
                            for (k, v) in obj {
                                headers.insert(
                                    k.to_lowercase(),
                                    v.as_str().unwrap_or("").to_string(),
                                );
                            }
                        }
                        found_document = true;

                        if status == 403 && is_cf {
                            // CF challenge page — keep the loop alive waiting for redirect.
                            continue 'events;
                        }
                        // Real page (200/4xx/5xx non-CF, or any non-403) — done.
                        break 'events;
                    }
                    // Sub-resource events: consume silently.
                }
                Ok(None) => break, // CDP stream closed
                Err(_) => {
                    // 200 ms with no new event.
                    if found_document {
                        break; // quiet period after page settled
                    }
                    // No Document yet — keep waiting (e.g. slow first navigation).
                }
            }
        }
        drop(response_events);

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

    #[test]
    fn test_parse_proxy_credentials_with_auth() {
        let (creds, url) = parse_proxy_credentials("http://user:pass@proxy.example.com:8080");
        assert_eq!(creds, Some(("user".into(), "pass".into())));
        assert_eq!(url, "http://proxy.example.com:8080");
    }

    #[test]
    fn test_parse_proxy_credentials_no_auth() {
        let (creds, url) = parse_proxy_credentials("http://proxy.example.com:8080");
        assert!(creds.is_none());
        assert_eq!(url, "http://proxy.example.com:8080");
    }

    #[test]
    fn test_parse_proxy_credentials_socks5() {
        let (creds, url) = parse_proxy_credentials("socks5://admin:s3cr3t@10.0.0.1:1080");
        assert_eq!(creds, Some(("admin".into(), "s3cr3t".into())));
        assert_eq!(url, "socks5://10.0.0.1:1080");
    }
}
