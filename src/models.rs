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
