# flaresolverr-rs: Library API Refactor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose a clean `FlareSolver` Rust library API (builder-style `get()`/`post()`/`fetch()`) so downstream crates like `spiderpig` can embed CF bypass without pulling in axum or the HTTP server.

**Architecture:** Add a `server` Cargo feature (enabled by default) that gates axum, figment, and the HTTP handler layer. Core library exposes `FlareSolver::new(FlareSolverConfig)` → `FlareSolver`, with `get/post/fetch` methods returning `FetchResponse`. Server fields (`host`, `port`, `log_level`) split into `ServerConfig` behind the feature. The binary keeps working unchanged because `default = ["server"]`.

**Tech Stack:** Rust, chaser-cf 0.2.1, reqwest 0.12, axum 0.7 (optional/server), figment 0.10 (optional/server), tokio, thiserror, dashmap

**Commit conventions:** Single monolithic commit at the end. No Co-authored-by. Never push unless asked.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `Cargo.toml` | Modify | Add `server` feature; make axum/figment/tracing-subscriber optional; bump to 0.2.0 |
| `src/config.rs` | Modify | `FlareSolverConfig` = library fields only + `Default`; `ServerConfig` + `load()` gated behind `#[cfg(feature="server")]` |
| `src/models.rs` | Modify | Add `FetchRequest` (builder), `FetchResponse`; gate `SolveRequest`/`SolveResponse`/`Solution`/`ProxyConfig` behind `server` feature |
| `src/solver.rs` | Modify | `fetch()` returns `FetchResponse`; `dispatch()` gated behind `server` |
| `src/solver_api.rs` | Create | `FlareSolver` struct: `new()`, `get()`, `post()`, `fetch()`, `session_store()` |
| `src/error.rs` | Modify | Gate `axum::IntoResponse` impl behind `server` feature |
| `src/lib.rs` | Modify | Clean re-exports: `FlareSolver`, `FetchRequest`, `FetchResponse`, `FlareSolverConfig`; gate `handlers`/`router` |
| `src/handlers.rs` | Modify | Convert `FetchResponse` to `Solution` for JSON; update to use `FetchResponse` from dispatch |
| `src/main.rs` | Modify | Use `load()` that returns `(FlareSolverConfig, ServerConfig)` |
| `tests/config_test.rs` | Modify | Test `FlareSolverConfig` without `host`/`port`; add `ServerConfig` tests under `server` feature |
| `tests/solver_api_test.rs` | Create | Tests for `FetchRequest` builder, `FetchResponse` fields, `FlareSolver` construction |
| `tests/models_test.rs` | Modify | Add `#![cfg(feature = "server")]` |
| `tests/api_test.rs` | Modify | Add `#![cfg(feature = "server")]` |

---

## Task 1: Cargo.toml — add `server` feature, make heavy deps optional, bump version

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Replace `Cargo.toml` with the following**

```toml
[package]
name = "flaresolverr-rs"
version = "0.2.0"
edition = "2021"
description = "Cloudflare bypass library and FlareSolverr-compatible HTTP proxy"
license = "MIT"
repository = "https://github.com/eben0/flaresolverr-rs"
keywords = ["cloudflare", "flaresolverr", "bypass", "scraping", "chromium"]
categories = ["web-programming", "web-programming::http-client"]
readme = "README.md"
exclude = ["PLAN.md", "bench/"]

[lib]
name = "flaresolverr_rs"
path = "src/lib.rs"

[[bin]]
name = "flaresolverr-rs"
path = "src/main.rs"
required-features = ["server"]

[[test]]
name = "prowlarr"
path = "integration/prowlarr.rs"

[features]
default = ["server"]
server = ["dep:axum", "dep:figment", "dep:tracing-subscriber"]

[dependencies]
tokio              = { version = "1", features = ["full"] }
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"
uuid               = { version = "1", features = ["v4"] }
dashmap            = "6"
thiserror          = "2"
tracing            = "0.1"
anyhow             = "1"
chaser-cf          = "0.2.1"
reqwest            = { version = "0.12", features = ["json"] }
axum               = { version = "0.7", optional = true }
figment            = { version = "0.10", features = ["toml", "env"], optional = true }
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"], optional = true }

[dev-dependencies]
serde_yaml = "0.9"
dotenvy    = "0.15"
```

