//! Prowlarr v11 indexer definition bypass tests.
//!
//! Requires: Chrome + Xvfb (Linux/Docker)
//! No-proxy run: cargo test --test prowlarr test_prowlarr_no_proxy -- --ignored --nocapture
//! Proxy run:    cargo test --test prowlarr test_prowlarr_with_proxy -- --ignored --nocapture
//! Both:         cargo test --test prowlarr -- --ignored --nocapture

use std::sync::Arc;

use chaser_cf::ChaserConfig;
use reqwest::Client;
use tokio::net::TcpListener;

const PROWLARR_DEFS_API: &str =
    "https://api.github.com/repos/Prowlarr/Indexers/contents/definitions/v11";

async fn start_test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = ChaserConfig::default()
        .with_headless(true)
        .with_virtual_display(true)
        .with_timeout_ms(120_000)
        .with_context_limit(5)
        .add_extra_arg("--no-sandbox")
        .add_extra_arg("--disable-gpu");
    let store = Arc::new(flaresolverr_rs::session::SessionStore::new(config));
    let router = flaresolverr_rs::router::create_router(store);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

/// Fetch all Prowlarr v11 definitions, extract the first link from each,
/// and POST it to the running test server. Returns (passed, failed, skipped).
async fn run_definitions(base: &str, proxy_url: Option<&str>) -> (usize, usize, usize) {
    let gh_client = Client::builder().user_agent("flaresolverr-rs-test/1.0").build().unwrap();
    let entries: Vec<serde_json::Value> = gh_client
        .get(PROWLARR_DEFS_API)
        .send().await.expect("failed to fetch definitions list")
        .json().await.expect("failed to parse definitions list");

    eprintln!("Found {} definition files", entries.len());

    let solve_client = Client::new();
    let (mut passed, mut failed, mut skipped) = (0usize, 0usize, 0usize);

    for entry in &entries {
        let name = entry["name"].as_str().unwrap_or("unknown");
        let Some(download_url) = entry["download_url"].as_str() else { skipped += 1; continue; };

        let yaml_str = match gh_client.get(download_url).send().await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(e) => { eprintln!("SKIP {name}: {e}"); skipped += 1; continue; }
        };
        let doc: serde_yaml::Value = match serde_yaml::from_str(&yaml_str) {
            Ok(d) => d,
            Err(e) => { eprintln!("SKIP {name}: YAML error: {e}"); skipped += 1; continue; }
        };

        let Some(links) = doc["links"].as_sequence() else { skipped += 1; continue; };
        let Some(first_link) = links.first().and_then(|l| l.as_str()) else { skipped += 1; continue; };

        let mut req = serde_json::json!({"cmd": "request.get", "url": first_link, "maxTimeout": 90000});
        if let Some(pu) = proxy_url { req["proxy"] = serde_json::json!({"url": pu}); }

        let resp: serde_json::Value = match solve_client.post(format!("{base}/v1")).json(&req).send().await {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(e) => { eprintln!("FAIL {name} ({first_link}): {e}"); failed += 1; continue; }
        };

        if resp["status"] == "ok" {
            eprintln!("PASS {name} ({first_link})");
            passed += 1;
        } else {
            eprintln!("FAIL {name} ({first_link}): {}", resp["message"].as_str().unwrap_or("?"));
            failed += 1;
        }
    }

    (passed, failed, skipped)
}

/// Pass 1: no proxy — tests direct CF bypass.
#[tokio::test]
#[ignore]
async fn test_prowlarr_no_proxy() {
    let base = start_test_server().await;
    eprintln!("Test server at {base} (no proxy)");
    let (passed, failed, skipped) = run_definitions(&base, None).await;
    eprintln!("Results: {passed} passed, {failed} failed, {skipped} skipped");
    assert_eq!(failed, 0, "{failed} definitions failed (no proxy)");
}

/// Pass 2: with proxy — loads HTTPS_PROXY from .env.proxy.
#[tokio::test]
#[ignore]
async fn test_prowlarr_with_proxy() {
    dotenvy::from_filename(".env.proxy").ok();
    let proxy_url = std::env::var("HTTPS_PROXY").expect("HTTPS_PROXY not set in .env.proxy");
    eprintln!("Using proxy: {proxy_url}");
    let base = start_test_server().await;
    eprintln!("Test server at {base}");
    let (passed, failed, skipped) = run_definitions(&base, Some(&proxy_url)).await;
    eprintln!("Results: {passed} passed, {failed} failed, {skipped} skipped");
    assert_eq!(failed, 0, "{failed} definitions failed (with proxy)");
}
