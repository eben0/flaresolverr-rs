use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

fn default_timeout() -> u64 {
    60_000
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestCookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub url: Option<String>,
}

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