- [ ] **Step 2: Verify it compiles (with default features)**

```
cargo check
```

Expected: no errors (server feature is on by default so axum/figment still resolve)

---

## Task 2: Refactor `src/config.rs` — split library vs server config

**Files:**
- Modify: `src/config.rs`
- Modify: `tests/config_test.rs`

- [ ] **Step 1: Write failing test first**

Replace `tests/config_test.rs` entirely:

```rust
use flaresolverr_rs::config::FlareSolverConfig;

#[test]
fn test_defaults() {
    let cfg = FlareSolverConfig::default();
    assert!(cfg.headless);
    assert!(!cfg.virtual_display);
    assert!(!cfg.lazy_init);
    assert_eq!(cfg.max_timeout_ms, 60_000);
    assert_eq!(cfg.context_limit, 20);
    assert!(cfg.no_sandbox);
}

#[test]
fn test_deserialize_overrides() {
    let cfg: FlareSolverConfig =
        serde_json::from_str(r#"{"headless":false,"context_limit":5}"#).unwrap();
    assert!(!cfg.headless);
    assert_eq!(cfg.context_limit, 5);
    // unset fields keep defaults
    assert_eq!(cfg.max_timeout_ms, 60_000);
    assert!(cfg.no_sandbox);
}

#[cfg(feature = "server")]
mod server_tests {
    use flaresolverr_rs::config::ServerConfig;

    #[test]
    fn test_server_defaults() {
        let s = ServerConfig::default();
        assert_eq!(s.host, "0.0.0.0");
        assert_eq!(s.port, 8191);
        assert_eq!(s.log_level, "info");
    }

    #[test]
    fn test_server_deserialize() {
        let s: ServerConfig =
            serde_json::from_str(r#"{"port":9000,"log_level":"debug"}"#).unwrap();
        assert_eq!(s.port, 9000);
        assert_eq!(s.log_level, "debug");
        assert_eq!(s.host, "0.0.0.0");
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```
cargo test --test config_test
```

Expected: compile error — `lazy_init` not a field, `ServerConfig` not found

- [ ] **Step 3: Replace `src/config.rs` entirely**

```rust
use chaser_cf::ChaserConfig;
use serde::Deserialize;

/// Library-facing configuration for the CF bypass engine.
/// Construct with `FlareSolverConfig::default()` and override fields as needed.
#[derive(Debug, Clone, Deserialize)]
pub struct FlareSolverConfig {
    #[serde(default = "default_headless")]
    pub headless: bool,
    #[serde(default)]
    pub virtual_display: bool,
    #[serde(default)]
    pub lazy_init: bool,
    #[serde(default = "default_max_timeout")]
    pub max_timeout_ms: u64,
    #[serde(default = "default_context_limit")]
    pub context_limit: usize,
    #[serde(default = "default_no_sandbox")]
    pub no_sandbox: bool,
}

impl Default for FlareSolverConfig {
    fn default() -> Self {
        Self {
            headless: true,
            virtual_display: false,
            lazy_init: false,
            max_timeout_ms: 60_000,
            context_limit: 20,
            no_sandbox: true,
        }
    }
}

impl FlareSolverConfig {
    pub fn to_chaser_config(&self) -> ChaserConfig {
        let mut cfg = ChaserConfig::default()
            .with_context_limit(self.context_limit)
            .with_timeout_ms(self.max_timeout_ms)
            .with_headless(self.headless)
            .with_virtual_display(self.virtual_display)
            .with_lazy_init(self.lazy_init);
        if self.no_sandbox {
            cfg = cfg.add_extra_arg("--no-sandbox");
        }
        cfg
    }
}

fn default_headless() -> bool { true }
fn default_max_timeout() -> u64 { 60_000 }
fn default_context_limit() -> usize { 20 }
fn default_no_sandbox() -> bool { true }

// ─── Server-only config ──────────────────────────────────────────────────────

#[cfg(feature = "server")]
pub use server::ServerConfig;
#[cfg(feature = "server")]
pub use server::load;

#[cfg(feature = "server")]
mod server {
    use figment::{
        providers::{Env, Format, Toml},
        Figment,
    };
    use serde::Deserialize;
    use super::FlareSolverConfig;

