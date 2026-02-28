FROM rust:1.89-bookworm AS builder

WORKDIR /workspace/core-rust

COPY core-rust/Cargo.toml core-rust/Cargo.lock ./
COPY core-rust/src ./src
COPY core-rust/static ./static
COPY core-rust/config.toml core-rust/config.prod.toml ./
COPY api-specs ../api-specs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/core-rust/target \
    cargo build --release --bin tsuki-core-rust --bin merge_toml \
    && /workspace/core-rust/target/release/merge_toml \
      --base /workspace/core-rust/config.toml \
      --overlay /workspace/core-rust/config.prod.toml \
      --output /tmp/config.toml \
    && cp /workspace/core-rust/target/release/tsuki-core-rust /tmp/tsuki-core-rust

FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /tmp/tsuki-core-rust /usr/local/bin/tsuki-core-rust
COPY --from=builder /tmp/config.toml /app/config.toml
COPY core-rust/scripts/container-entrypoint.sh /usr/local/bin/container-entrypoint.sh

RUN chmod +x /usr/local/bin/container-entrypoint.sh

ENV RUST_LOG=info
ENV TZ=UTC

EXPOSE 2953

ENTRYPOINT ["/usr/local/bin/container-entrypoint.sh"]
