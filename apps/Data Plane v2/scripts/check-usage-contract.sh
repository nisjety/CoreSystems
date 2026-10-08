#!/usr/bin/env bash
# CR-T10 — the usage ledger's cross-tree contracts must not drift.
#
# Two files are the contract between the Data Plane's usage ledger and what it talks to. Each
# is generated in one place, copied byte for byte into the tree that reads it, and pinned by a
# sha256 beside it on BOTH sides (the sha256 of the file with CRLF normalised to LF, so a
# checkout that rewrites line endings does not move it):
#
#   usage-envelopes.v2.json
#       Real signed envelopes built by usage-ledger-rs and read by the REAL Go verifier and
#       consumer of data-orchestrator-go.
#         generated: services/usage-ledger-rs/tests/fixtures/
#         read:      services/data-orchestrator-go/testdata/
#         regenerate: UPDATE_USAGE_FIXTURE=1 cargo test -p usage-ledger-rs --test envelope_fixture
#
#   forwarder-record-request.golden.json
#       The exact body the forwarder posts to cost-core's POST /api/v1/cost/record. cost-core
#       decodes it strictly; its tests post every body through its real handler (task CR-T4).
#         generated: services/data-orchestrator-go/testdata/
#         read:      apps/Model Plane/go/services/cost-core (a vendored copy, CR-T4)
#         regenerate: UPDATE_FORWARDER_GOLDEN=1 go test -run TestForwarderRequestMatchesItsGoldenAndPin ./internal/cost/
#
# Never edit a copy: regenerate it at its source, copy the file and its pin, run this script.
#
# Usage: bash scripts/check-usage-contract.sh
# Exit status 0 when every present copy matches. The cost-core copy does not exist until CR-T4
# vendors it: until then this says so and passes, and FAILS when REQUIRE_CONTRACT or CI is set
# (a contract that cannot be checked must not read as one that passed).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLANE_ROOT="$(cd "$ROOT/.." && pwd)"
failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# pin_of FILE: sha256 of the file with CRLF normalised to LF.
pin_of() {
    tr -d '\r' < "$1" | sha256sum | cut -d' ' -f1
}

# check_pinned FILE: the file exists, has a pin, and the pin is right.
check_pinned() {
    local file="$1"
    if [[ ! -f "$file" ]]; then
        fail "$file is missing"
        return 1
    fi
    if [[ ! -f "$file.sha256" ]]; then
        fail "$file has no pin ($file.sha256)"
        return 1
    fi
    local want got
    want="$(tr -d '[:space:]' < "$file.sha256")"
    got="$(pin_of "$file")"
    if [[ "$want" != "$got" ]]; then
        fail "$file was edited without regenerating its pin (sha256 $got, pinned $want)"
        return 1
    fi
}

# same_bytes A B: the two files are identical once CRLF is normalised.
same_bytes() {
    if ! diff -q <(tr -d '\r' < "$1") <(tr -d '\r' < "$2") > /dev/null; then
        fail "$2 differs from its source $1: regenerate at the source and copy the file and its pin"
        return 1
    fi
}

# 1. The envelope fixture: usage-ledger-rs generates it, data-orchestrator-go reads a copy.
rust_fixture="$ROOT/services/usage-ledger-rs/tests/fixtures/usage-envelopes.v2.json"
go_fixture="$ROOT/services/data-orchestrator-go/testdata/usage-envelopes.v2.json"
if check_pinned "$rust_fixture" && check_pinned "$go_fixture"; then
    same_bytes "$rust_fixture" "$go_fixture" || true
    same_bytes "$rust_fixture.sha256" "$go_fixture.sha256" || true
fi

# 2. The forwarder golden: data-orchestrator-go generates it; cost-core vendors a copy (CR-T4).
golden="$ROOT/services/data-orchestrator-go/testdata/forwarder-record-request.golden.json"
check_pinned "$golden" || true
cost_core_root="$PLANE_ROOT/Model Plane/go/services/cost-core"
vendored=""
if [[ -d "$cost_core_root" ]]; then
    vendored="$(find "$cost_core_root" -name 'forwarder-record-request.golden.json' -not -path '*/node_modules/*' 2>/dev/null | head -n 1 || true)"
fi
if [[ -n "$vendored" ]]; then
    if check_pinned "$vendored"; then
        same_bytes "$golden" "$vendored" || true
        same_bytes "$golden.sha256" "$vendored.sha256" || true
    fi
elif [[ -n "${REQUIRE_CONTRACT:-}" || -n "${CI:-}" ]]; then
    fail "cost-core has not vendored forwarder-record-request.golden.json (task CR-T4): the contract with cost-core cannot be checked"
else
    echo "note: cost-core has not vendored forwarder-record-request.golden.json yet (task CR-T4); not checked" >&2
fi

if [[ "$failures" -gt 0 ]]; then
    echo "usage contract check FAILED ($failures problem(s))" >&2
    exit 1
fi
echo "usage contract check passed"
