#!/bin/sh
set -e

echo "=== agent-core v2 starting ==="
echo "  PORT=${PORT:-8002}"
echo "  POSTGRES_HOST=${POSTGRES_HOST:-postgres}"
echo "  NATS_URL=${NATS_URL:-nats://velion-nats:4222}"

exec python -m uvicorn app.main:app \
    --host "${HOST:-0.0.0.0}" \
    --port "${PORT:-8002}" \
    --log-level "${LOG_LEVEL:-info}" \
    --no-access-log
