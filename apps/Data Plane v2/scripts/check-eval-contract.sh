#!/usr/bin/env bash
# =============================================================================
# check-eval-contract.sh  (CR-T14)
#
# The repo-level drift check of the two contracts between the retrieval-quality harness
# (scripts/eval-gate.py, Python) and the service that reports on it (data-quality-go):
#
#   eval-evidence-v1   producer data-quality-go   GET /v1/quality/context-coverage and GET /v1/quality/gates
#                      consumer eval-gate.py      the coverage and parity gates
#   harness-record-v1  producer eval-gate.py      --records-out
#                      consumer data-quality-go   POST /v1/evals/results (eval.ValidateRecord)
#
# Each contract is a directory of goldens (README.md, *.json and SHA256SUMS) that BOTH sides keep, byte for
# byte, so that each side tests against the same bytes:
#
#   eval-evidence-v1   services/data-quality-go/internal/handler/testdata/eval-evidence-v1
#                      scripts/tests/fixtures/eval-evidence-v1
#   harness-record-v1  services/data-quality-go/internal/eval/testdata/harness-record-v1
#                      scripts/tests/fixtures/harness-record-v1
#
# WHAT IT CHECKS, per contract
#   1. both directories exist;
#   2. each directory matches its own SHA256SUMS: every file is pinned and every pin holds (a golden edited
#      without its pin fails);
#   3. the two directories are the same files with the same bytes (a vendored copy that drifted fails, and so does
#      one that was re-pinned on its own).
#
# USAGE
#   bash scripts/check-eval-contract.sh              check
#   bash scripts/check-eval-contract.sh --self-test  prove the check can fail
#
# EVAL_CONTRACT_ROOT overrides the Data Plane root (the self-test uses it); never set it otherwise.
# =============================================================================
set -euo pipefail

SELF_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${EVAL_CONTRACT_ROOT:-$(cd "$SELF_DIR/.." && pwd)}

# name | one side | the other side
CONTRACTS=(
  "eval-evidence-v1|services/data-quality-go/internal/handler/testdata/eval-evidence-v1|scripts/tests/fixtures/eval-evidence-v1"
  "harness-record-v1|services/data-quality-go/internal/eval/testdata/harness-record-v1|scripts/tests/fixtures/harness-record-v1"
)

fail=0
warn() { printf '%s\n' "$*" >&2; }
bad() { warn "FAIL: $*"; fail=1; }

sha() { sha256sum "$1" | cut -d' ' -f1; }

