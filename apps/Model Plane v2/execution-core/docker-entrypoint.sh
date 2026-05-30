#!/bin/sh
set -e

echo "=== execution-core starting ==="
echo "  PORT=${PORT:-8003}"
echo "  POSTGRES_HOST=${POSTGRES_HOST:-reasoning-postgres}"
echo "  NATS_URL=${NATS_URL:-nats://velion-nats:4222}"
echo "  OBJECT_STORAGE_ENDPOINT=${OBJECT_STORAGE_ENDPOINT:-http://minio:9000}"

exec python -m uvicorn app.main:app \
    --host "${HOST:-0.0.0.0}" \
    --port "${PORT:-8003}" \
    --log-level "${LOG_LEVEL:-info}" \
    --no-access-log
