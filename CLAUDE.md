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

**All-browser fetch** (`src/solver.rs::fetch`): every request is driven through a real
stealth Chrome — navigate, solve any challenge, and return the rendered DOM from the
*same* session that passed the WAF (like flaresolverr-py). This is what lets it beat
fingerprint/behavioural WAFs (PerimeterX/HUMAN on Bloomberg, Datadome, Akamai), not just
Cloudflare. There is **no reqwest fetch path** anymore.

Flow:
1. `acquire_permit()` (bounds concurrency to `context_limit`) → `create_context()`
   (incognito per proxied request; shared default context otherwise, so `cf_clearance`
   carries across requests) → `new_page("about:blank")` (applies the native stealth
   profile) → proxy auth → `goto(url)`.
2. **`solve_and_wait`** — a smart wait that returns as soon as *either* a `cf_clearance`
   cookie appears (Cloudflare cleared) *or* the page is `readyState=complete` and no
   longer looks like a challenge (`is_challenge_title` + Turnstile/PerimeterX selectors).
   Clean sites and passively-admitted PerimeterX pages return in ~2–8s. Interactive
   Cloudflare Turnstile widgets are clicked via a ported CDP shadow-root + Bezier-cursor
   routine after a 6s passive window.
3. GET returns `page.content()` (rendered DOM); POST runs an in-page `fetch()` from the
   page's JS context. Status/headers are synthesized (200 / `{}`) since CDP navigation
   doesn't surface them — same as flaresolverr-py.

**Why we drive `BrowserManager` directly (not chaser-cf's `get_source`)**: chaser-cf
0.2.1's `get_source()` hardcodes `wait_for_clearance(30s)` (`chaser-cf-0.2.1/src/core/solver.rs:30`)
which polls for `cf_clearance` — on any non-CF site the cookie never appears, so it blocks
the full 30s. We instead use the public `chaser_cf::core::BrowserManager` (+ a direct
`chaser-oxide 0.2.4` dep, matching chaser-cf's exact features so the `Page` types unify)
with our own `solve_and_wait`, which early-exits on settle. Turnstile-click logic is ported
into `solver.rs` because it lives in chaser-cf's private solver.

**Config** (`src/config.rs`): uses figment with `config.toml` + `FLARESOLVERR_` env vars.
Fields use snake_case. `headless=false` + a real display (or Xvfb via `virtual_display` on
Linux) is the strongest stealth and is what passes PerimeterX.

**SessionStore** (`src/session.rs`): owns a lazily-launched `BrowserManager` (in a
`tokio::sync::OnceCell`, so health/session endpoints need no Chrome) + `SessionRegistry`
(DashMap of proxy configs). `main.rs`/`FlareSolver::new` force-init eagerly unless `lazy_init`.

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
