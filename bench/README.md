# flaresolverr-rs Benchmark

Performance comparison of **flaresolverr-rs** (Rust) vs **FlareSolverr** (Python) using two Docker containers side-by-side.

## Setup

```bash
cd bench/
docker compose up -d          # builds the Rust image on first run (~3 min)
pip install -r requirements.txt
python benchmark.py           # 5 requests per test (default, includes CF sites)
python benchmark.py --requests 10          # more samples
python benchmark.py --no-cf               # plain baseline only (faster)
```

Services:
- Python (v3.x): `http://localhost:8191`
- Rust  (v0.1.x): `http://localhost:8192`

## Results

Benchmarks run on Windows 11 / Docker Desktop (WSL2 backend), 3 requests per test.

### httpbin.org (no Cloudflare)

| Mode | Python | Rust | Speedup |
|------|--------|------|---------|
| ephemeral | avg 1.59s | avg 0.58s | **2.8× faster** |
| session cold | 0.90s | 0.54s | **1.7× faster** |
| session warm | avg 0.41s | avg 0.29s | **1.4× faster** |

### nowsecure.nl (Cloudflare JS challenge)

Both services solve the challenge and return `cf_clearance` (3/3).

| Mode | Python | Rust | Speedup |
|------|--------|------|---------|
| ephemeral | avg 1.44s | avg 0.48s | **3.0× faster** |
| session cold | 0.93s | 0.49s | **1.9× faster** |
| session warm | avg 0.48s | avg 0.22s | **2.2× faster** |

### bt4g.org (Cloudflare Turnstile)

> **Note:** Python and Rust take different paths here, so the speedup numbers are not a like-for-like
> comparison. Python solves a full JS browser challenge (~11s) and returns `cf_clearance` — a
> reusable cookie for subsequent direct requests. Rust bypasses the challenge via Turnstile URL
> redirect detection (returning status 200 and the page body) but does **not** produce `cf_clearance`.
> Both return the page successfully; they differ in what Cloudflare credentials are available afterward.
>
> For a fair CF-bypass comparison where both services return `cf_clearance`, see nowsecure.nl above.

| Mode | Python (with cf_clearance) | Rust (no cf_clearance) | Time to 200 |
|------|---------------------------|------------------------|-------------|
| ephemeral | avg 11.51s | avg 0.30s | Rust ~38× faster |
| session cold | 11.26s | 0.34s | Rust ~33× faster |
| session warm | avg 0.41s | avg 0.39s | ~1.0× (parity) |

### Sites unavailable during testing

| Site | Status |
|------|--------|
| idope.se | Both services failed (geo-restricted or stricter challenge) |
| yts.unblockninja.com | Both services failed (domain appears down) |

## Key observations

- **Plain sites (httpbin.org)**: Rust is ~1.4–2.8× faster. The ephemeral win comes from the
  pre-warmed browser (no per-request Chrome launch); the warm path win is lower CDP overhead
  from Rust's async runtime vs. Python's asyncio.

- **CF JS challenge (nowsecure.nl)**: Rust is ~1.9–3.0× faster while achieving the same bypass
  rate (3/3 `cf_clearance`). Ephemeral requests are now nearly as fast as session-warm because
  browser startup is no longer on the critical path.

- **CF Turnstile (bt4g.org)**: Rust reaches the page ~33–39× faster, but via a different path —
  Turnstile URL-redirect detection rather than a full JS challenge solve. Python takes ~11s and
  returns `cf_clearance`; Rust takes ~0.30s and returns the page but no `cf_clearance`. This is
  not a fair bypass comparison; see nowsecure.nl for that.

- **Session warm parity on bt4g.org**: Both services are ~0.4s on warm repeat fetches —
  the bottleneck here is the network round-trip to a distant server, not the framework.

## Implementation details

### Pre-warmed ephemeral browser

By default, `BrowserSession::new()` launches a fresh Chrome process per request (~0.5–0.6s).
flaresolverr-rs starts one shared Chrome process at server startup and reuses it for all
ephemeral (non-session) requests.

Per-request isolation is maintained via **browser contexts** (`Target.createBrowserContext` CDP
command): each request gets its own context with separate cookies, localStorage, and cache, then
disposes it when done. Creating and destroying a context costs ~5ms vs ~600ms for launching Chrome.

If a proxy is specified, a dedicated browser is still created (proxy is a browser-level flag, not
context-level). If the shared browser crashes, it is restarted in the background automatically.

### Event-driven challenge wait

When Cloudflare serves a 403 challenge page, the browser runs JavaScript that eventually
redirects to the real page. A naive implementation polls `page.evaluate()` in a loop to detect
when this happens — each poll is a CDP round-trip with isolated-world overhead (~500µs per call,
potentially 100+ calls for a long wait).

flaresolverr-rs subscribes to `EventResponseReceived` CDP events (a stream already open before
navigation) and reads from it in a tight async loop:

- `status 403` + `server: cloudflare` → CF challenge in progress, keep reading
- Any other Document response → real page arrived, break immediately
- 200ms silence after a document was found → page settled, break

This eliminates all polling round-trips and reacts to the real page with sub-millisecond latency.

## Mode definitions

| Mode | Description |
|------|-------------|
| `ephemeral` | Isolated browser context per request (shared Chrome process). Stateless — mirrors most use cases. |
| `session cold` | First request on a named session. Browser starts once; Chrome profile is fresh. |
| `session warm` | Subsequent requests on the same session. Browser stays alive; challenge already solved. |
| CF solved | `cf_clearance` cookie present in response (traditional JS challenge bypass). |

## Infrastructure notes

- Both containers run on the same host — network latency is negligible.
- The Rust image requires `FLARESOLVERR_NO_SANDBOX=true` because Docker's default seccomp profile
  disables the Linux user namespace that Chrome's sandbox relies on. This is standard for
  containerised Chrome; the flag maps to `BrowserConfigBuilder::no_sandbox()` which sets both
  `--no-sandbox` and `--disable-setuid-sandbox`.
- Each named `BrowserSession` gets a unique `user_data_dir` under `/tmp/flaresolverr-rs/<uuid>/`,
  cleaned up on drop. This prevents `SingletonLock` collisions when multiple sessions run concurrently.