    #[derive(Debug, Clone, Deserialize)]
    pub struct ServerConfig {
        #[serde(default = "default_host")]
        pub host: String,
        #[serde(default = "default_port")]
        pub port: u16,
        #[serde(default = "default_log_level")]
        pub log_level: String,
    }

    impl Default for ServerConfig {
        fn default() -> Self {
            Self {
                host: "0.0.0.0".into(),
                port: 8191,
                log_level: "info".into(),
            }
        }
    }

    /// Load configuration by merging `config.toml` and `FLARESOLVERR_` env vars.
    pub fn load() -> Result<(FlareSolverConfig, ServerConfig), Box<figment::Error>> {
        let solver: FlareSolverConfig = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("FLARESOLVERR_"))
            .extract()
            .map_err(Box::new)?;
        let server: ServerConfig = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("FLARESOLVERR_"))
            .extract()
            .map_err(Box::new)?;
        Ok((solver, server))
    }

    fn default_host() -> String { "0.0.0.0".into() }
    fn default_port() -> u16 { 8191 }
    fn default_log_level() -> String { "info".into() }
}
```

- [ ] **Step 4: Run tests**

```
cargo test --test config_test
```

Expected: all 4 pass

---

## Task 3: Add `FetchRequest`/`FetchResponse` to `src/models.rs`; gate HTTP types

**Files:**
- Modify: `src/models.rs`

- [ ] **Step 1: Replace `src/models.rs` entirely**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Library-facing types ─────────────────────────────────────────────────────

/// A URL fetch request with optional proxy, cookies, and POST body.
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub url: String,
    pub is_post: bool,
    pub post_data: Option<String>,
    pub proxy: Option<String>,
    pub cookies: Vec<RequestCookie>,
}

impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { url: url.into(), is_post: false, post_data: None, proxy: None, cookies: vec![] }
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self { url: url.into(), is_post: true, post_data: None, proxy: None, cookies: vec![] }
    }

    pub fn proxy(mut self, url: impl Into<String>) -> Self {
        self.proxy = Some(url.into());
        self
    }

    pub fn body(mut self, data: impl Into<String>) -> Self {
        self.post_data = Some(data.into());
        self
    }

    pub fn cookie(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cookies.push(RequestCookie { name: name.into(), value: value.into() });
        self
    }
}

/// The result of a successful fetch — HTML body, status, cookies, and the user-agent used.
#[derive(Debug)]
pub struct FetchResponse {
    pub url: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub cookies: Vec<ResponseCookie>,
    pub user_agent: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestCookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResponseCookie {
    pub domain: String,
    pub expiry: Option<f64>,
    pub http_only: bool,
    pub name: String,
    pub path: String,
    pub same_party: bool,
    pub secure: bool,
    pub session: bool,
    pub store_id: String,
    pub value: String,
}

// ─── HTTP server types (FlareSolverr v1 protocol) ─────────────────────────────

#[cfg(feature = "server")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveRequest {
    pub cmd: String,
    pub url: Option<String>,
    #[serde(default = "default_timeout")]
    pub max_timeout: u64,
    pub cookies: Option<Vec<RequestCookie>>,
    pub proxy: Option<ProxyConfig>,
    pub session: Option<String>,
    pub session_ttl_minutes: Option<u64>,
    pub return_screenshot: Option<bool>,
    pub post_data: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

#[cfg(feature = "server")]
fn default_timeout() -> u64 { 60_000 }

#[cfg(feature = "server")]
#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub url: Option<String>,
}

#[cfg(feature = "server")]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolveResponse {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution: Option<Solution>,
    pub start_timestamp: u64,
    pub end_timestamp: u64,
    pub version: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Solution {
    pub url: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub response: String,
    pub cookies: Vec<ResponseCookie>,
    pub user_agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

#[cfg(feature = "server")]
impl From<FetchResponse> for Solution {
    fn from(r: FetchResponse) -> Self {
        Solution {
            url: r.url,
            status: r.status,
            headers: r.headers,
            response: r.body,
            cookies: r.cookies,
            user_agent: r.user_agent,
            screenshot: None,
        }
    }
}
```

- [ ] **Step 2: Gate existing models tests**

At the top of `tests/models_test.rs`, add:

```rust
#![cfg(feature = "server")]
```

