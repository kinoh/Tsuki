#!/usr/bin/env sh

set -eu

if [ -w /etc/resolv.conf ]; then
  echo "nameserver 1.1.1.1" > /etc/resolv.conf
else
  echo "warning: /etc/resolv.conf is not writable" >&2
fi

exec shell-exec
