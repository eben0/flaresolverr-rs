# flaresolverr-rs

[![crates.io](https://img.shields.io/crates/v/flaresolverr-rs.svg)](https://crates.io/crates/flaresolverr-rs)
[![docs.rs](https://docs.rs/flaresolverr-rs/badge.svg)](https://docs.rs/flaresolverr-rs)
[![CI](https://github.com/eben0/flaresolverr-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/eben0/flaresolverr-rs/actions/workflows/ci.yml)

A Rust port of [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) — bypasses Cloudflare **and** fingerprint/behavioural bot-walls (PerimeterX/HUMAN, Datadome) by driving a real stealth browser and returning the page from the same session that passed the challenge. Available as a **library** and **HTTP proxy**, built on [`chaser-cf`](https://crates.io/crates/chaser-cf).

## Library usage

Add to `Cargo.toml` (no axum/server deps):

```toml
[dependencies]
flaresolverr-rs = { version = "0.2", default-features = false }
```

```rust
use flaresolverr_rs::{FlareSolver, FlareSolverConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let solver = FlareSolver::new(FlareSolverConfig::default()).await?;

    let resp = solver.get("https://example.com").await?;
    println!("{} — {} bytes", resp.status, resp.body.len());
    Ok(())
}
```

`FlareSolverConfig` fields (all optional, shown with defaults):

```rust
FlareSolverConfig {
    headless: true,
    virtual_display: false,
    lazy_init: false,
    max_timeout_ms: 60_000,
    context_limit: 20,
    no_sandbox: true,
}
```

## Docker (HTTP proxy)

Drop-in replacement for FlareSolverr on port `8191`:

```bash
docker compose up
```

Or build the image directly:

```bash
docker build -f Dockerfile.build -t flaresolverr-rs .
docker run -p 8191:8191 flaresolverr-rs
```

```bash
curl -s http://localhost:8191/v1 \
  -H 'Content-Type: application/json' \
  -d '{"cmd":"request.get","url":"https://example.com","maxTimeout":60000}'
```

## Performance

**20 Prowlarr v11 indexers · single run · 2026-06-26**

| Implementation | Pass rate | Avg latency | p50 | p95 |
|----------------|-----------|-------------|-----|-----|
| **flaresolverr-rs** | 20/20 (100%) | **3.1s** | 3.0s | **4.0s** |
| flaresolverr-py | 20/20 (100%) | 3.9s | 2.1s | 11.8s |

- **1.3× faster** average, **~3× faster** at p95, with a tight, consistent distribution (p50 3.0s → p95 4.0s, no slow tail).
- CF-protected sites: rs ~3s — solves once and reuses the cached `cf_clearance` — vs py ~11s (re-solves the Turnstile challenge every request).
- Plain non-CF sites: py edges rs at p50 (returns at `DOMContentLoaded`); rs runs a full stealth-browser navigation for every site, which is exactly what lets it clear fingerprint WAFs (PerimeterX/HUMAN, e.g. Bloomberg) that a plain HTTP client cannot.

→ See [bench/README.md](bench/README.md) for methodology and per-indexer results.

## HTTP API

Drop-in replacement for FlareSolverr. Supports:

| Endpoint | Description |
|----------|-------------|
| `POST /v1` | Solve requests (`request.get`, `request.post`) |
| `GET /health` | Health check |
| `GET /` | Version info |

Session management (`sessions.create`, `sessions.list`, `sessions.destroy`) is supported. Sessions store a proxy URL; the clearance cache is shared across all sessions via the single shared browser.

### Configuration

Configure via `config.toml` or environment variables. Every field maps to `FLARESOLVERR_<FIELD>` (uppercase), and **env vars override `config.toml`**:

```toml
host            = "0.0.0.0"   # FLARESOLVERR_HOST
port            = 8191        # FLARESOLVERR_PORT
log_level       = "info"      # FLARESOLVERR_LOG_LEVEL
headless        = false       # FLARESOLVERR_HEADLESS         — true for headless Chrome (no display)
virtual_display = false       # FLARESOLVERR_VIRTUAL_DISPLAY  — true on Linux without a real display (Xvfb)
max_timeout_ms  = 120000      # FLARESOLVERR_MAX_TIMEOUT_MS
context_limit   = 20          # FLARESOLVERR_CONTEXT_LIMIT    — max concurrent Chrome contexts
```

Override at runtime, e.g. with Docker:

```bash
docker run -p 8191:8191 \
  -e FLARESOLVERR_LOG_LEVEL=debug \
  -e FLARESOLVERR_VIRTUAL_DISPLAY=true \
  ghcr.io/eben0/flaresolverr-rs:latest
```

Or load a file of `FLARESOLVERR_*` vars: `docker run --env-file .env …`, or `env_file: [.env]` in `docker-compose.yml`.

## Architecture

- **Browser-driven fetch**: every request is navigated in a real stealth Chrome (via `chaser-cf` / `chaser-oxide`) and the rendered DOM is returned from the *same* session that passed the WAF. Because the content comes from the browser that cleared the challenge — not a separate HTTP client — it defeats fingerprint/behavioural bot-management (Cloudflare, PerimeterX/HUMAN, Datadome), not just Cloudflare challenges.
- **Smart wait**: returns as soon as the page settles (`readyState=complete` and not a challenge) or a `cf_clearance` cookie appears, so clean sites and passively-admitted pages finish in ~2–3s instead of blocking on a fixed timeout. Interactive Cloudflare Turnstile widgets are clicked automatically.
- **Clearance caching**: `cf_clearance` is cached in the shared browser context and reused across requests, so CF sites resolve in ~3s after the first solve.
- **Single Chrome instance**: one browser (launched lazily) manages all contexts; concurrent requests share the pool up to `context_limit`. Proxied requests get an isolated incognito context.
- **Feature flags**: `default = ["server"]`. Use `default-features = false` to depend on the library only, without pulling in axum/figment.

## Tests

```bash
# Unit tests (no browser required)
cargo test --test models_test --test config_test --test error_test --test session_test --test solver_test

# HTTP API tests (starts a real axum server, most don't need Chrome)
cargo test --test api_test

# Prowlarr integration (browser required)
cargo test --test prowlarr -- --ignored --nocapture
```

## License

MIT