- [ ] **Step 3: Verify**

```
cargo check
```

Expected: no errors

---

## Task 4: Update `src/solver.rs` — `fetch()` returns `FetchResponse`; gate `dispatch()`

**Files:**
- Modify: `src/solver.rs`

- [ ] **Step 1: Replace `src/solver.rs` entirely**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use chaser_cf::{ChaserCF, Cookie, ProxyConfig};

use crate::error::{FlareSolverError, Result};
use crate::models::{FetchResponse, RequestCookie, ResponseCookie};
use crate::session::SessionStore;

/// Parse "scheme://[user:pass@]host:port" into a chaser_cf ProxyConfig.
pub fn parse_proxy_url(url: &str) -> Result<ProxyConfig> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| FlareSolverError::Browser("proxy URL must include a scheme (e.g. http://host:port)".into()))?;

    let (creds, hostport) = if let Some(at) = rest.rfind('@') {
        (Some(&rest[..at]), &rest[at + 1..])
    } else {
        (None, rest)
    };

    let (host, port_str) = hostport
        .rsplit_once(':')
        .ok_or_else(|| FlareSolverError::Browser("proxy URL missing port".into()))?;
    let port = port_str
        .parse::<u16>()
        .map_err(|_| FlareSolverError::Browser("proxy URL has invalid port".into()))?;

    let mut cfg = ProxyConfig::new(host, port).with_scheme(scheme);
    if let Some(creds) = creds {
        if let Some((user, pass)) = creds.split_once(':') {
            cfg = cfg.with_auth(user, pass);
        }
    }
    Ok(cfg)
}

/// Convert a chaser_cf Cookie into a ResponseCookie.
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

/// Build a reqwest client with optional proxy.
fn build_client(user_agent: &str, proxy_url: Option<&str>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().user_agent(user_agent);
    if let Some(pu) = proxy_url.filter(|p| !p.is_empty()) {
        let proxy = reqwest::Proxy::all(pu).map_err(|e| FlareSolverError::Http(e.to_string()))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| FlareSolverError::Http(e.to_string()))
}

pub const DEFAULT_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Return true if response headers/body indicate a Cloudflare challenge page.
fn is_cf_challenge(status: u16, headers: &HashMap<String, String>, body: &str) -> bool {
    if status == 403 && headers.get("cf-mitigated").map(|v| v.as_str()) == Some("challenge") {
        return true;
    }
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

/// GET or POST: try direct reqwest first; fall back to Chrome WAF bypass if CF challenge
/// detected. Connection errors (e.g. AIA fetching) fall back to get_source().
///
/// Note: chaser-cf's get_source/solve_waf_session both call wait_for_clearance which
/// polls for 30 s on non-CF sites. They must only be invoked when CF is detected or
/// when reqwest cannot connect at all.
pub async fn fetch(
    chaser: &ChaserCF,
    shared_client: &reqwest::Client,
    url: &str,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    extra_cookies: &[RequestCookie],
) -> Result<FetchResponse> {
    let extra_cookie_header: String = extra_cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");

    // ── Pass 1: direct request (fast path for non-CF URLs) ──────────────────
    let owned_client;
    let client: &reqwest::Client = if proxy_url.filter(|p| !p.is_empty()).is_some() {
        owned_client = build_client(DEFAULT_UA, proxy_url)?;
        &owned_client
    } else {
        shared_client
    };
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

    enum Pass1Result {
        Clean(u16, HashMap<String, String>, String, String),
        CfChallenge,
        ConnectionError,
    }

    let pass1 = match direct_req.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let headers: HashMap<String, String> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let final_url = resp.url().to_string();
            match resp.text().await {
                Ok(body) => {
                    if is_cf_challenge(status, &headers, &body) {
                        Pass1Result::CfChallenge
                    } else {
                        Pass1Result::Clean(status, headers, final_url, body)
                    }
                }
                Err(e) => {
                    tracing::warn!(url, error = %e, "Pass 1 body read failed — falling back to browser");
                    Pass1Result::ConnectionError
                }
            }
        }
        Err(e) => {
            tracing::warn!(url, error = %e, "Pass 1 connection failed — falling back to browser");
            Pass1Result::ConnectionError
        }
    };

    match pass1 {
        Pass1Result::Clean(status, headers, final_url, body) => {
            return Ok(FetchResponse {
                url: final_url,
                status,
                headers,
                body,
                cookies: vec![],
                user_agent: DEFAULT_UA.to_string(),
            });
        }
        Pass1Result::ConnectionError => {
            tracing::info!(url, "connection error on fast path — fetching via browser");
            let chaser_proxy = proxy_url
                .filter(|p| !p.is_empty())
                .map(parse_proxy_url)
                .transpose()?;
            let html = chaser
                .get_source(url, chaser_proxy)
                .await
                .map_err(FlareSolverError::from)?;
            return Ok(FetchResponse {
                url: url.to_string(),
                status: 200,
                headers: HashMap::new(),
                body: html,
                cookies: vec![],
                user_agent: DEFAULT_UA.to_string(),
            });
        }
        Pass1Result::CfChallenge => {}
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

    let owned_client2;
    let client2: &reqwest::Client = if proxy_url.filter(|p| !p.is_empty()).is_some() {
        owned_client2 = build_client(&user_agent, proxy_url)?;
        &owned_client2
    } else {
        shared_client
    };
    let request2 = if is_post {
        client2
            .post(url)
            .header("User-Agent", &user_agent)
            .header("Cookie", &cookie_header)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data.unwrap_or("").to_string())
    } else {
        client2.get(url).header("User-Agent", &user_agent).header("Cookie", &cookie_header)
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

    Ok(FetchResponse {
        url: final_url,
        status,
        headers,
        body: html,
        cookies,
        user_agent,
    })
}

