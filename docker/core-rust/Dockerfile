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

RUN set -eux; \
    model_dir="/opt/tsuki/models/quantized-stable-static-embedding-fast-retrieval-mrl-ja"; \
    mkdir -p "${model_dir}"; \
    base_url="https://huggingface.co/RikkaBotan/quantized-stable-static-embedding-fast-retrieval-mrl-ja/resolve/e092037cf8c85a3f1325bfa22d2797f3bc13594e"; \
    wget -O "${model_dir}/tokenizer.json" "${base_url}/tokenizer.json"; \
    wget -O "${model_dir}/model_rest.safetensors" "${base_url}/model_rest.safetensors"; \
    wget -O "${model_dir}/embedding.q4_k_m.bin" "${base_url}/embedding.q4_k_m.bin"; \
    test -s "${model_dir}/tokenizer.json"; \
    test -s "${model_dir}/model_rest.safetensors"; \
    test -s "${model_dir}/embedding.q4_k_m.bin"

COPY --from=builder /tmp/tsuki-core-rust /usr/local/bin/tsuki-core-rust
COPY --from=builder /tmp/config.toml /app/config.toml

ENV RUST_LOG=info
ENV TZ=UTC

EXPOSE 2953

ENTRYPOINT ["/usr/local/bin/tsuki-core-rust"]
