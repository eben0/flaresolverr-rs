# flare_solver — Claude Code context

## What this is

A Rust port of FlareSolverr: an HTTP API server that bypasses Cloudflare and similar bot-protection challenges. Exposes `POST /v1` on port 8191 (same endpoint, same JSON format as the Python original — drop-in compatible).

## Build environment

**Build inside WSL** (Ubuntu-24.04). The Windows MSVC linker is not in PATH.

Standard build command:

```bash
CARGO_TARGET_DIR=/home/ubuntu/cargo-targets/flare_solver \
~/.cargo/bin/cargo build --manifest-path /mnt/d/git/agent-cain/flare_solver/Cargo.toml
```

Standard test command:

```bash
CARGO_TARGET_DIR=/home/ubuntu/cargo-targets/flare_solver \
~/.cargo/bin/cargo test --manifest-path /mnt/d/git/agent-cain/flare_solver/Cargo.toml
```

- `CARGO_TARGET_DIR` is on the WSL native filesystem to avoid NTFS permission issues and for speed.
- `flare_solver` has **no private git dependencies** — all deps are on crates.io or public GitHub. No SSH key required.

## chaser-oxide

[chaser-oxide](https://github.com/0xchasercat/chaser-oxide) is a fork of chromiumoxide (Chromium DevTools Protocol client). Key API points:

- `ChaserPage::new(page: Page) -> ChaserPage` — wrap a raw Page in stealth layer
- `ChaserPage::apply_profile(&ChaserProfile) -> anyhow::Result<()>` — apply stealth patches; always call this after `new_page()`
- `ChaserProfile::native().build()` — the recommended stealth profile
- `ChaserPage::goto(url: &str) -> anyhow::Result<()>` — navigate and wait for DOMContentLoaded; returns `Ok(())`, NOT `Ok(&Self)`
- `ChaserPage::content() -> anyhow::Result<String>` — get full page HTML
- `ChaserPage::url() -> anyhow::Result<Option<String>>` — current URL after redirects
- `ChaserPage::evaluate(script: &str) -> anyhow::Result<Option<serde_json::Value>>` — run JS in isolated world (safe — no `Runtime.enable` leak)
- `ChaserPage::raw_page() -> &Page` — access underlying chromiumoxide `Page` for CDP commands not exposed by ChaserPage
- `Page::get_cookies() -> chromiumoxide::Result<Vec<Cookie>>` — Cookie fields: `name, value, domain, path, expires: f64 (-1.0 = session), http_only: bool, secure: bool`
- `Page::user_agent() -> chromiumoxide::Result<String>` — safe, no Runtime.enable
- `Page::screenshot(params: impl Into<ScreenshotParams>) -> Result<Vec<u8>>` — returns PNG bytes; use `base64::engine::general_purpose::STANDARD.encode(&png)` for base64
- `Page::event_listener::<T>() -> Result<EventStream<T>>` — subscribe to CDP events; must be called BEFORE navigation to catch events that fire during navigation
- `BrowserConfig::builder().new_headless_mode().build()` — headless Chrome
- `BrowserConfig::builder().with_head().build()` — visible Chrome (dev only)
- `BrowserConfig::builder().arg("--proxy-server=http://host:port")` — proxy at browser launch; cannot be changed on a running instance

CDP type quirks:
- `EventResponseReceived.r#type` (raw identifier — `type` is a Rust keyword)
- `EventResponseReceived.response.headers.inner()` returns `&serde_json::Value` (a JSON object)
- `ResourceType::Document` — the variant for main-frame HTML responses
- `Headers` wraps `serde_json::Value`; access via `.inner().as_object()`

## Architecture

```
POST /v1 → handlers::solve() → dispatch by cmd
  request.get / request.post → handle_fetch()
    named session → SessionStore::get() → BrowserSession::fetch()
    ephemeral     → BrowserSession::new() → BrowserSession::fetch()
  sessions.create / list / destroy → SessionStore
```

`SessionStore` is a `DashMap<String, Arc<Mutex<BrowserSession>>>`. Sessions are concurrent-safe; each fetch holds the mutex for its duration.

`BrowserSession::fetch()` flow:
1. Open a new page in the session's Browser
2. `apply_profile(ChaserProfile::native())` — stealth
3. Subscribe to `EventResponseReceived` events (before navigation!)
4. Inject caller cookies via `set_cookies()`
5. `goto(url)` — navigate and wait for DOMContentLoaded
6. Drain `EventResponseReceived` stream, filter for `ResourceType::Document` → real headers + HTTP status
7. `content()` → HTML
8. `get_cookies()` → post-navigation cookies (includes `cf_clearance`)
9. Optionally `screenshot()` → base64 PNG

## Module responsibilities

| File | Purpose |
|---|---|
| `src/browser.rs` | `BrowserSession` wrapping chaser-oxide. `map_cookie()` is public for unit testing without launching Chrome. |
| `src/session.rs` | `SessionStore`: DashMap-backed CRUD. `BrowserSession::new(headless, proxy)`. |
| `src/handlers.rs` | `solve()` axum handler. Dispatches 5 commands. Wires proxy and screenshot through to browser. |
| `src/models.rs` | All JSON types (camelCase serde). `SolveRequest`, `SolveResponse`, `Solution`, cookies. |
| `src/error.rs` | `FlareSolverError` with `IntoResponse` returning HTTP 500 + JSON body. |
| `src/config.rs` | `FlareSolverConfig` with serde defaults. |
| `src/router.rs` | `create_router(AppState) -> Router` — single `POST /v1` route. |
| `src/lib.rs` | Re-exports all modules as `pub` so integration tests can import them. |
| `tests/api_test.rs` | Integration tests marked `#[ignore]` — require Chrome. Run with `cargo test -- --ignored`. |

## Testing

Unit tests (14 tests, no browser):

```bash
cargo test
```

Integration tests (require Chrome, start in-process server):

```bash
cargo test -- --ignored --nocapture
```

Unit tests cover:
- `browser::tests` — `map_cookie()` mapping logic
- `config::tests` — serde defaults and overrides
- `error::tests` — error message strings
- `handlers::tests` — cmd dispatch logic
- `models::tests` — JSON serialization round-trips
- `session::tests` — CRUD on in-memory store

## What NOT to do

- Do not use `raw_page().evaluate()` for JS — it triggers `Runtime.enable` which changes the browser's stealth fingerprint. Use `ChaserPage::evaluate()` instead (isolated world).
- Do not call `event_listener::<EventResponseReceived>()` after `goto()` — the events fire during navigation, before `goto()` returns. The listener must be set up first.
- Do not try to change a session's proxy after creation — Chrome does not support live proxy changes. Destroy and recreate the session.
- Do not run `cargo` via PowerShell — the Windows MSVC linker is missing. Always build in WSL.

## Known limitations

- **POST body**: Submitted via a JavaScript `<form method="POST">` workaround. Works for standard HTML form endpoints; JSON body POST endpoints are not supported.
- **`headers` request field**: Parsed and stored in `SolveRequest.headers` but not forwarded to the browser (browser controls its own headers).
- **`sessionTtlMinutes`**: Parsed but not enforced. Sessions live until `sessions.destroy`.
- **Response headers**: Captured from `EventResponseReceived` CDP event. The final hop's headers are returned (after all redirects). Headers are lowercased.
