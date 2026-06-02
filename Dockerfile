# syntax=docker/dockerfile:1.7
ARG VERSION
ARG BINARY_SOURCE=ghcr

# Path A (default): download binary from GitHub Releases
FROM debian:trixie-slim AS from-ghcr
ARG VERSION
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && if [ -n "${VERSION}" ]; then \
         URL="https://github.com/eben0/flaresolverr-rs/releases/download/v${VERSION}/flaresolverr-rs-linux-amd64"; \
       else \
         URL="https://github.com/eben0/flaresolverr-rs/releases/latest/download/flaresolverr-rs-linux-amd64"; \
       fi \
    && curl -fL -o /usr/local/bin/flaresolverr-rs "$URL" \
    && chmod +x /usr/local/bin/flaresolverr-rs

# Path B (CI): use pre-built binary from build context (no network needed)
FROM scratch AS from-context
COPY flaresolverr-rs-linux-amd64 /usr/local/bin/flaresolverr-rs

# Select binary source via BINARY_SOURCE build-arg (BuildKit skips unused stages)
FROM from-${BINARY_SOURCE} AS binary-provider

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
        chromium \
        fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

# Chrome refuses to run as root; use an unprivileged user.
RUN useradd -m -u 1000 -s /bin/sh app
COPY --from=binary-provider /usr/local/bin/flaresolverr-rs /usr/local/bin/flaresolverr-rs
COPY config.toml /app/config.toml

USER app
WORKDIR /app
EXPOSE 8191
ENTRYPOINT ["/usr/local/bin/flaresolverr-rs"]
