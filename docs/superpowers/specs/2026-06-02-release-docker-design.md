# Release & Docker Binary Distribution

**Date:** 2026-06-02

## Problem

`flaresolverr-rs` had no binary release artifacts. The Docker image compiled from source inside the build using a `rust:slim` builder stage, making Docker builds slow (~5 min) and coupling the Docker image to the Rust toolchain.

## Goals

1. Publish pre-built binaries to GitHub Releases on every version tag
2. Rewrite the Dockerfile to download the pre-built binary instead of compiling
3. Wire the Docker CI workflow to depend on the Release workflow, guaranteeing the binary exists before the Docker image is built

## Design Decisions

**Workflow chain:** `CI` → `Release` → `Docker` (each triggered via `workflow_run` from the previous)

**Platforms:** `linux/amd64` only initially (binary named `flaresolverr-rs-linux-amd64`)

**VERSION strategy:** The Docker workflow strips the `v` prefix from the tag (e.g. `v0.1.6` → `0.1.6`) and passes it as a build-arg. The Dockerfile re-adds the `v` prefix in the download URL. If `VERSION` is empty (local `docker build` with no arg), the Dockerfile falls back to the `/releases/latest/download/` redirect.

**PR Docker builds:** Dropped. CI already verifies compilation; the PR Docker check was redundant overhead that breaks with binary-download approach anyway.

**`docker/metadata-action` removed from Docker workflow:** Its semver tag patterns rely on `github.ref` being a tag ref, which is not the case in `workflow_run` context. Replaced with explicit tag construction from the extracted version string.

**`Dockerfile.build` preserved:** The original build-from-source Dockerfile is kept as `Dockerfile.build` for local development without a published release.

## Files Changed

| File | Change |
|------|--------|
| `.github/workflows/release.yml` | New — builds `linux-amd64` binary, publishes GitHub Release |
| `.github/workflows/docker.yml` | Modified — waits for Release, adds VERSION build-arg, drops PR trigger |
| `Dockerfile` | Replaced — downloads binary from GitHub Releases instead of compiling |
| `Dockerfile.build` | New — original build-from-source Dockerfile preserved for local dev |
