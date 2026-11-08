#!/usr/bin/env bash
# Seed the PRIVATE auth_service database (inside the nested daemon) with the live SCHEMA (no user data) plus the migration ledger rows,
# because auth-core cannot bootstrap an empty database (gdpr_hard_delete.sql runs before the Better Auth tables exist).
# Read-only against the live controlplane-postgres; writes only into the nested daemon.
set -eu
ROOT="C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567"
D="$ROOT/.plan-work/space-proof"
docker exec controlplane-postgres sh -c 'pg_dump -U "$POSTGRES_USER" -a --no-owner -t public.auth_schema_migrations auth_service' > "$D/auth_ledger.data.sql"
nest() { docker exec space-proof-dind "$@"; }
nest docker stop auth-service >/dev/null 2>&1 || true
nest docker exec controlplane-postgres sh -c 'psql -U "$POSTGRES_USER" -d postgres -c "DROP DATABASE IF EXISTS auth_service WITH (FORCE)" -c "CREATE DATABASE auth_service"'
docker exec -i space-proof-dind docker exec -i controlplane-postgres sh -c 'psql -q -U "$POSTGRES_USER" -d auth_service -v ON_ERROR_STOP=0' < "$D/auth_service.schema.sql" > "$D/auth_restore.log" 2>&1
docker exec -i space-proof-dind docker exec -i controlplane-postgres sh -c 'psql -q -U "$POSTGRES_USER" -d auth_service' < "$D/auth_ledger.data.sql" >> "$D/auth_restore.log" 2>&1
nest docker exec controlplane-postgres sh -c 'psql -U "$POSTGRES_USER" -d auth_service -Atc "select count(*) from public.auth_schema_migrations"'
grep -c "ERROR" "$D/auth_restore.log" || true
