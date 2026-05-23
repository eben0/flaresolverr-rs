#!/usr/bin/env python3
"""
Performance comparison: FlareSolverr Python vs flaresolverr-rs

Usage:
    cd bench/
    docker compose up -d
    pip install -r requirements.txt
    python benchmark.py [--requests N] [--cf]

Flags:
    --requests N   Number of requests per test (default: 5)
    --cf           Also test against Cloudflare-protected sites (slower)
"""

import argparse
import statistics
import sys
import time
from typing import Optional

import requests
from tabulate import tabulate

PY_URL = "http://localhost:8191/v1"
RS_URL = "http://localhost:8192/v1"

PLAIN_URLS = [
    ("example.com",   "https://example.com"),
    ("httpbin.org",   "https://httpbin.org/get"),
]

CF_URLS = [
    ("nowsecure.nl",  "https://nowsecure.nl"),
    ("cfspeedtest",   "https://www.cloudflare.com/cdn-cgi/trace"),
]


# ── helpers ───────────────────────────────────────────────────────────────────

def fetch(base_url: str, url: str, session_id: Optional[str] = None, timeout_s: int = 60) -> tuple[float, bool, int]:
    """Returns (elapsed_s, success, http_status)."""
    payload = {"cmd": "request.get", "url": url, "maxTimeout": timeout_s * 1000}
    if session_id:
        payload["session"] = session_id
    t0 = time.perf_counter()
    try:
        r = requests.post(base_url, json=payload, timeout=timeout_s + 10)
        elapsed = time.perf_counter() - t0
        body = r.json()
        ok = body.get("status") == "ok"
        status = body.get("solution", {}).get("status", 0) if ok else 0
        return elapsed, ok, status
    except Exception as e:
        return time.perf_counter() - t0, False, 0


def create_session(base_url: str) -> Optional[str]:
    try:
        r = requests.post(base_url, json={"cmd": "sessions.create"}, timeout=30)
        body = r.json()
        if body.get("status") == "ok":
            msg = body.get("message", "")
            prefix = "Session created: "
            if prefix in msg:
                return msg.split(prefix, 1)[1].strip()
    except Exception:
        pass
    return None


def destroy_session(base_url: str, session_id: str) -> None:
    try:
        requests.post(
            base_url,
            json={"cmd": "sessions.destroy", "session": session_id},
            timeout=10,
        )
    except Exception:
        pass


def check_alive(name: str, base_url: str) -> bool:
    for _ in range(3):
        try:
            requests.post(base_url, json={"cmd": "sessions.list"}, timeout=5)
            print(f"  ✓ {name} reachable at {base_url}")
            return True
        except Exception:
            time.sleep(2)
    print(f"  ✗ {name} NOT reachable at {base_url} — is docker compose up?")
    return False


# ── benchmark core ────────────────────────────────────────────────────────────

def run_mode(base_url: str, test_url: str, n: int, mode: str) -> dict:
    """
    mode: "ephemeral" — new Chrome per request
          "session"   — shared Chrome, first request is cold
    """
    times, errors = [], 0
    session_id = None

    if mode == "session":
        session_id = create_session(base_url)
        if not session_id:
            return {"times": [], "errors": n, "cold": None, "warm": []}

    for i in range(n):
        elapsed, ok, _ = fetch(base_url, test_url, session_id=session_id)
        if ok:
            times.append(elapsed)
        else:
            errors += 1

    if session_id:
        destroy_session(base_url, session_id)

    cold = times[0] if times else None
    warm = times[1:] if len(times) > 1 else []
    return {"times": times, "errors": errors, "cold": cold, "warm": warm}


