#!/usr/bin/env bash
# Serial verification of the tree after build passes 6 to 8 (no agents running): contract sync, schema tests, Model Plane workspace tests under CI
# conditions, SPA typecheck and tests (the refusal catalogue copy changed), the Quarry workspace tests in Docker. One marker line per step.
set -uo pipefail
S="C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-OneDrive---Aquatiq-Dokumenter-CoresSystem-apps-Frontend-Plane-verevonv3/b44cb88b-b6fc-4fa7-b484-b93c9017d9e9/scratchpad"
O="$S/p8-verify"; rm -rf "$O"; mkdir -p "$O"; : > "$O/summary.txt"
cd C:/dev/CoresSystem
bash scripts/tests/zdr-contract-sync.test.sh > "$O/sync.log" 2>&1; echo "1a. contract sync exit $?: $(tail -n 1 "$O/sync.log" | cut -c1-70)" >> "$O/summary.txt"
bash scripts/tests/zdr-contract-sync.test.sh --self-test > "$O/selftest.log" 2>&1; echo "1b. sync self-test exit $?" >> "$O/summary.txt"
node --test scripts/tests/zdr-contract-schema.test.mjs > "$O/schema.log" 2>&1; echo "1c. schema tests exit $?" >> "$O/summary.txt"
( cd "apps/Model Plane/rust" && CI=true CARGO_TERM_COLOR=never NO_COLOR=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --workspace --no-fail-fast > "$O/modelplane.log" 2>&1 ); echo "2. model plane workspace tests exit $?" >> "$O/summary.txt"
( cd "apps/Frontend Plane/verevonv3" && CI=1 pnpm typecheck > "$O/spa-typecheck.log" 2>&1 ); echo "3a. spa typecheck exit $?" >> "$O/summary.txt"
( cd "apps/Frontend Plane/verevonv3" && CI=1 pnpm test > "$O/spa-tests.log" 2>&1 ); echo "3b. spa tests exit $?" >> "$O/summary.txt"
bash "$S/run_quarry_tests.sh" "$O/quarry.out" > /dev/null 2>&1; echo "4. quarry workspace tests exit $?" >> "$O/summary.txt"
echo ALL-DONE >> "$O/summary.txt"
