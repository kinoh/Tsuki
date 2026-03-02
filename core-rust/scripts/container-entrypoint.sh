#!/bin/sh
set -eu

mkdir -p /data

if [ "${1:-}" = "backfill" ]; then
  exec /usr/local/bin/tsuki-core-rust "$@"
fi

if [ ! -f /data/prompts.md ]; then
  echo "PROMPTS_MISSING error=/data/prompts.md is required" >&2
  exit 1
fi

exec /usr/local/bin/tsuki-core-rust "$@"
