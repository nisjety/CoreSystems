#!/bin/sh
set -e
exec uvicorn app.main:app --host 0.0.0.0 --port 8005 --log-level info
