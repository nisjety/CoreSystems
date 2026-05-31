#!/bin/sh
set -eu

if [ -z "${DATABASE_URL:-}" ]; then
  echo "DATABASE_URL is required" >&2
  exit 1
fi

for migration in /app/migrations/*.up.sql; do
  [ -f "$migration" ] || continue
  if ! psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -f "$migration" >/tmp/capability-core-migrate.log 2>&1; then
    cat /tmp/capability-core-migrate.log >&2
    exit 1
  fi
done

rm -f /tmp/capability-core-migrate.log
exec "$@"
