#!/bin/sh
set -eu

mkdir -p /data

if [ ! -f /data/prompts.md ]; then
  cp /app/default/prompts.md /data/prompts.md
fi

exec /usr/local/bin/tsuki-core-rust
