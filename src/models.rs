use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Inbound ───────────────────────────────────────────────────────────────────

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

fn default_timeout() -> u64 { 60_000 }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestCookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyConfig {
    pub url: String,
}

// ── Outbound ──────────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_get_request() {
        let json = r#"{"cmd":"request.get","url":"https://bloomberg.com","maxTimeout":30000}"#;
        let req: SolveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.cmd, "request.get");
        assert_eq!(req.url.as_deref(), Some("https://bloomberg.com"));
        assert_eq!(req.max_timeout, 30_000);
        assert!(req.session.is_none());
    }

    #[test]
    fn test_deserialize_post_request() {
        let json = r#"{"cmd":"request.post","url":"https://example.com","postData":"a=1&b=2"}"#;
        let req: SolveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.post_data.as_deref(), Some("a=1&b=2"));
        assert_eq!(req.max_timeout, 60_000);
    }

    #[test]
    fn test_serialize_response() {
        let resp = SolveResponse {
            status: "ok".into(),
            message: "".into(),
            solution: None,
            start_timestamp: 1000,
            end_timestamp: 2000,
            version: "0.1.0".into(),
        };
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains(r#""status":"ok""#));
        assert!(s.contains(r#""startTimestamp":1000"#));
    }

    #[test]
    fn test_deserialize_with_proxy() {
        let json = r#"{"cmd":"request.get","url":"https://example.com","proxy":{"url":"http://p:8080"}}"#;
        let req: SolveRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.proxy.unwrap().url, "http://p:8080");
    }
}
