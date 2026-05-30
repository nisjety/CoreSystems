#!/bin/sh
set -e

exec uvicorn app.main:app \
    --host 0.0.0.0 \
    --port "${PORT:-8004}" \
    --log-level "${LOG_LEVEL:-info}" \
    --no-access-log
