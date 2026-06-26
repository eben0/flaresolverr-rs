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

See [report-20260626-225050.md](report-20260626-225050.md).

**20 indexers · single run · 2026-06-26**

| Implementation | Pass rate | Avg latency | p50 | p95 |
|----------------|-----------|-------------|-----|-----|
| flaresolverr-rs | 20/20 (100%) | 3.1s | 3.0s | 4.0s |
| flaresolverr-py | 20/20 (100%) | 3.9s | 2.1s | 11.8s |

rs is **1.3× faster** on average and **~3× faster** at p95, with a tight distribution (p50 3.0s → p95 4.0s). CF-protected sites dominate py's p95 because it re-solves the Turnstile challenge on every request (~11s) while rs reuses its cached clearance cookie (~3s).

## Architecture Notes

### Browser-driven fetch (rs)

Every request is navigated in a real stealth Chrome (via `chaser-cf` / `chaser-oxide`) and the rendered DOM is returned from the **same** session that passed the WAF. There is no separate HTTP client, so the content provenance is the browser that cleared the challenge — which is what lets rs defeat fingerprint/behavioural bot-management (Cloudflare, PerimeterX/HUMAN, Datadome), not just Cloudflare. A **smart wait** returns as soon as the page settles (`readyState=complete` and not a challenge) or a `cf_clearance` cookie appears, so clean sites finish in ~2–3s rather than blocking on a fixed timeout.

### Why py is slower on CF sites

flaresolverr-py re-solves the Turnstile challenge on every request (~11s each). rs keeps the `cf_clearance` cookie cached in its shared browser context and reuses it, reducing CF-site latency to ~3s.

### Why py is faster on plain non-CF sites

py fires at `DOMContentLoaded` (~1.7s); rs runs a full stealth-browser navigation for every site (~3s). That extra cost is deliberate — it is what allows rs to clear fingerprint WAFs (PerimeterX/HUMAN, Datadome) where a plain HTTP client only ever gets a bot-wall.
