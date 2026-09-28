#!/bin/sh
# sandbox-manager container entrypoint: apply pending migrations (when a
# database is configured) then exec the service. DATABASE_URL is optional
# here, unlike capability-core's entrypoint (which always requires one):
# cmd/main.go itself falls back to an in-memory lease/snapshot store when
# both DATABASE_URL and SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT=true are
# set (mirroring cost-core's own "runs against its in-memory ledger"
# precedent) — a real deployment (ephemeral development not opted in) still
# requires DATABASE_URL and fails closed without it, enforced in Go, not
# here; this script just skips applying migrations it has nothing to run
# them against.
set -eu

if [ -n "${DATABASE_URL:-}" ]; then
  for migration in /app/migrations/*.up.sql; do
    [ -f "$migration" ] || continue
    if ! psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$migration" >/tmp/sandbox-manager-migrate.log 2>&1; then
      cat /tmp/sandbox-manager-migrate.log >&2
      exit 1
    fi
  done
  rm -f /tmp/sandbox-manager-migrate.log
else
  echo "DATABASE_URL not set; skipping migrations (lease/snapshot state is in-memory)" >&2
fi

exec "$@"
