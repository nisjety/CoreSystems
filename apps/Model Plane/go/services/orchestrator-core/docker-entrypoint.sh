#!/bin/sh
# orchestrator-core container entrypoint: apply pending migrations (when a
# database is configured) then exec the service. When DATABASE_URL is unset the
# worker runs against its in-memory feedback store — ratings are then lost on
# restart, which the service logs at WARN — so migrations are skipped gracefully.
set -eu

if [ -n "${DATABASE_URL:-}" ]; then
  for migration in /app/migrations/*.up.sql; do
    [ -f "$migration" ] || continue
    if ! psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$migration" >/tmp/orchestrator-core-migrate.log 2>&1; then
      cat /tmp/orchestrator-core-migrate.log >&2
      exit 1
    fi
  done
  rm -f /tmp/orchestrator-core-migrate.log
else
  echo "DATABASE_URL not set; skipping migrations (in-memory feedback store)" >&2
fi

exec "$@"
