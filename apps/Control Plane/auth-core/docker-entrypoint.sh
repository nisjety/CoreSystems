#!/bin/sh
set -e

# Docker Compose file-backed secrets retain the source file's ownership and
# mode. The production override starts this entrypoint as root only long enough
# to copy each mounted secret into an app-owned private directory, then drops
# privileges before any database migration or application code runs.
if [ "$(id -u)" -eq 0 ]; then
  set -eu
  secret_directory=/run/control-secrets
  if [ -L "$secret_directory" ] || { [ -e "$secret_directory" ] && [ ! -d "$secret_directory" ]; }; then
    echo "Control secret handoff path is not a regular directory" >&2
    exit 1
  fi
  if [ ! -d "$secret_directory" ]; then
    mkdir -- "$secret_directory"
  fi
  chown root:appgroup /run/control-secrets
  chmod 0710 /run/control-secrets
  for variable in \
    AUTH_GRPC_SERVICE_CREDENTIALS_FILE \
    AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE \
    PLANE_SERVICE_PRINCIPALS_FILE \
    USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE \
    USER_CORE_GRPC_TLS_CA_FILE \
    CONVEX_AUTH_PRIVATE_KEY_FILE \
    CONVEX_AUTH_PUBLIC_KEY_FILE; do
    eval "source=\${$variable:-}"
    case "$source" in
      /run/secrets/*)
        [ -f "$source" ] && [ ! -L "$source" ] || { echo "$variable secret is not a regular file" >&2; exit 1; }
        basename=$(basename "$source")
        target="$secret_directory/$basename"
        staged="$secret_directory/.$basename.new"
        if [ -L "$target" ] || { [ -e "$target" ] && [ ! -f "$target" ]; }; then
          echo "$variable handoff target is not a regular file" >&2
          exit 1
        fi
        rm -f -- "$staged"
        cp -- "$source" "$staged"
        chown appuser:appgroup "$staged"
        chmod 0600 "$staged"
        mv -f -- "$staged" "$target"
        export "$variable=$target"
        ;;
      "")
        ;;
    esac
  done
  export CONTROL_SECRET_HANDOFF_DONE=1
  exec /sbin/su-exec appuser "$0" "$@"
fi

# Parse DB name/host from DATABASE_URL if set.
PG_URL=${DATABASE_URL:-}
if [ -n "$PG_URL" ]; then
  DB_NAME=$(echo "$PG_URL" | sed -n 's|.*/\([^/?]*\).*|\1|p')
  DB_HOST=$(echo "$PG_URL" | sed -n 's|.*@\([^:/?]*\).*|\1|p')
fi
: ${DB_NAME:=auth_service}
: ${DB_HOST:=controlplane-postgres}
: ${DB_USER:=coresystem}
: ${POSTGRES_READY_MAX_ATTEMPTS:=90}

case "$POSTGRES_READY_MAX_ATTEMPTS" in
  ''|*[!0-9]*)
    echo "POSTGRES_READY_MAX_ATTEMPTS must be a positive integer" >&2
    exit 1
    ;;
esac
if [ "$POSTGRES_READY_MAX_ATTEMPTS" -lt 1 ]; then
  echo "POSTGRES_READY_MAX_ATTEMPTS must be a positive integer" >&2
  exit 1
fi
case "$DB_NAME" in
  *[!A-Za-z0-9_-]*)
    echo "Configured database name contains unsupported characters" >&2
    exit 1
    ;;
esac

echo "Waiting for Postgres at ${DB_HOST} (using DATABASE_URL? ${PG_URL:+yes})..."
# Wait for the Postgres server (not the specific DB) to become available
postgres_ready_attempt=1
until pg_isready -q -h "$DB_HOST" -U "$DB_USER"; do
  if [ "$postgres_ready_attempt" -ge "$POSTGRES_READY_MAX_ATTEMPTS" ]; then
    echo "Postgres readiness timed out after ${POSTGRES_READY_MAX_ATTEMPTS} attempts" >&2
    exit 1
  fi
  postgres_ready_attempt=$((postgres_ready_attempt + 1))
  sleep 1
done

# Create database if missing
if [ -n "$PG_URL" ]; then
  # Build admin URL by replacing the DB path with /postgres (preserve user/host/port)
  base="${PG_URL%%\?*}"
  PG_ADMIN_URL="${base%/*}/postgres"
  # If we cannot connect to the target DB, create it via the admin DB
  if ! psql --dbname="$PG_URL" -c '\q' >/dev/null 2>&1; then
    echo "Creating database $DB_NAME via configured Postgres admin connection"
    psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"$DB_NAME\";" >/dev/null 2>&1 || true
    if ! psql --dbname="$PG_URL" -c '\q' >/dev/null 2>&1; then
      echo "Database creation did not make the configured target available" >&2
      exit 1
    fi
  fi
else
  if ! psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q' >/dev/null 2>&1; then
    echo "Creating database $DB_NAME"
    psql -h "$DB_HOST" -U "$DB_USER" -d postgres \
      -c "CREATE DATABASE \"$DB_NAME\";" >/dev/null 2>&1 || true
    if ! psql -h "$DB_HOST" -U "$DB_USER" -d "$DB_NAME" -c '\q' >/dev/null 2>&1; then
      echo "Database creation did not make the configured target available" >&2
      exit 1
    fi
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

# Report-and-stop on historical authority gaps. The preflight is read-only and
# never selects owners or recreates access without an operator-reviewed Auth
# action.
/app/validate-lifecycle-preflight.sh

# Start the node application
exec "$@"
