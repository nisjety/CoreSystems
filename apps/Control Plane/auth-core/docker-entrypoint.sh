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

# Drizzle push is an explicit local-development escape hatch, never an implicit
# production schema mutator. Versioned SQL below is the deployment authority.
if [ "${RUN_DRIZZLE_PUSH:-0}" = "1" ] && command -v drizzle-kit >/dev/null 2>&1; then
  echo "Running drizzle-kit push"
  drizzle-kit push
fi

run_psql() {
  if [ -n "$PG_URL" ]; then
    psql --dbname="$PG_URL" "$@"
  else
    psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" "$@"
  fi
}

apply_migration() {
  migration_file="$1"
  migration_version=$(basename "$migration_file")
  migration_checksum=$(sha256sum "$migration_file" | awk '{print $1}')
  echo "Applying migration $migration_version"

  # The advisory lock, migration body, and ledger insert share one database
  # session and transaction. Concurrent replicas serialize here; any SQL error
  # aborts the transaction and ON_ERROR_STOP prevents the application starting.
  {
    echo 'BEGIN;'
    echo "SELECT pg_advisory_xact_lock(hashtext('auth-core-schema-migrations'));"
    echo 'CREATE TABLE IF NOT EXISTS public.auth_schema_migrations ('
    echo '  version TEXT PRIMARY KEY,'
    echo '  checksum TEXT NOT NULL,'
    echo '  applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()'
    echo ');'
    echo "SELECT EXISTS ("
    echo "  SELECT 1 FROM public.auth_schema_migrations"
    echo "  WHERE version = :'migration_version' AND checksum <> :'migration_checksum'"
    echo ") AS checksum_mismatch \gset"
    echo '\if :checksum_mismatch'
    echo '\echo Migration checksum mismatch for :migration_version'
    echo '\quit 3'
    echo '\endif'
    echo "SELECT NOT EXISTS ("
    echo "  SELECT 1 FROM public.auth_schema_migrations WHERE version = :'migration_version'"
    echo ") AS should_apply \\gset"
    echo '\if :should_apply'
    echo '\i :migration_file'
    echo "INSERT INTO public.auth_schema_migrations (version, checksum)"
    echo "VALUES (:'migration_version', :'migration_checksum');"
    echo '\endif'
    echo 'COMMIT;'
  } | run_psql -X -v ON_ERROR_STOP=1 \
    -v migration_version="$migration_version" \
    -v migration_checksum="$migration_checksum" \
    -v migration_file="$migration_file"
}

# Bootstrap tables must exist before additive numbered migrations reference
# Better Auth's user/account/organization/member tables. The explicit order is
# intentional and covered by deployment-contract tests.
if [ -d "/app/migrations" ]; then
  echo "Applying SQL migrations for $DB_NAME"
  for f in \
    /app/migrations/init_better_auth.sql \
    /app/migrations/gdpr_hard_delete.sql \
    /app/migrations/[0-9][0-9][0-9]_*.sql; do
    [ -f "$f" ] || continue
    apply_migration "$f"
  done
fi

# Start the node application
exec "$@"
