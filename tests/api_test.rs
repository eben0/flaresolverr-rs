use std::sync::Arc;

use chaser_cf::{ChaserCF, ChaserConfig};
use reqwest::Client;
use tokio::net::TcpListener;

async fn start_test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let chaser = Arc::new(
        ChaserCF::new(
            ChaserConfig::default()
                .with_headless(true)
                .with_context_limit(2)
                .with_lazy_init(true)
                .add_extra_arg("--no-sandbox"),
        )
        .await
        .unwrap(),
    );
    let store = Arc::new(flaresolverr_rs::session::SessionStore::new(chaser));
    let router = flaresolverr_rs::router::create_router(store);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn test_health_endpoint() {
    let base = start_test_server().await;
    let resp: serde_json::Value = Client::new()
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["status"], "ok");
}

#[tokio::test]
async fn test_index_endpoint() {
    let base = start_test_server().await;
    let resp: serde_json::Value = Client::new()
        .get(format!("{base}/"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["msg"], "FlareSolverr is ready!");
    assert!(resp["version"].is_string());
    assert!(resp["userAgent"].is_string());
}

#[tokio::test]
async fn test_unsupported_command_returns_500() {
    let base = start_test_server().await;
    let resp = Client::new()
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "bogus.cmd"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 500);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert!(body["message"].as_str().unwrap().contains("bogus.cmd"));
}

#[tokio::test]
async fn test_session_lifecycle() {
    let base = start_test_server().await;
    let client = Client::new();

    let create: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.create", "session": "test-sess"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(create["status"], "ok");
    assert!(create["message"].as_str().unwrap().contains("test-sess"));

    let list: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.list"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["status"], "ok");

    let destroy: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.destroy", "session": "test-sess"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(destroy["status"], "ok");
}

#[tokio::test]
async fn test_missing_url_for_request_get_returns_error() {
    let base = start_test_server().await;
    let resp = Client::new()
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "request.get"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 500);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert!(body["message"].as_str().unwrap().contains("url"));
}

#[tokio::test]
#[ignore] // requires Chrome
async fn test_request_get_example() {
    let base = start_test_server().await;
    let resp: serde_json::Value = Client::new()
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({
            "cmd": "request.get",
            "url": "https://example.com",
            "maxTimeout": 30000
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["status"], "ok");
    assert!(resp["solution"]["response"]
        .as_str()
        .unwrap()
        .contains("Example Domain"));
    assert!(resp["solution"]["cookies"].is_array());
    assert!(resp["solution"]["userAgent"].is_string());
}