# pins_hold <dir>: every file in dir except SHA256SUMS is pinned, and every pin holds.
pins_hold() {
  local dir=$1 sums="$1/SHA256SUMS" file name pinned actual ok=0
  [[ -f $sums ]] || { bad "$dir has no SHA256SUMS"; return 1; }
  declare -A pins=()
  while read -r hash name; do
    [[ -n ${hash:-} ]] || continue
    pins[${name#\*}]=$hash
  done < <(tr -d '\r' <"$sums")
  for file in "$dir"/*; do
    name=$(basename "$file")
    [[ $name == SHA256SUMS ]] && continue
    pinned=${pins[$name]:-}
    if [[ -z $pinned ]]; then bad "$dir/$name is not pinned in SHA256SUMS"; ok=1; continue; fi
    actual=$(sha "$file")
    if [[ $actual != "$pinned" ]]; then bad "$dir/$name does not match its pin in SHA256SUMS (edit a golden and its pin together)"; ok=1; fi
    unset "pins[$name]"
  done
  for name in "${!pins[@]}"; do bad "SHA256SUMS in $dir pins $name, which is not in the directory"; ok=1; done
  return "$ok"
}

check_one() {
  local name=$1 left="$ROOT/$2" right="$ROOT/$3" file base
  [[ -d $left ]] || { bad "$name: the directory $2 is missing"; return 0; }
  [[ -d $right ]] || { bad "$name: the directory $3 is missing"; return 0; }
  pins_hold "$left" || true
  pins_hold "$right" || true
  for file in "$left"/*; do
    base=$(basename "$file")
    if [[ ! -f $right/$base ]]; then bad "$name: $3 lacks $base"; continue; fi
    cmp -s "$file" "$right/$base" || bad "$name: $base differs between $2 and $3: the vendored copy has drifted"
  done
  for file in "$right"/*; do
    base=$(basename "$file")
    [[ -f $left/$base ]] || bad "$name: $3 has $base, which $2 does not"
  done
}

check() {
  local entry name left right
  fail=0
  for entry in "${CONTRACTS[@]}"; do
    IFS='|' read -r name left right <<<"$entry"
    check_one "$name" "$left" "$right"
  done
  return "$fail"
}

# ── self-test: a checker that cannot fail reads as coverage ──────────────────
self_test() {
  local tmp rc=0
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  expect() { # expect <ok|fail> <description> <command...>
    local want=$1 what=$2 got; shift 2
    if "$@" >/dev/null 2>&1; then got=ok; else got=fail; fi
    if [[ $got == "$want" ]]; then echo "  ok: $what"; else echo "  WRONG: $what (wanted $want, got $got)"; rc=1; fi
  }
  fresh() {
    local entry name left right
    rm -rf "$tmp/root"
    for entry in "${CONTRACTS[@]}"; do
      IFS='|' read -r name left right <<<"$entry"
      mkdir -p "$tmp/root/$left" "$tmp/root/$right"
      cp "$ROOT/$left"/* "$tmp/root/$left/"
      cp "$ROOT/$right"/* "$tmp/root/$right/"
    done
  }
  run() { EVAL_CONTRACT_ROOT="$tmp/root" bash "${BASH_SOURCE[0]}" "$@"; }
  local ev_a="services/data-quality-go/internal/handler/testdata/eval-evidence-v1"
  local ev_b="scripts/tests/fixtures/eval-evidence-v1"
  local rec_a="services/data-quality-go/internal/eval/testdata/harness-record-v1"
  local rec_b="scripts/tests/fixtures/harness-record-v1"

  fresh
  expect ok "both contracts, both sides, as committed" run

  fresh; printf '\n' >>"$tmp/root/$ev_a/coverage.baseline-off.golden.json"
  expect fail "a golden edited on the producer's side without its pin" run

  fresh; printf '\n' >>"$tmp/root/$ev_b/gates.in-sync.golden.json"
  expect fail "a golden edited on the consumer's side without its pin" run

  fresh; printf '\n' >>"$tmp/root/$rec_a/invalid.verdict.json"
  (cd "$tmp/root/$rec_a" && sha256sum README.md invalid.* record.* | sed 's/  / */' >SHA256SUMS)
  expect fail "a golden edited and re-pinned on one side only (the vendored copy drifted)" run

  fresh; rm "$tmp/root/$rec_b/record.cell.golden.json"
  expect fail "a pinned file that is missing on one side" run

  fresh; echo '{}' >"$tmp/root/$rec_b/record.extra.golden.json"
  expect fail "a file that is not pinned" run

  fresh; rm -rf "$tmp/root/$ev_b"
  expect fail "a missing directory" run

  fresh; sed -i 's/1164/1165/' "$tmp/root/$ev_b/coverage.contextual-ready.golden.json"
  (cd "$tmp/root/$ev_b" && sha256sum README.md coverage.* gates.* | sed 's/  / */' >SHA256SUMS)
  expect fail "a consumer copy that drifted and was re-pinned on its own" run

  return "$rc"
}

rc=0
case "${1:-}" in
  --self-test) self_test || exit $?; echo "eval contract self-test: ok"; exit 0 ;;
  "") check || rc=$? ;;
  *) warn "usage: $0 [--self-test]"; exit 2 ;;
esac
if (( rc == 0 )); then echo "eval contract: ok"; else warn "eval contract: FAILED"; fi
exit "$rc"
