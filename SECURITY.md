# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x (latest) | Yes |
| < 0.1.0 | No |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Open a [GitHub Security Advisory](https://github.com/eben0/flaresolverr-rs/security/advisories/new) or email via your GitHub profile with:
- A description of the vulnerability
- Steps to reproduce
- Potential impact

You can expect an acknowledgement within 48 hours and a resolution timeline within 7 days for confirmed issues.

---

## Security Scanning

CI runs two automated security checks on every push and pull request to `main`.

### cargo audit — dependency vulnerability scan

Checks all dependencies against the [RustSec Advisory Database](https://rustsec.org/).

**Run locally:**
```bash
cargo install cargo-audit
cargo audit
```

**Current status (as of 2026-05-23):** 0 vulnerabilities across 254 dependencies.

### cargo clippy — static analysis

Enforces Rust best practices and catches common error patterns. CI runs with `-D warnings` (warnings are errors).

**Run locally:**
```bash
cargo clippy -- -D warnings
```

**Current status:** Clean (0 warnings).

### semgrep — SAST scan

Runs [Semgrep](https://semgrep.dev/) with the default ruleset on every push and weekly on a schedule. Results are uploaded to GitHub Code Scanning as SARIF.

**Run locally:**
```bash
pip install semgrep
semgrep scan --config p/default
```

---

## Threat Model

This service runs a headless Chromium instance and proxies arbitrary URLs. Key considerations:

- **SSRF**: The server will fetch any URL provided in `request.get` / `request.post`. Do not expose port 8191 to untrusted networks. Run behind a firewall or authenticated reverse proxy.
- **Proxy credentials**: Proxy URLs with embedded credentials (`user:pass@host`) are visible in the Chrome process list and are **not** forwarded by Chrome's `--proxy-server` flag. Use CDP proxy auth events instead.
- **Resource limits**: No built-in rate limiting. A caller can exhaust system resources by flooding the endpoint with concurrent requests. Apply rate limiting at the reverse proxy layer.
- **Data isolation**: Named sessions share a single Chrome process (isolated browser contexts per request). Ephemeral requests use browser context isolation — cookies and storage do not leak between requests.
