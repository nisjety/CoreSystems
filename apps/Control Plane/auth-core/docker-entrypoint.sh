#!/bin/sh
set -e

# Parse DB name/host from DATABASE_URL if set
if [ -n "$DATABASE_URL" ]; then
  DB_NAME=$(echo "$DATABASE_URL" | sed -n 's|.*/\([^/?]*\).*|\1|p')
  DB_HOST=$(echo "$DATABASE_URL" | sed -n 's|.*@\([^:/?]*\).*|\1|p')
  PG_URL="$DATABASE_URL"
fi
: ${DB_NAME:=auth_service}
: ${DB_HOST:=controlplane-postgres}
: ${DB_USER:=aquatiq}

echo "Waiting for Postgres at ${DB_HOST} (using DATABASE_URL? ${PG_URL:+yes})..."
# Wait for the Postgres server (not the specific DB) to become available
until pg_isready -h "$DB_HOST" -U "$DB_USER"; do sleep 1; done

# Create database if missing
if [ -n "$PG_URL" ]; then
  # Build admin URL by replacing the DB path with /postgres (preserve user/host/port)
  base="${PG_URL%%\?*}"
  PG_ADMIN_URL="${base%/*}/postgres"
  # If we cannot connect to the target DB, create it via the admin DB
  if ! psql --dbname="$PG_URL" -c '\q' >/dev/null 2>&1; then
    echo "Creating database $DB_NAME via $PG_ADMIN_URL"
    psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"$DB_NAME\";" || true
  fi
else
  if ! psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q' >/dev/null 2>&1; then
    echo "Creating database $DB_NAME"
    psql -h "$DB_HOST" -U "$DB_USER" -d postgres -c "CREATE DATABASE \"$DB_NAME\";" || true
  fi
fi

# If drizzle is available, try to run drizzle-kit push
if command -v drizzle-kit >/dev/null 2>&1; then
  echo "Running drizzle-kit push"
  drizzle-kit push || true
fi

# Apply raw SQL migrations if present
if [ -d "/app/migrations" ]; then
  echo "Applying SQL migrations for $DB_NAME"
  for f in /app/migrations/*.sql /app/migrations/*.up.sql; do
    [ -f "$f" ] || continue
    echo "Applying $f"
    if [ -n "$PG_URL" ]; then
      psql --dbname="$PG_URL" -f "$f"
    else
      psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -f "$f"
    fi
  done
fi

# Start the node application
exec "$@"
