#!/bin/sh
# sandbox-manager container entrypoint: apply pending migrations (when a
# database is configured) then exec the service. Lease/snapshot state is
# still in-memory as of this migration (S3.3 step 1 — the workspace_files
# manifest schema has no consumer yet); DATABASE_URL is therefore optional
# here, unlike capability-core's entrypoint, which always requires one.
# This will change once a later S3.3 step ports lease.Store/snapshot.Store
# onto Postgres and cmd/main.go starts requiring DATABASE_URL itself.
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
