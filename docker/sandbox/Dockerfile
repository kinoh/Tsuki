FROM rust:1.89-bookworm AS builder

WORKDIR /app

COPY mcp/shell-exec/Cargo.toml mcp/shell-exec/Cargo.lock ./mcp/shell-exec/
COPY mcp/shell-exec/src ./mcp/shell-exec/src

WORKDIR /app/mcp/shell-exec
RUN cargo build --release

FROM node:20-bookworm-slim AS node-runtime

FROM ubuntu:24.04

COPY --from=node-runtime /usr/local/ /usr/local/

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    dnsutils \
    findutils \
    git \
    gzip \
    iputils-ping \
    jq \
    less \
    lsof \
    openssl \
    patch \
    python3 \
    python3-numpy \
    procps \
    sed \
    tar \
    unzip \
    wget \
    whois \
    xz-utils \
    zip \
  && rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash sandbox \
  && mkdir -p /memory \
  && mkdir -p /ms-playwright \
  && chown -R sandbox:sandbox /memory /ms-playwright

RUN npm install -g @playwright/cli@latest playwright@latest

ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright

RUN playwright install --with-deps firefox

COPY --from=builder /app/mcp/shell-exec/target/release/shell-exec /usr/local/bin/shell-exec
COPY docker/sandbox/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

WORKDIR /memory

ENV MCP_HTTP_BIND=0.0.0.0:8000 \
    MCP_HTTP_PATH=/mcp

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
