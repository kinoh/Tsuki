FROM rustlang/rust:nightly as build

WORKDIR /app

RUN curl -sSLf -o /tmp/vosk.zip https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-linux-x86_64-0.3.45.zip \
    && unzip -qj -d /tmp /tmp/vosk.zip vosk-linux-x86_64-0.3.45/libvosk.so \
    && install /tmp/libvosk.so /usr/local/lib \
    && rm /tmp/vosk.zip /tmp/libvosk.so
RUN apt-get update \
    && apt-get -y install cmake protobuf-compiler \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=build.rs,target=build.rs \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    <<EOF
set -e
cargo build --locked --release
cp ./target/release/tsuki /bin/
EOF

FROM debian:stable-slim as final

RUN apt-get update \
    && apt-get install -y ca-certificates \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser
USER appuser

ENV LD_LIBRARY_PATH="/usr/local/lib"

COPY --from=build /usr/local/lib/libvosk.so /usr/local/lib/
COPY --from=build /bin/tsuki /bin/

ENTRYPOINT ["/bin/tsuki"]
