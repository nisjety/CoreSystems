#!/bin/sh
set -e

if [ -n "$DATABASE_URL" ]; then
  DB_NAME=$(echo "$DATABASE_URL" | sed -n 's|.*/\([^/?]*\).*|\1|p')
  DB_HOST=$(echo "$DATABASE_URL" | sed -n 's|.*@\([^:/?]*\).*|\1|p')
  PG_URL="$DATABASE_URL"

  if [ -z "$DB_NAME" ] || [ -z "$DB_HOST" ]; then
    echo "❌ Failed to parse database settings from DATABASE_URL" >&2
    exit 1
  fi
fi

: ${DB_HOST:=controlplane-postgres}
: ${DB_USER:=aquatiq}

case "$DB_NAME" in
  ''|*[!A-Za-z0-9_-]*)
    echo "❌ Invalid DB_NAME: $DB_NAME" >&2
    exit 1
    ;;
esac

echo "⏳ Waiting for Postgres at ${DB_HOST} (db=${DB_NAME})..."
until pg_isready -h "$DB_HOST" -U "$DB_USER" -q; do sleep 1; done
echo "✅ Postgres ready"

if [ -n "$PG_URL" ]; then
  base="${PG_URL%%\?*}"
  PG_ADMIN_URL="${base%/*}/postgres"
  if ! psql --dbname="$PG_URL" -c '\q' >/dev/null 2>&1; then
    echo "📦 Creating database ${DB_NAME}..."
    psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"${DB_NAME}\";"
  fi
else
  if ! psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q' >/dev/null 2>&1; then
    echo "📦 Creating database ${DB_NAME}..."
    psql -h "$DB_HOST" -U "$DB_USER" -d postgres -c "CREATE DATABASE \"${DB_NAME}\";"
  fi
fi