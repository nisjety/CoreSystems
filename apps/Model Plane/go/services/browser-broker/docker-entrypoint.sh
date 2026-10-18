#!/bin/sh
# browser-broker container entrypoint: apply the grant-store migrations (when a
# database is configured) then exec the service. Copied from sandbox-manager's
# entrypoint. Every *.up.sql is re-applied on each start (gaps between numbers
# are tolerated), so every statement in migrations/ must be idempotent
# (R-3: IF NOT EXISTS / ADD COLUMN IF NOT EXISTS).
#
# DATABASE_URL is optional here only because cmd/main.go itself refuses to
# start without it unless BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT=true (dev
# only). That gate is enforced in Go, not here; this script just skips
# migrations it has nothing to run them against.
set -eu

if [ -n "${DATABASE_URL:-}" ]; then
  for migration in /app/migrations/*.up.sql; do
    [ -f "$migration" ] || continue
    if ! psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$migration" >/tmp/browser-broker-migrate.log 2>&1; then
      cat /tmp/browser-broker-migrate.log >&2
      exit 1
    fi
  done
  rm -f /tmp/browser-broker-migrate.log
else
  echo "DATABASE_URL not set; skipping migrations (grants are in-memory)" >&2
fi

exec "$@"
