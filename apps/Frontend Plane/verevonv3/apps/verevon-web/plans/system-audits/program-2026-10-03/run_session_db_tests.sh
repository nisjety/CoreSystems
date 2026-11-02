#!/usr/bin/env bash
# Runs session-core's ignored zdr_receipt database tests (Z-17 and Z-21a, including migrations 0044 and 0045) against a fresh
# throwaway database inside the scratch Postgres container (review-z12-pg, localhost only); the database is dropped afterwards.
# The connection string is built in memory from the container's own environment and is never printed or written anywhere;
# any credential in tool output is masked.
set -uo pipefail
C=review-z12-pg
U="$(docker exec "$C" printenv POSTGRES_USER 2>/dev/null)"; P="$(docker exec "$C" printenv POSTGRES_PASSWORD 2>/dev/null)"
PORT="$(docker port "$C" 5432/tcp 2>/dev/null | head -1 | sed 's/.*://')"
[ -n "$U" ] && [ -n "$P" ] && [ -n "$PORT" ] || { echo "container env not readable"; exit 2; }
DB="session_core_test_verify_$$"
docker exec "$C" psql -U "$U" -d postgres -At -c "create database $DB" >/dev/null 2>&1 || { echo "could not create the test database"; exit 2; }
export DATABASE_URL="postgres://$U:$P@127.0.0.1:$PORT/$DB?sslmode=disable"
export CARGO_TERM_COLOR=never NO_COLOR=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
cd "C:/dev/CoresSystem/apps/Model Plane/rust"
cargo test --locked -p session-core --bin session-core -- --ignored --test-threads=1 ${FILTERS:-zdr_receipt} 2>&1 \
  | sed -E 's#postgres://[^@ ]*@#postgres://<masked>@#g' | tail -n "${1:-40}" | cut -c1-200
rc=${PIPESTATUS[0]}
echo "== schema facts after the run (0044 and 0045):"
docker exec "$C" psql -U "$U" -d "$DB" -At -c "select max(version) from _sqlx_migrations" 2>&1 | sed 's/^/  max migration version: /'
docker exec "$C" psql -U "$U" -d "$DB" -At -c "select indexname, indisvalid from pg_indexes i join pg_class c on c.relname=i.indexname join pg_index x on x.indexrelid=c.oid where i.tablename='runs' and i.indexname ilike '%waived%'" 2>&1 | sed 's/^/  waived index: /'
docker exec "$C" psql -U "$U" -d postgres -At -c "drop database if exists $DB" >/dev/null 2>&1 && echo "test database dropped"
unset U P DATABASE_URL
exit $rc
