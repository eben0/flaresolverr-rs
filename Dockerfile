# syntax=docker/dockerfile:1.7
ARG VERSION

FROM debian:trixie-slim AS downloader
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

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 \
        chromium \
        fonts-liberation \
    && rm -rf /var/lib/apt/lists/*

# Chrome refuses to run as root; use an unprivileged user.
RUN useradd -m -u 1000 -s /bin/sh app
COPY --from=downloader /usr/local/bin/flaresolverr-rs /usr/local/bin/flaresolverr-rs
COPY config.toml /app/config.toml

USER app
WORKDIR /app
EXPOSE 8191
ENTRYPOINT ["/usr/local/bin/flaresolverr-rs"]
