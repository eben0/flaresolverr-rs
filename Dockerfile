# syntax=docker/dockerfile:1.7
FROM rust:slim AS builder
WORKDIR /build/app
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates git mold \
    && rm -rf /var/lib/apt/lists/*

COPY . /build/app
RUN cargo build --release --bin flaresolverr-rs

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
        chromium \
        fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

# Chrome refuses to run as root; use an unprivileged user.
RUN useradd -m -u 1000 -s /bin/sh app
COPY --from=builder /build/app/target/release/flaresolverr-rs /usr/local/bin/flaresolverr-rs
COPY --from=builder /build/app/config.toml /app/config.toml

USER app
WORKDIR /app
EXPOSE 8191
ENTRYPOINT ["/usr/local/bin/flaresolverr-rs"]
