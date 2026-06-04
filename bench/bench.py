#!/usr/bin/env python3
"""
Benchmark flaresolverr-rs vs flaresolverr-py against Prowlarr v11 definitions.

Usage:
    python bench/bench.py [--rs-url URL] [--py-url URL] [--limit N] [--timeout S]

Outputs a Markdown report to bench/report-YYYYMMDD-HHMMSS.md
"""
import argparse
import json
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
    except Exception as e:
        return None


def solve(base_url: str, target_url: str, timeout_ms: int) -> dict:
    payload = {"cmd": "request.get", "url": target_url, "maxTimeout": timeout_ms}
    t0 = time.monotonic()
    try:
        resp = requests.post(f"{base_url}/v1", json=payload, timeout=timeout_ms / 1000 + 10)
        elapsed = int((time.monotonic() - t0) * 1000)
        data = resp.json()
        status = data.get("status", "error")
        solution = data.get("solution") or {}
        cookies = solution.get("cookies") or []
        has_clearance = any(c.get("name") == "cf_clearance" for c in cookies)
        http_status = solution.get("status", 0)
        return {
            "ok": status == "ok",
            "elapsed_ms": elapsed,
            "http_status": http_status,
            "has_clearance": has_clearance,
            "message": data.get("message", ""),
        }
    except Exception as e:
        elapsed = int((time.monotonic() - t0) * 1000)
        return {"ok": False, "elapsed_ms": elapsed, "http_status": 0, "has_clearance": False, "message": str(e)}


def check_alive(url: str, label: str) -> bool:
    try:
        resp = requests.get(f"{url}/health", timeout=5)
        data = resp.json()
        if data.get("status") == "ok":
            print(f"  {label} ({url}): UP")
            return True
    except Exception:
        pass
    print(f"  {label} ({url}): DOWN (skipping)")
    return False


def fmt_ms(ms: int) -> str:
    return f"{ms:,}ms" if ms < 1000 else f"{ms/1000:.1f}s"


def build_report(results: list[dict], rs_url: str, py_url: str, rs_alive: bool, py_alive: bool) -> str:
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    lines = [
        f"# FlareSolverr Benchmark Report",
        f"",
        f"**Date:** {now}  ",
        f"**flaresolverr-rs:** {rs_url}  ",
        f"**flaresolverr-py:** {py_url}  ",
        f"**Definitions tested:** {len(results)}",
        f"",
    ]

    for label, key, alive in [("flaresolverr-rs", "rs", rs_alive), ("flaresolverr-py", "py", py_alive)]:
        if not alive:
            lines += [f"## {label}", "", "_Service was not available during benchmark._", ""]
            continue

        rows = [r[key] for r in results if key in r]
        ok_rows = [r for r in rows if r["ok"]]
        fail_rows = [r for r in rows if not r["ok"]]
        clearance_rows = [r for r in ok_rows if r["has_clearance"]]
        times = [r["elapsed_ms"] for r in ok_rows]
        avg_ms = int(sum(times) / len(times)) if times else 0
        p50 = sorted(times)[len(times) // 2] if times else 0
        p95 = sorted(times)[int(len(times) * 0.95)] if times else 0

        lines += [
            f"## {label}",
            f"",
            f"| Metric | Value |",
            f"|--------|-------|",
            f"| Passed | {len(ok_rows)} / {len(rows)} |",
            f"| Failed | {len(fail_rows)} |",
            f"| CF clearance obtained | {len(clearance_rows)} |",
            f"| Avg latency (success) | {fmt_ms(avg_ms)} |",
            f"| p50 latency | {fmt_ms(p50)} |",
            f"| p95 latency | {fmt_ms(p95)} |",
            f"",
        ]

    # Per-indexer table
    lines += ["## Per-Indexer Results", ""]
    headers = ["Indexer", "URL"]
    if rs_alive:
        headers += ["RS Status", "RS Time", "RS CF"]
    if py_alive:
        headers += ["PY Status", "PY Time", "PY CF"]
    lines.append("| " + " | ".join(headers) + " |")
    lines.append("| " + " | ".join(["---"] * len(headers)) + " |")

    for r in results:
        row = [r["name"], f"[link]({r['url']})" if r.get("url") else "N/A"]
        if rs_alive and "rs" in r:
            d = r["rs"]
            row += [
                "✅" if d["ok"] else "❌",
                fmt_ms(d["elapsed_ms"]),
                "🍪" if d["has_clearance"] else ("—" if d["ok"] else "✗"),
            ]
        if py_alive and "py" in r:
            d = r["py"]
            row += [
                "✅" if d["ok"] else "❌",
                fmt_ms(d["elapsed_ms"]),
                "🍪" if d["has_clearance"] else ("—" if d["ok"] else "✗"),
            ]
        lines.append("| " + " | ".join(row) + " |")

    lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="Benchmark flaresolverr-rs vs flaresolverr-py")
    parser.add_argument("--rs-url", default="http://localhost:8191", help="flaresolverr-rs base URL")
    parser.add_argument("--py-url", default="http://localhost:8192", help="flaresolverr-py base URL")
    parser.add_argument("--limit", type=int, default=20, help="Max definitions to test (0=all)")
    parser.add_argument("--timeout", type=int, default=60, help="Timeout per request in seconds")
    args = parser.parse_args()

    timeout_ms = args.timeout * 1000

    print("Checking services...")
    rs_alive = check_alive(args.rs_url, "flaresolverr-rs")
    py_alive = check_alive(args.py_url, "flaresolverr-py")

    if not rs_alive and not py_alive:
        print("ERROR: Both services are down. Exiting.")
        sys.exit(1)

    definitions = fetch_definitions(args.limit)
    results = []

    for i, entry in enumerate(definitions, 1):
        name = entry["name"].removesuffix(".yml")
        download_url = entry.get("download_url")
        print(f"[{i}/{len(definitions)}] {name}")

        url = None
        if download_url:
            url = get_first_link(download_url)

        if not url:
            print(f"  SKIP: no URL found")
            results.append({"name": name, "url": None})
            continue

        print(f"  URL: {url}")
        row = {"name": name, "url": url}

        if rs_alive:
            r = solve(args.rs_url, url, timeout_ms)
            row["rs"] = r
            status = "OK" if r["ok"] else "FAIL"
            cf = " [cf_clearance]" if r["has_clearance"] else ""
            print(f"  rs: {status} {fmt_ms(r['elapsed_ms'])}{cf}")

        if py_alive:
            r = solve(args.py_url, url, timeout_ms)
            row["py"] = r
            status = "OK" if r["ok"] else "FAIL"
            cf = " [cf_clearance]" if r["has_clearance"] else ""
            print(f"  py: {status} {fmt_ms(r['elapsed_ms'])}{cf}")

        results.append(row)

    report = build_report(results, args.rs_url, args.py_url, rs_alive, py_alive)

    bench_dir = Path(__file__).parent
    ts = datetime.now().strftime("%Y%m%d-%H%M%S")
    report_path = bench_dir / f"report-{ts}.md"
    report_path.write_text(report, encoding="utf-8")
    print(f"\nReport written to {report_path}")


if __name__ == "__main__":
    main()
