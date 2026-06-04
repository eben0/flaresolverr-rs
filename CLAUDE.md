# flaresolverr-rs — Claude Notes

## Build

Windows native, PowerShell shell. Just `cargo <cmd>` — no cross-compilation needed.

```bash
cargo build --release
cargo test
```

## Code conventions

- All tests in `tests/` (one file per module). No `#[cfg(test)]` in source files.
- Tested symbols must be `pub`.
- No `Co-authored-by` in commit messages.
- Monolithic commits: one commit per logical change, not one per task.
- Never push unless explicitly asked.

## Architecture

**Two-pass fetch** (`src/solver.rs::fetch`):

1. **Pass 1 — reqwest direct**: Fast path for non-CF sites. Uses the shared `reqwest::Client` from `SessionStore` (connection pooling). Returns immediately on HTTP 200.
2. **Pass 2 — chaser-cf**: Triggered on CF challenge (HTTP 403/503 with CF markers). Calls `solve_waf_session()` which caches `cf_clearance` in the browser context. Then a second reqwest call with the clearance cookies fetches the actual page.
3. **Fallback — `get_source()`**: Used only when reqwest cannot connect (TLS chain / AIA fetching). NOT used as a general fast path.

**Why `get_source()` cannot replace the reqwest fast path**: chaser-cf 0.2.1's `get_source()` calls `wait_for_clearance(30s)` which polls for a `cf_clearance` cookie before returning. On non-CF sites the cookie never appears, so every call blocks for the full 30s. Source: `chaser-cf-0.2.1/src/core/solver.rs:30`.

**Config** (`src/config.rs`): uses figment with `config.toml` + `FLARESOLVERR_` env vars. Fields use snake_case (no `rename_all` — TOML and JSON use the same names).

**SessionStore** (`src/session.rs`): owns `Arc<ChaserCF>` (shared browser) + `SessionRegistry` (DashMap of proxy configs) + shared `reqwest::Client`.

## Running locally

```powershell
# Kill old server if running (binary lock on rebuild)
Stop-Process -Name "flaresolverr-rs" -Force -ErrorAction SilentlyContinue
Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

cargo build --release
Start-Process -FilePath ".\target\release\flaresolverr-rs.exe" -WindowStyle Hidden
```

flaresolverr-py comparison instance runs on port 8192 via Docker: `docker compose up flaresolverr-py`.

## Benchmarking

```bash
python bench/bench.py --limit 20 --timeout 90 --runs 3
```

See `bench/README.md` for full details.
