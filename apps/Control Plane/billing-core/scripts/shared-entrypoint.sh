#!/bin/sh
# =============================================================
# shared-entrypoint.sh — Control Plane shared startup logic
# =============================================================
# Usage: source this file, then call: exec "$@"
# Requires: DB_NAME, DB_HOST (defaults below), DATABASE_URL (optional)
# =============================================================
set -e

# Parse DATABASE_URL if provided
if [ -n "$DATABASE_URL" ]; then
  DB_NAME=$(echo "$DATABASE_URL" | sed -n 's|.*/\([^/?]*\).*|\1|p')
  DB_HOST=$(echo "$DATABASE_URL" | sed -n 's|.*@\([^:/?]*\).*|\1|p')
  PG_URL="$DATABASE_URL"
fi

# Apply defaults — caller should set DB_NAME before sourcing
: ${DB_HOST:=controlplane-postgres}
: ${DB_USER:=aquatiq}

echo "⏳ Waiting for Postgres at ${DB_HOST} (db=${DB_NAME})..."
until pg_isready -h "$DB_HOST" -U "$DB_USER" -q; do sleep 1; done
echo "✅ Postgres ready"

# Create the target database if it doesn't exist yet
if [ -n "$PG_URL" ]; then
  base="${PG_URL%%\?*}"
  PG_ADMIN_URL="${base%/*}/postgres"
  if ! psql --dbname="$PG_URL" -c '\q' >/dev/null 2>&1; then
    echo "📦 Creating database ${DB_NAME}..."
    psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"${DB_NAME}\";" || true
  fi
else
  if ! psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q' >/dev/null 2>&1; then
    echo "📦 Creating database ${DB_NAME}..."
    psql -h "$DB_HOST" -U "$DB_USER" -d postgres -c "CREATE DATABASE \"${DB_NAME}\";" || true
  fi
fi

# NOTE: Migrations are intentionally NOT applied here.
# Each service binary runs schema_migrations-tracked migrations at startup.
# Running psql files here would re-apply every migration on every restart.
