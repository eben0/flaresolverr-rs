# flaresolverr-rs

A Rust port of [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) — a reverse proxy that bypasses Cloudflare and similar bot-protection challenges using a headless Chromium browser with protocol-level stealth.

Drop-in replacement: existing code that talks to `http://localhost:8191/v1` works without modification.

## How it works

Every request is routed through a headless Chromium instance ([chaser-oxide](https://crates.io/crates/chaser-oxide)) that applies protocol-level stealth patches to evade fingerprinting. The browser renders the page, solves any Cloudflare/DataDome/Imperva challenge, and returns the final HTML along with the bypass cookies (`cf_clearance`, etc.) and real HTTP response headers captured via Chrome DevTools Protocol.

Named sessions keep a Chrome instance alive across requests so that bypass cookies persist and subsequent requests on the same domain skip the challenge entirely.

## Installation

Add to your `Cargo.toml` to use flaresolverr-rs as a library — no HTTP server needed, no serialization overhead. The browser runs in-process and you get `PageResult` directly:

```toml
[dependencies]
flaresolverr-rs = "0.1"
```

Chromium or Google Chrome must be installed on the host (`CHROME_PATH` env var if it's in a non-standard location).

See [Use as a library](#use-as-a-library) for usage examples.

---

## Docker

```yaml
services:
  flaresolverr:
    image: ghcr.io/eben0/flaresolverr-rs:latest
    ports:
      - "8191:8191"
    shm_size: 1gb
    environment:
      - FLARESOLVERR_LOG_LEVEL=info
      - FLARESOLVERR_NO_SANDBOX=true   # required in Docker (seccomp disables Chrome sandbox)
    restart: unless-stopped
```

`shm_size: 1gb` — Chromium needs more than the default 64 MB of shared memory or it crashes on large pages.

`FLARESOLVERR_NO_SANDBOX=true` — Docker's default seccomp profile disables Linux user namespaces, which Chrome's sandbox relies on. This is the standard workaround for containerised Chrome.

---

## API

Single endpoint: `POST /v1`

All requests and responses use JSON. The `cmd` field determines the operation.

### request.get

Fetch a URL via GET. Returns the final HTML after any redirects and bot-challenges.

```json
{
  "cmd": "request.get",
  "url": "https://www.bloomberg.com",
  "maxTimeout": 60000,
  "cookies": [{"name": "my_cookie", "value": "abc"}],
  "proxy": {"url": "http://user:pass@host:port"},
  "session": "my-session-id",
  "returnScreenshot": false
}
```

### request.post

Fetch a URL via POST with form-encoded body.

```json
{
  "cmd": "request.post",
  "url": "https://example.com/login",
  "postData": "username=foo&password=bar",
  "maxTimeout": 60000
}
```

### sessions.create

Create a named browser session. The session's Chrome instance stays alive until destroyed, carrying cookies and bypass tokens across requests.

```json
{
  "cmd": "sessions.create",
  "session": "my-session-id",
  "proxy": {"url": "http://user:pass@host:port"}
}
```

If `session` is omitted a UUID is generated and returned in `message`.

### sessions.list

List all active session IDs.

```json
{"cmd": "sessions.list"}
```

### sessions.destroy

Close a named session and free its browser.

```json
{"cmd": "sessions.destroy", "session": "my-session-id"}
```

---

### Response format

Success:

```json
{
  "status": "ok",
  "message": "",
  "solution": {
    "url": "https://www.bloomberg.com/",
    "status": 200,
    "headers": {"content-type": "text/html; charset=utf-8", "...": "..."},
    "response": "<html>...</html>",
    "cookies": [
      {
        "name": "cf_clearance",
        "value": "...",
        "domain": ".bloomberg.com",
        "path": "/",
        "expires": 1700000000.0,
        "httpOnly": false,
        "secure": true,
        "session": false,
        "sameParty": false,
        "storeId": "0"
      }
    ],
    "userAgent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 ...",
    "screenshot": null
  },
  "startTimestamp": 1716000000000,
  "endTimestamp": 1716000015000,
  "version": "0.1.0"
}
```

Error (HTTP 500):

```json
{
  "status": "error",
  "message": "unsupported command: bogus.cmd",
  "solution": null,
  "startTimestamp": 0,
  "endTimestamp": 0,
  "version": "0.1.0"
}
```

---

### Field reference

| Field | Type | Default | Notes |
|---|---|---|---|
| `cmd` | string | required | One of the commands above |
| `url` | string | required for `request.*` | Target URL |
| `maxTimeout` | number | 60000 | Milliseconds before timeout error |
| `cookies` | array | `[]` | Injected into browser before navigation |
| `proxy` | object | none | `{"url": "http://host:port"}`. Ephemeral browsers use per-request proxy; named sessions use the proxy they were created with |
| `session` | string | none | Named session ID. Omit for an ephemeral browser |
| `sessionTtlMinutes` | number | — | Parsed but not enforced (sessions persist until `sessions.destroy`) |
| `returnScreenshot` | bool | false | If true, `solution.screenshot` contains a base64-encoded PNG |
| `postData` | string | none | URL-encoded POST body for `request.post` |
| `headers` | object | — | Parsed but not forwarded (browser controls its own headers) |

---

## Configuration

Config is loaded from `config.toml` in the working directory, then overridden by environment variables with the `FLARESOLVERR_` prefix.

`config.toml` defaults:

```toml
host           = "0.0.0.0"
port           = 8191
log_level      = "info"      # trace | debug | info | warn | error
headless       = true        # set false to see the browser window (dev only)
max_timeout_ms = 60000
```

Environment variable override examples:

```bash
FLARESOLVERR_PORT=9000 FLARESOLVERR_HEADLESS=false cargo run
```

---

## Running

```bash
# Development
cargo run

# Release
cargo build --release
./target/release/flaresolverr-rs
```

Verify it's up:

```bash
curl -s -X POST http://localhost:8191/v1 \
  -H "Content-Type: application/json" \
  -d '{"cmd":"request.get","url":"https://example.com","maxTimeout":30000}' \
  | jq '.status, .solution.status'
```

### Test against a Cloudflare-protected site

```bash
curl -s -X POST http://localhost:8191/v1 \
  -H "Content-Type: application/json" \
  -d '{"cmd":"request.get","url":"https://www.bloomberg.com","maxTimeout":60000}' \
  | jq '{status, http_status: .solution.status, ua: .solution.userAgent, cookies: (.solution.cookies | map(.name))}'
```

Expected: `"status": "ok"`, `"http_status": 200`, `cf_clearance` in cookies.

### Named session workflow

```bash
# Create a session once (expensive: launches Chrome)
SESSION=$(curl -s -X POST http://localhost:8191/v1 \
  -H "Content-Type: application/json" \
  -d '{"cmd":"sessions.create"}' \
  | jq -r '.message | split(": ")[1]')

# Reuse the session for subsequent requests (fast: reuses Chrome + cookies)
curl -s -X POST http://localhost:8191/v1 \
  -H "Content-Type: application/json" \
  -d "{\"cmd\":\"request.get\",\"url\":\"https://www.bloomberg.com\",\"session\":\"$SESSION\"}" \
  | jq '.solution.status'

# Destroy when done
curl -s -X POST http://localhost:8191/v1 \
  -H "Content-Type: application/json" \
  -d "{\"cmd\":\"sessions.destroy\",\"session\":\"$SESSION\"}"
```

---

## Use as a library

Add to your `Cargo.toml`:

```toml
flaresolverr-rs = "0.1"
```

Use `BrowserSession` and `FetchRequest` directly — no HTTP round-trip, no serialization overhead:

```rust
use flaresolverr_rs::{BrowserSession, FetchRequest};

// Launch a browser (once; reuse for multiple fetches)
let session = BrowserSession::new(true, None).await?;

// GET
let result = session.fetch(
    FetchRequest::get("https://www.bloomberg.com")
        .timeout(30_000)
).await?;
println!("{}", result.html);

// GET with cookies and screenshot
let result = session.fetch(
    FetchRequest::get("https://example.com")
        .cookie("cf_clearance", "abc123")
        .timeout(60_000)
        .screenshot()
).await?;
println!("status={} screenshot_len={}", result.status, result.screenshot.as_ref().map_or(0, |s| s.len()));

// POST (form-encoded body)
let result = session.fetch(
    FetchRequest::post("https://example.com/login", "user=foo&pass=bar")
        .timeout(30_000)
).await?;
```

`PageResult` fields: `url`, `status`, `headers`, `html`, `cookies`, `user_agent`, `screenshot`.

### Cloudflare bypass strategies

**Single request / rarely visited domain** — create a session, fetch, drop:

```rust
let session = BrowserSession::new(true, None).await?;
let result = session.fetch(FetchRequest::get("https://www.bloomberg.com").timeout(60_000)).await?;
// session drops here, Chrome exits
```

Cost: full Chrome launch + challenge solve every time (~10–30s). Fine for one-off scrapes.

**Multiple requests to the same domain (recommended)** — reuse the session. After the first fetch solves the challenge, `cf_clearance` lives in Chrome's cookie jar. Subsequent fetches on the same domain skip the challenge and return in ~1–2s:

```rust
let session = BrowserSession::new(true, None).await?;

// First fetch: pays the Cloudflare challenge cost (~10–30s)
let r1 = session.fetch(FetchRequest::get("https://www.bloomberg.com/markets").timeout(60_000)).await?;

// Subsequent fetches: ~1–2s, challenge already solved
let r2 = session.fetch(FetchRequest::get("https://www.bloomberg.com/technology").timeout(30_000)).await?;
let r3 = session.fetch(FetchRequest::get("https://www.bloomberg.com/opinion").timeout(30_000)).await?;
```

**Concurrent scraping across multiple domains** — one session per domain, all in parallel:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

let bloomberg = Arc::new(Mutex::new(BrowserSession::new(true, None).await?));
let ft        = Arc::new(Mutex::new(BrowserSession::new(true, None).await?));

tokio::join!(
    async { bloomberg.lock().await.fetch(FetchRequest::get("https://www.bloomberg.com/markets").timeout(60_000)).await },
    async { ft.lock().await.fetch(FetchRequest::get("https://www.ft.com/content/...").timeout(60_000)).await },
);
```

The `Mutex` is needed because opening two tabs in the same browser simultaneously can cause CDP message interleaving.

**Key rules:**

- **One session per domain** — `cf_clearance` is domain-scoped; a Bloomberg session's cookies don't help on FT.
- **Don't share sessions across domains** — you'd be browsing FT with Bloomberg-authenticated Chrome state.
- **Sessions can go stale** — `cf_clearance` has a TTL (~30 min to a few hours). If you get a 403 or challenge on a reused session, destroy and recreate it.
- **Proxy + session = fixed pairing** — `cf_clearance` is bound to the IP that solved the challenge. If your proxy rotates IPs, create a new session per IP.

---

## Building

Requires:

- Rust 1.75+ (2021 edition)
- Chromium or Google Chrome installed (`CHROME_PATH` env var if non-standard location)
- OpenSSL development headers (`libssl-dev` on Debian/Ubuntu)

```bash
sudo apt-get install -y libssl-dev pkg-config chromium
cargo build
```

---

## Testing

Unit tests (no browser required):

```bash
cargo test
```

Integration tests (require Chrome):

```bash
cargo test -- --ignored --nocapture
```

Integration tests start an in-process axum server on a random port and make real HTTP requests against it.

---

## Architecture

```
POST /v1
  └── handlers::solve()         # dispatch by cmd
        ├── handle_fetch()       # request.get / request.post
        │     ├── SessionStore::get()      # named session path
        │     └── BrowserSession::new()   # ephemeral path
        │           └── BrowserSession::fetch(FetchRequest)
        │                 ├── event_listener::<EventResponseReceived>  # subscribe before navigation
        │                 ├── ChaserPage::apply_profile()             # stealth patches
        │                 ├── set_cookies()                           # inject caller cookies
        │                 ├── ChaserPage::goto()                      # navigate + solve challenge
        │                 ├── drain response events → headers + status
        │                 ├── ChaserPage::content()                   # HTML
        │                 ├── Page::get_cookies()                     # post-nav cookies
        │                 └── Page::screenshot()                      # optional PNG
        └── handle_session_*()   # sessions.create / list / destroy
              └── SessionStore    # DashMap<String, Arc<Mutex<BrowserSession>>>
```

| File | Responsibility |
|---|---|
| `src/main.rs` | Entry point: load config, init tracing, start axum |
| `src/config.rs` | `FlareSolverConfig` with serde defaults |
| `src/models.rs` | `SolveRequest`, `SolveResponse`, `Solution`, cookie types |
| `src/error.rs` | `FlareSolverError` enum + axum `IntoResponse` |
| `src/browser.rs` | `BrowserSession`, `FetchRequest` builder, `PageResult` |
| `src/session.rs` | `SessionStore`: DashMap-backed named session CRUD |
| `src/handlers.rs` | axum handler: JSON dispatch to request/session commands |
| `src/router.rs` | `create_router()`: axum Router wiring |
| `tests/api_test.rs` | Integration tests against a live in-process server |

---

## Known limitations

- **POST body**: Submitted via a JavaScript form (`<form method="POST">`). Works for standard form endpoints; raw JSON body endpoints are not supported.
- **`headers` field**: Accepted in the request body but not forwarded — the browser controls its own headers.
- **`sessionTtlMinutes`**: Parsed but not enforced. Sessions persist until `sessions.destroy` is called.
- **Proxy on existing sessions**: Changing the proxy on an existing named session requires destroying and recreating it (Chrome does not support live proxy changes).
