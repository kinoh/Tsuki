FROM rust:1.89-bookworm AS builder

WORKDIR /app

COPY mcp/shell-exec/Cargo.toml mcp/shell-exec/Cargo.lock ./mcp/shell-exec/
COPY mcp/shell-exec/src ./mcp/shell-exec/src

WORKDIR /app/mcp/shell-exec
RUN cargo build --release

FROM ubuntu:24.04

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

RUN echo "nameserver 1.1.1.1" > /etc/resolv.conf

RUN useradd -m -s /bin/bash sandbox \
  && mkdir -p /work \
  && chown -R sandbox:sandbox /work

COPY --from=builder /app/mcp/shell-exec/target/release/shell-exec /usr/local/bin/shell-exec

USER sandbox
WORKDIR /work

ENV MCP_HTTP_BIND=0.0.0.0:8000 \
    MCP_HTTP_PATH=/mcp

ENTRYPOINT ["shell-exec"]
