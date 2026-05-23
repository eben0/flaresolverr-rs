use std::sync::Arc;
use reqwest::Client;
use tokio::net::TcpListener;

async fn start_test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let store = Arc::new(flaresolverr_rs::session::SessionStore::new(true));
    let router = flaresolverr_rs::router::create_router(store);
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
#[ignore]
async fn test_request_get_example() {
    let base = start_test_server().await;
    let client = Client::new();
    let resp: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({
            "cmd": "request.get",
            "url": "https://example.com",
            "maxTimeout": 30000
        }))
        .send().await.unwrap()
        .json().await.unwrap();

    assert_eq!(resp["status"], "ok");
    assert!(resp["solution"]["response"].as_str().unwrap().contains("Example Domain"));
}

#[tokio::test]
#[ignore]
async fn test_unsupported_command_returns_error() {
    let base = start_test_server().await;
    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "bogus.cmd"}))
        .send().await.unwrap();

    assert_eq!(resp.status().as_u16(), 500);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "error");
    assert!(body["message"].as_str().unwrap().contains("bogus.cmd"));
}

#[tokio::test]
#[ignore]
async fn test_session_lifecycle() {
    let base = start_test_server().await;
    let client = Client::new();

    let create: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.create"}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(create["status"], "ok");

    let _list: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.list"}))
        .send().await.unwrap().json().await.unwrap();

    let session_id = create["message"]
        .as_str().unwrap()
        .strip_prefix("Session created: ").unwrap();

    let destroy: serde_json::Value = client
        .post(format!("{base}/v1"))
        .json(&serde_json::json!({"cmd": "sessions.destroy", "session": session_id}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(destroy["status"], "ok");
}
