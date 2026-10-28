#!/usr/bin/env bash
# Runs the retrieval-engine database and vector tests that skip without a backend (zdr_behavior and service_scope, including
# QB-M1a's two gated tests) against a fresh throwaway database in the scratch Postgres container (review-z12-pg) and a throwaway
# Qdrant started from the locally cached image on a random localhost port. Both are removed afterwards. Never touches a live
# database or the live ingestion Qdrant. Credentials are built in memory and masked in any output.
set -uo pipefail
C=review-z12-pg
U="$(docker exec "$C" printenv POSTGRES_USER 2>/dev/null)"; P="$(docker exec "$C" printenv POSTGRES_PASSWORD 2>/dev/null)"
PORT="$(docker port "$C" 5432/tcp 2>/dev/null | head -1 | sed 's/.*://')"
[ -n "$U" ] && [ -n "$P" ] && [ -n "$PORT" ] || { echo "container env not readable"; exit 2; }
DB="dataplane_test_verify_$$"
Q="verify-qdrant-$$"
docker exec "$C" psql -U "$U" -d postgres -At -c "create database $DB" >/dev/null 2>&1 || { echo "could not create the test database"; exit 2; }
docker run -d --rm --name "$Q" -p 127.0.0.1::6334 qdrant/qdrant:v1.13.4 >/dev/null 2>&1 || { echo "could not start the throwaway qdrant"; docker exec "$C" psql -U "$U" -d postgres -At -c "drop database if exists $DB" >/dev/null 2>&1; exit 2; }
QPORT=""
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do QPORT="$(docker port "$Q" 6334/tcp 2>/dev/null | head -1 | sed 's/.*://')"; [ -n "$QPORT" ] && break; sleep 2; done
sleep 6
export TEST_DATABASE_URL="postgres://$U:$P@127.0.0.1:$PORT/$DB?sslmode=disable"
export TEST_QDRANT_URL="http://127.0.0.1:$QPORT"
export CARGO_TERM_COLOR=never NO_COLOR=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
cd "C:/dev/CoresSystem/apps/Data Plane v2"
cargo test --locked -p retrieval-engine-rs ${TESTARGS:---test zdr_behavior --test service_scope} -- --include-ignored --test-threads=1 2>&1 \
  | sed -E 's#postgres://[^@ ]*@#postgres://<masked>@#g' | tail -n "${1:-60}" | cut -c1-200
rc=${PIPESTATUS[0]}
docker stop "$Q" >/dev/null 2>&1 && echo "throwaway qdrant stopped"
docker exec "$C" psql -U "$U" -d postgres -At -c "drop database if exists $DB" >/dev/null 2>&1 && echo "test database dropped"
unset U P TEST_DATABASE_URL TEST_QDRANT_URL
exit $rc