def run_pair(py_url: str, rs_url: str, label: str, test_url: str, n: int) -> dict:
    results = {}
    for svc_name, base_url in [("py", py_url), ("rs", rs_url)]:
        svc_label = "Python" if svc_name == "py" else "Rust"
        print(f"\n  [{svc_label}] {label}")
        for mode in ("ephemeral", "session"):
            print(f"    {mode:10s} ...", end="", flush=True)
            r = run_mode(base_url, test_url, n, mode)
            for t in r["times"]:
                print(f" {t:.2f}s", end="", flush=True)
            if r["errors"]:
                print(f" ({r['errors']} errors)", end="")
            print()
            results.setdefault(svc_name, {})[mode] = r
    return results


# ── output ────────────────────────────────────────────────────────────────────

def _stats(times: list[float]) -> str:
    if not times:
        return "—"
    avg = statistics.mean(times)
    if len(times) == 1:
        return f"{avg:.2f}s"
    p95_idx = max(0, int(len(times) * 0.95) - 1)
    p95 = sorted(times)[p95_idx]
    return f"avg {avg:.2f}s  p95 {p95:.2f}s"


def _speedup(py_times: list[float], rs_times: list[float]) -> str:
    if not py_times or not rs_times:
        return ""
    ratio = statistics.mean(py_times) / statistics.mean(rs_times)
    if ratio > 1:
        return f"Rust {ratio:.1f}× faster"
    else:
        return f"Rust {1/ratio:.1f}× slower"


def print_report(all_results: dict) -> None:
    rows = []
    for label, pair in all_results.items():
        for mode in ("ephemeral", "session"):
            py = pair.get("py", {}).get(mode, {})
            rs = pair.get("rs", {}).get(mode, {})

            py_times = py.get("times", [])
            rs_times = rs.get("times", [])

            # For session mode, split cold vs warm
            if mode == "session":
                py_cold = f"{py.get('cold'):.2f}s" if py.get("cold") else "—"
                rs_cold = f"{rs.get('cold'):.2f}s" if rs.get("cold") else "—"
                py_warm = _stats(py.get("warm", []))
                rs_warm = _stats(rs.get("warm", []))
                rows.append([label, "session cold", py_cold, rs_cold, _speedup([py.get("cold")] if py.get("cold") else [], [rs.get("cold")] if rs.get("cold") else [])])
                rows.append(["", "session warm", py_warm, rs_warm, _speedup(py.get("warm", []), rs.get("warm", []))])
            else:
                rows.append([label, "ephemeral", _stats(py_times), _stats(rs_times), _speedup(py_times, rs_times)])

    print("\n" + "=" * 85)
    print(tabulate(
        rows,
        headers=["URL", "Mode", "Python", "Rust", "Δ"],
        tablefmt="simple",
    ))
    print("=" * 85)
    print()
    print("Notes:")
    print("  ephemeral  = new Chrome instance per request (includes browser startup)")
    print("  session cold = first request on a named session (browser startup + CF challenge)")
    print("  session warm = subsequent requests on same session (bypasses challenge)")


# ── main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark FlareSolverr Python vs Rust")
    parser.add_argument("--requests", type=int, default=5, metavar="N",
                        help="requests per test (default: 5)")
    parser.add_argument("--cf", action="store_true",
                        help="include Cloudflare-protected test URLs")
    args = parser.parse_args()

    print("=" * 60)
    print("  FlareSolverr Python vs Rust — Performance Benchmark")
    print("=" * 60)
    print(f"  Python : {PY_URL}")
    print(f"  Rust   : {RS_URL}")
    print(f"  N      : {args.requests} requests per test\n")

    print("Checking services...")
    ok_py = check_alive("Python", PY_URL)
    ok_rs = check_alive("Rust",   RS_URL)
    if not (ok_py and ok_rs):
        print("\nStart both services first:\n  docker compose up -d")
        sys.exit(1)

    urls = PLAIN_URLS + (CF_URLS if args.cf else [])
    all_results: dict = {}

    for label, test_url in urls:
        print(f"\n{'─' * 60}")
        print(f"  {label}  ({test_url})")
        all_results[label] = run_pair(PY_URL, RS_URL, label, test_url, args.requests)

    print_report(all_results)


if __name__ == "__main__":
    main()