/// Dispatch a FlareSolverr protocol command. Returns (status, message, Option<FetchResponse>).
#[cfg(feature = "server")]
#[allow(clippy::too_many_arguments)]
pub async fn dispatch(
    store: &Arc<SessionStore>,
    cmd: &str,
    url: Option<&str>,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    session_id: Option<&str>,
    extra_cookies: &[RequestCookie],
) -> Result<(String, String, Option<FetchResponse>)> {
    match cmd {
        "request.get" | "request.post" => {
            let url = url.ok_or_else(|| FlareSolverError::MissingField("url".into()))?;
            let session_proxy = session_id.and_then(|id| store.registry.get_proxy(id));
            let effective_proxy = session_proxy.as_deref().or(proxy_url);
            let resp = fetch(
                &store.chaser,
                &store.client,
                url,
                is_post,
                post_data,
                effective_proxy,
                extra_cookies,
            )
            .await?;
            Ok(("ok".into(), String::new(), Some(resp)))
        }
        "sessions.create" => {
            let id = store
                .registry
                .create(session_id.map(|s| s.to_string()), proxy_url)?;
            Ok(("ok".into(), format!("Session created: {id}"), None))
        }
        "sessions.list" => {
            let ids = store.registry.list();
            Ok(("ok".into(), serde_json::to_string(&ids).unwrap_or_default(), None))
        }
        "sessions.destroy" => {
            let id = session_id
                .ok_or_else(|| FlareSolverError::MissingField("session".into()))?;
            store.registry.destroy(id)?;
            Ok(("ok".into(), format!("Session destroyed: {id}"), None))
        }
        other => Err(FlareSolverError::UnsupportedCommand(other.to_string())),
    }
}
```

- [ ] **Step 2: Verify solver_test still compiles and passes**

```
cargo test --test solver_test
```

Expected: all 6 pass

---

## Task 5: Add `src/solver_api.rs` — the `FlareSolver` high-level API

**Files:**
- Create: `src/solver_api.rs`
- Create: `tests/solver_api_test.rs`

- [ ] **Step 1: Write failing tests first**

Create `tests/solver_api_test.rs`:

```rust
use flaresolverr_rs::models::{FetchRequest, RequestCookie};

#[test]
fn test_fetch_request_get_defaults() {
    let req = FetchRequest::get("https://example.com");
    assert_eq!(req.url, "https://example.com");
    assert!(!req.is_post);
    assert!(req.post_data.is_none());
    assert!(req.proxy.is_none());
    assert!(req.cookies.is_empty());
}

#[test]
fn test_fetch_request_post_with_body() {
    let req = FetchRequest::post("https://example.com").body("a=1&b=2");
    assert!(req.is_post);
    assert_eq!(req.post_data.as_deref(), Some("a=1&b=2"));
}

