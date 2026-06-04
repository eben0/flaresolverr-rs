# CloudFlare / Anti-Bot / Proxy Challenge Bypass Plan

## Skills
find, install and use the skills listed below:
- find-skills
- prompt-improver
- karpathy-guidelines
- leonardomso/rust-skills
- superpowers:*

## Problem
Some sites using APIs that are behind proxy such as CloudFlare and are protected by challenge.
Scripts that call those APIs are blocked by CF challenge.

## Solution
Build a proxy-like server on top of Rust create `chaser-cf`.

## Programming Language
Rust

## Libraries
- chaser-cf (latest version)

## HTTP Server
Translate `chaser-cf` [HTTP Server](https://github.com/0xchasercat/chaser-cf#http-server) into *Endpoints*.
Do not reinvent the wheel, just translate requests.

## Endpoints
**Mimics Request/Response**: `https://raw.githubusercontent.com/FlareSolverr/FlareSolverr/refs/heads/master/src/flaresolverr_service.py`
 - /v1
   - **Mimics Endpoint**: `controller_v1_endpoint`
 - /health
   - **Mimic Endpoint**: `health_endpoint`
 - /
   - **Mimic Endpoint**: `index_endpoint`

## Proxy
- Challenge is bypassed using a proxy (with authentication).

## tests
### Unittests
- place in `tests/` and not inline.
### Integration tests
- place in `integration/`.

## Core Requirements
- **All tests pass**: Run all tests until it works.
- **Prowlarr Definitions**: All links in definition files bypassed challenge and returning correct response.
- **Proxy**: Proxy and Proxy Authentication works.

### Integration Tests: Prowlarr Definitions 
- Fetch all definitions from the **Source**, for each definition file extract the first item in the `links` array.
- Run `chaser-cf` against this URL.
- requires Chrome browser + Xvfb / XQuartz
- Proxy URL and credentials fetched from environment variable `HTTPS_PROXY` from `.env.proxy`.

Example below: `https://1337x.to/`
```yaml
---
id: 1337x
name: 1337x
description: "1337x is a Public torrent site that offers verified torrent downloads"
language: en-US
type: public
encoding: UTF-8
requestDelay: 3
# get status and news on domains at the official site https://1337x-status.org/
links:
  - https://1337x.to/
  - https://1337x.st/
```
- *Source**: `https://github.com/Prowlarr/Indexers/tree/master/definitions/v11` 

## Target Platforms
- Docker
  - Runs without any special environment variables or config.
- Linux
- WSL

## Host
Windows 11, Cargo, Rust, PowerShell 7, Docker, WSL

## Flow
- compile, test, debug, if error - repeat.

## Code Style
- Write clean and enterprise grade code - maintainable, readable, clean, short.
- comment the code - short, descriptive and precise.
- use linter and prettifier.

## Rules
- Do not push
- *Core Requirements*

## Context:
- https://github.com/0xchasercat/chaser-cf
- https://crates.io/crates/chaser-cf

## Commit Conventions
- Never add "Co-authored-by" attributions to commit messages.
- Monolithic commits.
- Never push without user approval.