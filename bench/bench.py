#!/usr/bin/env python3
"""
Benchmark flaresolverr-rs vs flaresolverr-py against Prowlarr v11 definitions.

Usage:
    python bench/bench.py [--rs-url URL] [--py-url URL] [--limit N] [--timeout S] [--runs N]

With --runs N the benchmark executes N full passes and reports averaged latency,
pass rate, and CF clearance counts alongside per-run breakdowns.

Outputs a Markdown report to bench/report-YYYYMMDD-HHMMSS.md
"""
import argparse
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

# Auto-install deps
try:
    import requests
    import yaml
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests", "pyyaml"])
    import requests
    import yaml

DEFS_API = "https://api.github.com/repos/Prowlarr/Indexers/contents/definitions/v11"
GH_HEADERS = {"User-Agent": "flaresolverr-bench/1.0", "Accept": "application/vnd.github.v3+json"}


def fetch_definitions(limit: int) -> list[dict]:
    print(f"Fetching Prowlarr v11 definitions (limit={limit})...")
    resp = requests.get(DEFS_API, headers=GH_HEADERS, timeout=30)
    resp.raise_for_status()
    entries = resp.json()
    entries = [e for e in entries if e.get("name", "").endswith(".yml")]
    if limit:
        entries = entries[:limit]
    print(f"  {len(entries)} definitions to test")
    return entries


def get_first_link(download_url: str) -> str | None:
    try:
        resp = requests.get(download_url, headers=GH_HEADERS, timeout=15)
        resp.raise_for_status()
        doc = yaml.safe_load(resp.text)
        links = doc.get("links") or []
        return links[0] if links else None
    except Exception:
        return None


def solve(base_url: str, target_url: str, timeout_ms: int) -> dict:
    payload = {"cmd": "request.get", "url": target_url, "maxTimeout": timeout_ms}
    t0 = time.monotonic()
    try:
        resp = requests.post(f"{base_url}/v1", json=payload, timeout=timeout_ms / 1000 + 10)
        elapsed = int((time.monotonic() - t0) * 1000)
        data = resp.json()
        solution = data.get("solution") or {}
        cookies = solution.get("cookies") or []
        has_clearance = any(c.get("name") == "cf_clearance" for c in cookies)
        return {
            "ok": data.get("status") == "ok",
            "elapsed_ms": elapsed,
            "http_status": solution.get("status", 0),
            "has_clearance": has_clearance,
            "message": data.get("message", ""),
        }
    except Exception as e:
        elapsed = int((time.monotonic() - t0) * 1000)
        return {"ok": False, "elapsed_ms": elapsed, "http_status": 0, "has_clearance": False, "message": str(e)}


def check_alive(url: str, label: str) -> bool:
    try:
        resp = requests.get(f"{url}/health", timeout=5)
        if resp.json().get("status") == "ok":
            print(f"  {label} ({url}): UP")
            return True
    except Exception:
        pass
    print(f"  {label} ({url}): DOWN (skipping)")
    return False


def fmt_ms(ms: float) -> str:
    ms = int(ms)
    return f"{ms:,}ms" if ms < 1000 else f"{ms/1000:.1f}s"


def run_once(definitions: list[dict], rs_url: str, py_url: str,
             rs_alive: bool, py_alive: bool, timeout_ms: int,
             run_num: int, total_runs: int) -> list[dict]:
    """Execute one full pass over all definitions. Returns per-indexer result rows."""
    results = []
    prefix = f"[run {run_num}/{total_runs}]" if total_runs > 1 else ""
    for i, entry in enumerate(definitions, 1):
        name = entry["name"].removesuffix(".yml")
        print(f"{prefix}[{i}/{len(definitions)}] {name}")
        url = get_first_link(entry["download_url"]) if entry.get("download_url") else None
        if not url:
            print(f"  SKIP: no URL found")
            results.append({"name": name, "url": None})
            continue
        print(f"  URL: {url}")
        row: dict = {"name": name, "url": url}
        if rs_alive:
            r = solve(rs_url, url, timeout_ms)
            row["rs"] = r
            print(f"  rs: {'OK' if r['ok'] else 'FAIL'} {fmt_ms(r['elapsed_ms'])}{'  [cf_clearance]' if r['has_clearance'] else ''}")
        if py_alive:
            r = solve(py_url, url, timeout_ms)
            row["py"] = r
            print(f"  py: {'OK' if r['ok'] else 'FAIL'} {fmt_ms(r['elapsed_ms'])}{'  [cf_clearance]' if r['has_clearance'] else ''}")
        results.append(row)
    return results


