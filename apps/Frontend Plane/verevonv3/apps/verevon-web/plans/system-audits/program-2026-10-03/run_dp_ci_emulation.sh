#!/usr/bin/env bash
# Emulates the two data-plane CI jobs that run retrieval-engine tests, on this tree, with CI=true like GitHub Actions:
#  A. rust-check "Unit tests, whole workspace": cargo test --workspace --tests, NO database reachable.
#  B. rust-integration-tests: cargo test -p retrieval-engine-rs --tests -- --nocapture, with TEST_DATABASE_URL pointing at a fresh
#     scratch database (created in review-z12-pg and dropped afterwards), all test binaries sharing it as in CI.
# Credentials are built in memory and masked in output. usage: run_dp_ci_emulation.sh <outdir>
set -uo pipefail
O="$1"; mkdir -p "$O"
export CI=true CARGO_TERM_COLOR=never NO_COLOR=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
cd "C:/dev/CoresSystem/apps/Data Plane v2"
unset TEST_DATABASE_URL TEST_QDRANT_URL REQUIRE_CONTRACT
CARGO_BUILD_JOBS=2 cargo test --locked --workspace --tests --no-fail-fast > "$O/A-rust-check.log" 2>&1
echo "A rust-check emulation exit: $?" > "$O/summary.txt"
C=review-z12-pg
U="$(docker exec "$C" printenv POSTGRES_USER 2>/dev/null)"; P="$(docker exec "$C" printenv POSTGRES_PASSWORD 2>/dev/null)"
PORT="$(docker port "$C" 5432/tcp 2>/dev/null | head -1 | sed 's/.*://')"
if [ -z "$U" ] || [ -z "$P" ] || [ -z "$PORT" ]; then echo "B skipped: container env not readable" >> "$O/summary.txt"; echo ALL-DONE >> "$O/summary.txt"; exit 2; fi
DB="dataplane_test_ci_$$"
docker exec "$C" psql -U "$U" -d postgres -At -c "create database $DB" >/dev/null 2>&1 || { echo "B skipped: could not create db" >> "$O/summary.txt"; echo ALL-DONE >> "$O/summary.txt"; exit 2; }
export TEST_DATABASE_URL="postgres://$U:$P@127.0.0.1:$PORT/$DB?sslmode=disable"
cargo test --locked -p retrieval-engine-rs --tests --no-fail-fast -- --nocapture --test-threads=1 2>&1 | sed -E 's#postgres://[^@ ]*@#postgres://<masked>@#g' > "$O/B-integration.log"
echo "B rust-integration-tests emulation exit: ${PIPESTATUS[0]}" >> "$O/summary.txt"
docker exec "$C" psql -U "$U" -d postgres -At -c "drop database if exists $DB" >/dev/null 2>&1 && echo "test database dropped" >> "$O/summary.txt"
unset U P TEST_DATABASE_URL
echo ALL-DONE >> "$O/summary.txt"
