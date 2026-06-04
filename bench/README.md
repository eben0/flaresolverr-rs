# Benchmark

Compares **flaresolverr-rs** against **flaresolverr-py** using live Prowlarr v11 indexer definitions.

## Requirements

- Python 3.10+
- Both services running:
  - `flaresolverr-rs` on `http://localhost:8191` (default)
  - `flaresolverr-py` on `http://localhost:8192` (default)

Dependencies (`requests`, `pyyaml`) are auto-installed on first run.

## Usage

```bash
# Quick single pass, first 20 definitions
python bench/bench.py --limit 20 --timeout 90

# Averaged results over 3 runs
python bench/bench.py --limit 20 --timeout 90 --runs 3

# Full suite (all ~2000 definitions) — takes ~30 min
python bench/bench.py --timeout 90 --runs 1
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--rs-url URL` | `http://localhost:8191` | flaresolverr-rs endpoint |
| `--py-url URL` | `http://localhost:8192` | flaresolverr-py endpoint |
| `--limit N` | all | Test only the first N definitions |
| `--timeout S` | 90 | `maxTimeout` sent in each request (seconds) |
| `--runs N` | 1 | Repeat each definition N times and average results |

Output is written to `bench/report-YYYYMMDD-HHMMSS.md`.

## Latest Results

See [report-20260604-164341.md](report-20260604-164341.md).

**20 indexers · 3 runs · 2026-06-04**

| Implementation | Pass rate | Avg latency | p95 latency |
|----------------|-----------|-------------|-------------|
| flaresolverr-rs | 20/20 (100%) | 2.7s | 3.6s |
| flaresolverr-py | 20/20 (100%) | 3.9s | 12.7s |

rs is **1.4× faster** on average and **3.5× faster** at p95 (CF-protected sites dominate p95 because py re-solves the Turnstile challenge on every request while rs reuses its cached clearance cookie).

## Architecture Notes

### Two-pass fetch (rs)

1. **Pass 1 — reqwest direct**: Fast for ~75% of sites (no CF protection). Completes in ~2s.
2. **Pass 2 — Chrome via chaser-cf**: Only triggered on a CF challenge response (HTTP 403/503 with CF markers). Returns in ~3s due to cached `cf_clearance`.
3. **Fallback — `get_source()`**: Used only when reqwest cannot connect (TLS chain / AIA fetching required). Chrome handles these natively.

### Why not use Chrome for all GETs?

`chaser-cf::get_source()` calls `wait_for_clearance(30s)` internally — it polls for a `cf_clearance` cookie before returning. On non-CF sites that cookie never appears, so every call blocks for the full 30 seconds. This would make non-CF sites ~33s instead of ~2s.

### Why py is slower on CF sites

flaresolverr-py (Chrome inside Docker/Linux) re-solves the Turnstile challenge on every request (~11s each). rs keeps the `cf_clearance` cookie cached in its browser context and reuses it, reducing CF-site latency to ~3s.

### Why py is faster on non-CF sites

py fires at `DOMContentLoaded` (~1.7s); rs downloads the full response body via reqwest (~2.4s). The gap is structural and cannot be closed with chaser-cf 0.2.1 — see the architecture note above.