def aggregate_runs(all_runs: list[list[dict]], keys: list[str]) -> list[dict]:
    """Average per-indexer metrics across runs for each service key."""
    if not all_runs:
        return []
    # Index run results by indexer name
    by_name: dict[str, list[dict]] = {}
    for run in all_runs:
        for row in run:
            by_name.setdefault(row["name"], []).append(row)

    aggregated = []
    for name, rows in by_name.items():
        agg: dict = {"name": name, "url": rows[0].get("url")}
        for key in keys:
            keyed = [r[key] for r in rows if key in r]
            if not keyed:
                continue
            ok_count = sum(1 for r in keyed if r["ok"])
            clearance_count = sum(1 for r in keyed if r["has_clearance"])
            ok_times = [r["elapsed_ms"] for r in keyed if r["ok"]]
            agg[key] = {
                "ok": ok_count > len(keyed) // 2,        # majority pass = pass
                "ok_rate": ok_count / len(keyed),
                "elapsed_ms": int(sum(ok_times) / len(ok_times)) if ok_times else 0,
                "has_clearance": clearance_count > 0,
                "n_ok": ok_count,
                "n": len(keyed),
            }
        aggregated.append(agg)
    return aggregated


def summary_stats(rows: list[dict], key: str) -> dict:
    keyed = [r[key] for r in rows if key in r]
    ok_rows = [r for r in keyed if r["ok"]]
    times = [r["elapsed_ms"] for r in ok_rows]
    sorted_t = sorted(times)
    return {
        "passed": len(ok_rows),
        "total": len(keyed),
        "clearance": sum(1 for r in ok_rows if r["has_clearance"]),
        "avg_ms": int(sum(times) / len(times)) if times else 0,
        "p50": sorted_t[len(sorted_t) // 2] if sorted_t else 0,
        "p95": sorted_t[int(len(sorted_t) * 0.95)] if sorted_t else 0,
    }


def build_report(
    agg_results: list[dict],
    all_runs: list[list[dict]],
    rs_url: str, py_url: str,
    rs_alive: bool, py_alive: bool,
    num_runs: int,
) -> str:
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    lines = [
        "# FlareSolverr Benchmark Report",
        "",
        f"**Date:** {now}  ",
        f"**flaresolverr-rs:** {rs_url}  ",
        f"**flaresolverr-py:** {py_url}  ",
        f"**Definitions tested:** {len(agg_results)}  ",
        f"**Runs:** {num_runs}",
        "",
    ]

    if num_runs > 1:
        lines += ["_Latency figures are averaged across all runs. Pass/fail = majority vote._", ""]

    alive_keys = []
    if rs_alive:
        alive_keys.append(("flaresolverr-rs", "rs"))
    if py_alive:
        alive_keys.append(("flaresolverr-py", "py"))

    for label, key in alive_keys:
        s = summary_stats(agg_results, key)
        lines += [
            f"## {label}",
            "",
            f"| Metric | Value |",
            f"|--------|-------|",
            f"| Passed | {s['passed']} / {s['total']} ({s['passed']*100//s['total'] if s['total'] else 0}%) |",
            f"| CF clearance obtained | {s['clearance']} |",
            f"| Avg latency (success) | {fmt_ms(s['avg_ms'])} |",
            f"| p50 latency | {fmt_ms(s['p50'])} |",
            f"| p95 latency | {fmt_ms(s['p95'])} |",
            "",
        ]

    # Per-run summary table (only when multi-run)
    if num_runs > 1:
        lines += ["## Per-Run Summary", ""]
        run_headers = ["Run"]
        if rs_alive:
            run_headers += ["RS Passed", "RS Avg"]
        if py_alive:
            run_headers += ["PY Passed", "PY Avg"]
        lines.append("| " + " | ".join(run_headers) + " |")
        lines.append("| " + " | ".join(["---"] * len(run_headers)) + " |")
        for idx, run in enumerate(all_runs, 1):
            row = [str(idx)]
            if rs_alive:
                ok = [r["rs"] for r in run if "rs" in r and r["rs"]["ok"]]
                total = sum(1 for r in run if "rs" in r)
                avg = int(sum(r["elapsed_ms"] for r in ok) / len(ok)) if ok else 0
                row += [f"{len(ok)}/{total}", fmt_ms(avg)]
            if py_alive:
                ok = [r["py"] for r in run if "py" in r and r["py"]["ok"]]
                total = sum(1 for r in run if "py" in r)
                avg = int(sum(r["elapsed_ms"] for r in ok) / len(ok)) if ok else 0
                row += [f"{len(ok)}/{total}", fmt_ms(avg)]
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")

    # Per-indexer averaged table
    lines += ["## Per-Indexer Results", ""]
    col_headers = ["Indexer", "URL"]
    if rs_alive:
        col_headers += ["RS Status", "RS Avg Time", "RS CF"]
    if py_alive:
        col_headers += ["PY Status", "PY Avg Time", "PY CF"]
    lines.append("| " + " | ".join(col_headers) + " |")
    lines.append("| " + " | ".join(["---"] * len(col_headers)) + " |")

    for r in agg_results:
        row = [r["name"], f"[link]({r['url']})" if r.get("url") else "N/A"]
        for key in (["rs"] if rs_alive else []) + (["py"] if py_alive else []):
            if key not in r:
                row += ["—", "—", "—"]
                continue
            d = r[key]
            ok_str = "✅" if d["ok"] else "❌"
            if num_runs > 1 and d["n"] > 1:
                ok_str += f" ({d['n_ok']}/{d['n']})"
            row += [
                ok_str,
                fmt_ms(d["elapsed_ms"]) if d["elapsed_ms"] else "—",
                "🍪" if d["has_clearance"] else ("—" if d["ok"] else "✗"),
            ]
        lines.append("| " + " | ".join(row) + " |")

    lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Benchmark flaresolverr-rs vs flaresolverr-py")
    parser.add_argument("--rs-url", default="http://localhost:8191")
    parser.add_argument("--py-url", default="http://localhost:8192")
    parser.add_argument("--limit", type=int, default=20, help="Max definitions to test (0=all)")
    parser.add_argument("--timeout", type=int, default=60, help="Timeout per request in seconds")
    parser.add_argument("--runs", type=int, default=1, help="Number of benchmark passes to average")
    args = parser.parse_args()

    timeout_ms = args.timeout * 1000

    print("Checking services...")
    rs_alive = check_alive(args.rs_url, "flaresolverr-rs")
    py_alive = check_alive(args.py_url, "flaresolverr-py")
    if not rs_alive and not py_alive:
        print("ERROR: Both services are down.")
        sys.exit(1)

    definitions = fetch_definitions(args.limit)
    alive_keys = (["rs"] if rs_alive else []) + (["py"] if py_alive else [])

    all_runs: list[list[dict]] = []
    for run_num in range(1, args.runs + 1):
        if args.runs > 1:
            print(f"\n{'='*60}")
            print(f"  RUN {run_num} / {args.runs}")
            print(f"{'='*60}")
        run_results = run_once(
            definitions, args.rs_url, args.py_url,
            rs_alive, py_alive, timeout_ms, run_num, args.runs,
        )
        all_runs.append(run_results)

    agg = aggregate_runs(all_runs, alive_keys)
    report = build_report(agg, all_runs, args.rs_url, args.py_url, rs_alive, py_alive, args.runs)

    bench_dir = Path(__file__).parent
    ts = datetime.now().strftime("%Y%m%d-%H%M%S")
    report_path = bench_dir / f"report-{ts}.md"
    report_path.write_text(report, encoding="utf-8")
    print(f"\nReport written to {report_path}")


if __name__ == "__main__":
    main()
