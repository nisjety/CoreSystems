#!/bin/sh
set -eu

# Apply Postgres migrations only when a DATABASE_URL is configured. letta-bridge
# can run on the in-memory or agent-memory-server backends with no Postgres at
# all, so an unset DATABASE_URL is not an error here (unlike capability-core).
# The service additionally ensures the schema in-code at startup, so these
# migrations are belt-and-suspenders for operators who manage schema out of band.
if [ -n "${DATABASE_URL:-}" ]; then
  for migration in /app/migrations/*.up.sql; do
    [ -f "$migration" ] || continue
    if ! psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$migration" >/tmp/letta-bridge-migrate.log 2>&1; then
      cat /tmp/letta-bridge-migrate.log >&2
      exit 1
    fi
  done
  rm -f /tmp/letta-bridge-migrate.log
fi

exec "$@"