#[test]
fn test_fetch_request_builder_chain() {
    let req = FetchRequest::get("https://example.com")
        .proxy("http://proxy:8080")
        .cookie("name", "value")
        .cookie("foo", "bar");
    assert_eq!(req.proxy.as_deref(), Some("http://proxy:8080"));
    assert_eq!(req.cookies.len(), 2);
    assert_eq!(req.cookies[0].name, "name");
    assert_eq!(req.cookies[1].name, "foo");
}

#[test]
fn test_request_cookie_is_clone() {
    let c = RequestCookie { name: "x".into(), value: "y".into() };
    let c2 = c.clone();
    assert_eq!(c2.name, "x");
}
```

- [ ] **Step 2: Run to confirm pass (these only test models)**

```
cargo test --test solver_api_test
```

Expected: all 4 pass (models already defined in Task 3)

- [ ] **Step 3: Create `src/solver_api.rs`**

```rust
use std::sync::Arc;

use chaser_cf::ChaserCF;

use crate::config::FlareSolverConfig;
use crate::error::Result;
use crate::models::{FetchRequest, FetchResponse};
use crate::session::SessionStore;
use crate::solver;

/// High-level CF bypass client. Create with [`FlareSolver::new`], then call
/// [`get`](FlareSolver::get), [`post`](FlareSolver::post), or [`fetch`](FlareSolver::fetch).
///
/// # Example
/// ```no_run
/// # tokio_test::block_on(async {
/// use flaresolverr_rs::{FlareSolver, FlareSolverConfig};
/// let solver = FlareSolver::new(FlareSolverConfig::default()).await.unwrap();
/// let resp = solver.get("https://example.com").await.unwrap();
/// println!("{}", resp.body);
/// # })
/// ```
pub struct FlareSolver {
    store: Arc<SessionStore>,
}

impl FlareSolver {
    /// Initialize the browser engine and return a ready solver.
    pub async fn new(config: FlareSolverConfig) -> Result<Self> {
        let chaser = Arc::new(ChaserCF::new(config.to_chaser_config()).await?);
        let store = Arc::new(SessionStore::new(chaser));
        Ok(Self { store })
    }

    /// Fetch a URL via GET.
    pub async fn get(&self, url: &str) -> Result<FetchResponse> {
        self.fetch(FetchRequest::get(url)).await
    }

    /// Fetch a URL via POST with a URL-encoded body.
    pub async fn post(&self, url: &str, body: &str) -> Result<FetchResponse> {
        self.fetch(FetchRequest::post(url).body(body)).await
    }

    /// Fetch a URL with full control over proxy, cookies, and method.
    pub async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse> {
        solver::fetch(
            &self.store.chaser,
            &self.store.client,
            &req.url,
            req.is_post,
            req.post_data.as_deref(),
            req.proxy.as_deref(),
            &req.cookies,
        )
        .await
    }

    /// Expose the underlying session store for HTTP server integration.
    #[cfg(feature = "server")]
    pub fn session_store(&self) -> Arc<SessionStore> {
        Arc::clone(&self.store)
    }
}
```

- [ ] **Step 4: Verify it compiles**

```
cargo check
```

Expected: no errors

---

## Task 6: Gate server code; update `src/error.rs`, `src/handlers.rs`, `src/lib.rs`, `src/main.rs`

**Files:**
- Modify: `src/error.rs`
- Modify: `src/handlers.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Modify: `tests/api_test.rs`

- [ ] **Step 1: Update `src/error.rs` — gate `IntoResponse` behind `server` feature**

Replace `src/error.rs` entirely:

