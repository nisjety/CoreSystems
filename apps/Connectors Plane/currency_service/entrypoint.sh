#!/bin/sh
set -e

# Fix cache directory permissions after bind mount overrides /app/cache.
# The bind mount (./cache:/app/cache) is owned by root on the host, so
# appuser cannot create subdirectories. We run this as root first.
mkdir -p /app/cache/rates
chown -R appuser:appuser /app/cache

# Drop to non-root user and exec the given command (e.g. python app.py)
exec gosu appuser "$@"
