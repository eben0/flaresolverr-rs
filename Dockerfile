# syntax=docker/dockerfile:1

ARG VERSION=latest

# ── Runtime ────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

ARG VERSION

# Chrome shared-library deps + Xvfb for virtual display
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        wget gnupg ca-certificates libssl3 \
        fonts-liberation libnss3 libatk1.0-0 libatk-bridge2.0-0 \
        libcups2 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 \
        libxrandr2 libgbm1 libasound2 xvfb \
    && wget -qO- https://dl.google.com/linux/linux_signing_key.pub \
       | gpg --dearmor > /usr/share/keyrings/google-chrome.gpg \
    && echo "deb [arch=amd64 signed-by=/usr/share/keyrings/google-chrome.gpg] \
       http://dl.google.com/linux/chrome/deb/ stable main" \
       > /etc/apt/sources.list.d/google-chrome.list \
    && apt-get update && apt-get install -y --no-install-recommends google-chrome-stable

WORKDIR /app

# Download pre-built binary from GitHub releases
RUN if [ "$VERSION" = "latest" ]; then \
        URL="https://github.com/eben0/flaresolverr-rs/releases/latest/download/flaresolverr-rs-linux-amd64"; \
    else \
        URL="https://github.com/eben0/flaresolverr-rs/releases/download/v${VERSION}/flaresolverr-rs-linux-amd64"; \
    fi \
    && wget -qO flaresolverr-rs "$URL" \
    && chmod +x flaresolverr-rs

COPY config.toml ./

# On Linux, headed Chrome via Xvfb is the reliable CF-bypass path.
# Flip virtual_display on and keep headless off so chaser-cf starts Xvfb.
RUN sed -i 's/^headless\s*=.*/headless        = false/' config.toml \
 && sed -i 's/^virtual_display\s*=.*/virtual_display = true/' config.toml

EXPOSE 8191
CMD ["./flaresolverr-rs"]
