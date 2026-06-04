# flaresolverr-rs

A Rust port of [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) — a Cloudflare bypass proxy that implements the same HTTP API, built on [`chaser-cf`](https://crates.io/crates/chaser-cf).

## Performance

**20 Prowlarr v11 indexers · 3-run average**

| Implementation | Pass rate | Avg latency | p95 latency |
|----------------|-----------|-------------|-------------|
| **flaresolverr-rs** | 20/20 (100%) | **2.7s** | **3.6s** |
| flaresolverr-py | 20/20 (100%) | 3.9s | 12.7s |

- **1.4× faster** average, **3.5× faster** at p95
- CF-protected sites: rs ~3s (cached clearance) vs py ~12s (re-solves challenge each time)
- Non-CF sites: rs ~2.4s (reqwest direct) vs py ~1.7s (DOMContentLoaded)

→ See [bench/README.md](bench/README.md) for methodology and detailed per-indexer results.

## API Compatibility

Drop-in replacement for FlareSolverr. Supports:

| Endpoint | Description |
|----------|-------------|
| `POST /v1` | Solve requests (`request.get`, `request.post`) |
| `GET /health` | Health check |
| `GET /` | Version info |

Session management (`sessions.create`, `sessions.list`, `sessions.destroy`) is supported. Sessions store a proxy URL; the CF clearance cache is shared across all sessions via the single `ChaserCF` instance.

### Example

```bash
curl -s http://localhost:8191/v1 \
  -H 'Content-Type: application/json' \
  -d '{"cmd":"request.get","url":"https://example.com","maxTimeout":60000}'
```

## Running

### Docker (recommended)

```bash
docker compose up
```

Runs flaresolverr-rs on port `8191` and flaresolverr-py on `8192` for comparison.

### Local

```bash
cargo build --release
./target/release/flaresolverr-rs
```

Reads `config.toml` from the current directory. Environment variables prefixed `FLARESOLVERR_` override TOML values.

### Configuration

`config.toml`:

```toml
host            = "0.0.0.0"
port            = 8191
log_level       = "info"
headless        = false      # set true for headless Chrome (no display)
virtual_display = false      # set true on Linux without a real display
max_timeout_ms  = 120000
context_limit   = 20         # max concurrent Chrome contexts
```

## Architecture

- **Two-pass fetch**: reqwest direct for non-CF sites (~2s), Chrome via chaser-cf only when a CF challenge is detected or reqwest cannot connect (TLS/AIA issues).
- **CF clearance caching**: `cf_clearance` cookies are cached in the shared browser context and reused across requests. CF sites resolve in ~3s after the first solve.
- **Single Chrome instance**: One `ChaserCF` manages all browser contexts. Concurrent requests share the pool up to `context_limit`.

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
