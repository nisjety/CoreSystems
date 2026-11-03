#!/usr/bin/env bash
# Drift check for the Space goldens that exist as a Go original and a vendored SPA copy.
#
#   * space-denial-codes (WS03): Go `testdata/space-denial-codes.golden.json` and its .sha256 are
#     byte-identical to the SPA copy, and the pin matches the golden.
#   * room role matrix (WS04-S4/S6): Go `testdata/room-role-matrix.v1.json` is byte-identical to
#     the SPA copy `space-room-role-matrix.v1.json`. The SPA copy must always exist and carry the
#     four vector groups. The Go copy is created by the Control writer's roles test; until it
#     exists the equality check is reported as `skip` (never as a pass), with the reason.
#
# A missing SPA copy is a failure. Goldens are LF files, compared byte for byte.
#
# Usage:
#   scripts/tests/space-contract-vectors-sync.test.sh              run the checks
#   scripts/tests/space-contract-vectors-sync.test.sh --self-test  prove the checks fail on a drifted copy
set -Eeuo pipefail

SELF="${BASH_SOURCE[0]}"
SCRIPT_DIR="$(cd "$(dirname "$SELF")" && pwd)"
# CONTRACT_SYNC_ROOT lets the self-test point the same checks at a scratch tree.
ROOT="${CONTRACT_SYNC_ROOT:-$(cd "$SCRIPT_DIR/../.." && pwd)}"

GO_TESTDATA="apps/Control Plane/user-core/internal/spaces/testdata"
SPA_CONTRACTS="apps/Frontend Plane/verevonv3/src/shared/api/contracts"

passes=0
failures=0
skips=0
pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }
skip() { printf 'skip - %s\n' "$1"; skips=$((skips + 1)); }

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# check_identical <description> <go original> <vendored spa copy> [skip-when-go-missing]
check_identical() {
  local description="$1" go_copy="$ROOT/$2" spa_copy="$ROOT/$3" skip_missing="${4:-}"
  if [[ ! -f "$spa_copy" ]]; then
    fail "$description: the SPA copy is missing ($3)"
    return
  fi
  if [[ ! -f "$go_copy" ]]; then
    if [[ -n "$skip_missing" ]]; then
      skip "$description: the Go copy is not created yet ($2)"
    else
      fail "$description: the Go original is missing ($2)"
    fi
    return
  fi
  if cmp -s "$go_copy" "$spa_copy"; then
    pass "$description"
  else
    fail "$description: $2 and $3 differ"
  fi
}

run_checks() {
  check_identical "space-denial-codes golden is byte-identical in Go and the SPA" \
    "$GO_TESTDATA/space-denial-codes.golden.json" "$SPA_CONTRACTS/space-denial-codes.json"
  check_identical "space-denial-codes pin is byte-identical in Go and the SPA" \
    "$GO_TESTDATA/space-denial-codes.golden.json.sha256" "$SPA_CONTRACTS/space-denial-codes.json.sha256"

  local golden="$ROOT/$GO_TESTDATA/space-denial-codes.golden.json"
  local pin="$ROOT/$GO_TESTDATA/space-denial-codes.golden.json.sha256"
  if [[ -f "$golden" && -f "$pin" ]]; then
    if [[ "$(sha256_of "$golden")" == "$(cut -d' ' -f1 "$pin")" ]]; then
      pass "space-denial-codes golden hashes to its pin"
    else
      fail "space-denial-codes golden does not hash to its pin"
    fi
  else
    fail "space-denial-codes golden or pin is missing"
  fi

  check_identical "room role matrix is byte-identical in Go and the SPA" \
    "$GO_TESTDATA/room-role-matrix.v1.json" "$SPA_CONTRACTS/space-room-role-matrix.v1.json" skip-when-go-missing

  local matrix="$ROOT/$SPA_CONTRACTS/space-room-role-matrix.v1.json"
  if [[ -f "$matrix" ]]; then
    local group missing=""
    for group in '"add"' '"change_role"' '"remove"' '"join"'; do
      grep -q "$group" "$matrix" || missing="$missing $group"
    done
    if [[ -z "$missing" ]]; then
      pass "the SPA role matrix carries add, change_role, remove and join vectors"
    else
      fail "the SPA role matrix lacks vector groups:$missing"
    fi
  fi
}

self_test() {
  scratch="$(mktemp -d)"
  trap 'rm -rf "${scratch:-}"' EXIT
  mkdir -p "$scratch/$GO_TESTDATA" "$scratch/$SPA_CONTRACTS"
  cp "$ROOT/$GO_TESTDATA/space-denial-codes.golden.json" "$ROOT/$GO_TESTDATA/space-denial-codes.golden.json.sha256" "$scratch/$GO_TESTDATA/"
  cp "$ROOT/$SPA_CONTRACTS/space-denial-codes.json" "$scratch/$SPA_CONTRACTS/space-denial-codes.json"
  cp "$ROOT/$SPA_CONTRACTS/space-denial-codes.json.sha256" "$scratch/$SPA_CONTRACTS/space-denial-codes.json.sha256"
  cp "$ROOT/$SPA_CONTRACTS/space-room-role-matrix.v1.json" "$scratch/$SPA_CONTRACTS/"
  cp "$scratch/$SPA_CONTRACTS/space-room-role-matrix.v1.json" "$scratch/$GO_TESTDATA/room-role-matrix.v1.json"
  # Drift the SPA copy of both goldens: both checks must go red.
  printf ' ' >> "$scratch/$SPA_CONTRACTS/space-denial-codes.json"
  printf ' ' >> "$scratch/$SPA_CONTRACTS/space-room-role-matrix.v1.json"
  if CONTRACT_SYNC_ROOT="$scratch" bash "$SELF" >/dev/null 2>&1; then
    printf 'not ok - self-test: a drifted copy was accepted\n' >&2
    exit 1
  fi
  printf 'ok - self-test: a drifted copy is rejected\n'
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit 0
fi

run_checks
printf '%d passed, %d failed, %d skipped\n' "$passes" "$failures" "$skips"
[[ "$failures" -eq 0 ]]