```rust
use chaser_cf::ChaserError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlareSolverError {
    #[error("browser error: {0}")]
    Browser(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session already exists: {0}")]
    SessionAlreadyExists(String),
    #[error("missing required field: {0}")]
    MissingField(String),
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("request timed out after {0}ms")]
    Timeout(u64),
    #[error("http error: {0}")]
    Http(String),
}

impl From<ChaserError> for FlareSolverError {
    fn from(e: ChaserError) -> Self {
        match e {
            ChaserError::Timeout(ms) => FlareSolverError::Timeout(ms),
            other => FlareSolverError::Browser(other.to_string()),
        }
    }
}

#[cfg(feature = "server")]
mod axum_impl {
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
        Json,
    };
    use super::FlareSolverError;

    impl IntoResponse for FlareSolverError {
        fn into_response(self) -> Response {
            tracing::error!(error = %self, "request failed");
            let body = serde_json::json!({
                "status": "error",
                "message": self.to_string(),
                "solution": null,
                "startTimestamp": 0u64,
                "endTimestamp": 0u64,
                "version": env!("CARGO_PKG_VERSION"),
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

pub type Result<T> = std::result::Result<T, FlareSolverError>;
```

- [ ] **Step 2: Update `src/handlers.rs` — convert `FetchResponse` to `Solution` for JSON**

Replace `src/handlers.rs` entirely:

```rust
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{extract::State, Json};

use crate::error::FlareSolverError;
use crate::models::{SolveRequest, SolveResponse, Solution};
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
    let (status, message, fetch_resp) = dispatch(
        &store,
        &req.cmd,
        req.url.as_deref(),
        req.cmd == "request.post",
        req.post_data.as_deref(),
        proxy_url,
        req.session.as_deref(),
        req.cookies.as_deref().unwrap_or(&[]),
    )
    .await?;

    Ok(Json(SolveResponse {
        status,
        message,
        solution: fetch_resp.map(Solution::from),
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
```

- [ ] **Step 3: Replace `src/lib.rs` with clean exports**

```rust
pub mod config;
pub mod error;
pub mod models;
pub mod session;
pub mod solver;
pub mod solver_api;

pub use config::FlareSolverConfig;
pub use error::{FlareSolverError, Result};
pub use models::{FetchRequest, FetchResponse, RequestCookie, ResponseCookie};
pub use solver_api::FlareSolver;

#[cfg(feature = "server")]
pub mod handlers;
#[cfg(feature = "server")]
pub mod router;
```

- [ ] **Step 4: Update `src/main.rs` to use the new `load()` return type**

```rust
use std::sync::Arc;

use chaser_cf::ChaserCF;
use flaresolverr_rs::config::load;
use flaresolverr_rs::router::create_router;
use flaresolverr_rs::session::SessionStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (solver_cfg, server_cfg) = load().map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&server_cfg.log_level)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    tracing::info!("initializing chaser-cf browser engine");
    let chaser = Arc::new(ChaserCF::new(solver_cfg.to_chaser_config()).await?);
    let store = Arc::new(SessionStore::new(chaser));
    let router = create_router(store);
    let addr = format!("{}:{}", server_cfg.host, server_cfg.port);

    tracing::info!("flaresolverr-rs listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
```

- [ ] **Step 5: Add feature gate to `tests/api_test.rs`**

Add this as the very first line of `tests/api_test.rs`:

```rust
#![cfg(feature = "server")]
```

- [ ] **Step 6: Run all tests**

```
cargo test
```

Expected: all tests pass. Tests gated behind `server` feature still run because `default = ["server"]`.

- [ ] **Step 7: Verify library compiles without server feature**

```
cargo check --no-default-features
```

Expected: no errors. axum/figment/tracing-subscriber not required.

- [ ] **Step 8: Commit**

```
git add -A
git commit -m "feat: expose FlareSolver library API; gate HTTP server behind 'server' feature"
```

---

## Verification Summary

### All unit tests
```
cargo test
```
Expected: all pass (server feature on by default)

### Library-only compile check
```
cargo check --no-default-features
```
Expected: compiles cleanly without axum/figment

### Binary smoke test
```
cargo build --release --bin flaresolverr-rs
```
Expected: builds successfully

### Integration usage (what spiderpig will write)
```rust
use flaresolverr_rs::{FlareSolver, FlareSolverConfig};

let solver = FlareSolver::new(FlareSolverConfig::default()).await?;
let resp = solver.get("https://example.com").await?;
println!("Status: {} | Body length: {}", resp.status, resp.body.len());
```

### `spiderpig`'s Cargo.toml
```toml
[dependencies]
flaresolverr-rs = { version = "0.2", default-features = false }
```
This pulls in: chaser-cf, reqwest, tokio, serde, thiserror, dashmap, uuid, tracing. Does NOT pull in: axum, figment, tracing-subscriber.
