#!/usr/bin/env bash
# =============================================================================
# check-parity-contract.sh  (CR-T11)
#
# The repo-level drift check of the keyword-index parity contract `index-parity-v1`:
#
#   producer  quickwit-adapter-rs   GET /admin/index-parity        (CR-T9a)
#   consumer  data-quality-go       the sparse_index_in_sync release gate
#
# The contract is the golden directory inside the CONSUMER's build context,
# apps/Data Plane v2/services/data-quality-go/internal/gates/testdata/index-parity-v1/
# (README.md, response.*.golden.json and SHA256SUMS). The producer vendors it byte for
# byte under
# apps/Data Plane v2/services/quickwit-adapter-rs/tests/fixtures/contracts/index-parity-v1/
# with the same SHA256SUMS, so that each side tests against the same bytes.
#
# WHAT IT CHECKS
#   1. the consumer's directory matches its own SHA256SUMS (a golden edited without its
#      pin fails);
#   2. once the adapter serves the route (its src/api.rs names `index-parity`), its vendored
#      copy must exist and be byte-identical to the consumer's, file for file, with the same
#      SHA256SUMS. Before that, the adapter side is "pending" (CR-T9a has not built the
#      endpoint, and the gate reports `unavailable`, never a pass); --require-adapter makes
#      pending a failure.
#
# USAGE
#   bash scripts/check-parity-contract.sh                    check
#   bash scripts/check-parity-contract.sh --require-adapter  pending is a failure
#   bash scripts/check-parity-contract.sh --self-test        prove the check can fail
#
# PARITY_ROOT overrides the Data Plane root (the self-test uses it); never set it otherwise.
# =============================================================================
set -euo pipefail

SELF_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${PARITY_ROOT:-$(cd "$SELF_DIR/.." && pwd)}
CONSUMER_DIR="services/data-quality-go/internal/gates/testdata/index-parity-v1"
PRODUCER_DIR="services/quickwit-adapter-rs/tests/fixtures/contracts/index-parity-v1"
PRODUCER_ROUTE_FILE="services/quickwit-adapter-rs/src/api.rs"

fail=0
warn() { printf '%s\n' "$*" >&2; }
bad() { warn "FAIL: $*"; fail=1; }

# Strip CR so a CRLF checkout and an LF one compare equal in the report; the bytes that are
# compared and hashed below are the files' own (the goldens are LF by rule).
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

check() {
  local require_adapter=${1:-0} consumer="$ROOT/$CONSUMER_DIR" producer="$ROOT/$PRODUCER_DIR" file name
  fail=0
  [[ -d $consumer ]] || { bad "the consumer's golden directory is missing: $CONSUMER_DIR"; return 1; }
  pins_hold "$consumer" || true

  if [[ -f $ROOT/$PRODUCER_ROUTE_FILE ]] && grep -q 'index-parity' "$ROOT/$PRODUCER_ROUTE_FILE"; then
    if [[ ! -d $producer ]]; then
      bad "quickwit-adapter-rs serves index-parity but has not vendored the contract: $PRODUCER_DIR is missing"
    else
      pins_hold "$producer" || true
      for file in "$consumer"/*; do
        name=$(basename "$file")
        if [[ ! -f $producer/$name ]]; then bad "the adapter's copy lacks $name"; continue; fi
        cmp -s "$file" "$producer/$name" || bad "the adapter's $name differs from the consumer's: the vendored copy has drifted"
      done
      for file in "$producer"/*; do
        name=$(basename "$file")
        [[ -f $consumer/$name ]] || bad "the adapter's copy has $name, which the contract does not"
      done
    fi
  else
    if [[ $require_adapter == 1 ]]; then
      bad "--require-adapter: quickwit-adapter-rs does not serve index-parity yet (CR-T9a)"
    else
      echo "pending: quickwit-adapter-rs does not serve index-parity yet (CR-T9a); the sparse_index_in_sync gate reports unavailable until it does. Only the consumer's pins were checked."
    fi
  fi
  return "$fail"
}

# ── self-test: a checker that cannot fail reads as coverage ──────────────────
self_test() {
  local tmp rc=0
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  expect() { # expect <ok|fail> <description> <command...>
    local want=$1 what=$2; shift 2
    if "$@" >/dev/null 2>&1; then got=ok; else got=fail; fi
    if [[ $got == "$want" ]]; then echo "  ok: $what"; else echo "  WRONG: $what (wanted $want, got $got)"; rc=1; fi
  }
  fresh() {
    rm -rf "$tmp/root"; mkdir -p "$tmp/root/$CONSUMER_DIR" "$tmp/root/services/quickwit-adapter-rs/src"
    cp "$ROOT/$CONSUMER_DIR"/* "$tmp/root/$CONSUMER_DIR/"
  }
  run() { PARITY_ROOT="$tmp/root" bash "${BASH_SOURCE[0]}" "$@"; }

  fresh
  expect ok "the consumer alone, adapter pending" run
  expect fail "pending is a failure under --require-adapter" run --require-adapter

  fresh; printf '\n' >>"$tmp/root/$CONSUMER_DIR/response.in-sync.golden.json"
  expect fail "a golden edited without its pin" run

  fresh; rm "$tmp/root/$CONSUMER_DIR/response.drift.golden.json"
  expect fail "a pinned file that is missing" run

  fresh; echo '{}' >"$tmp/root/$CONSUMER_DIR/response.extra.golden.json"
  expect fail "a file that is not pinned" run

  fresh; echo 'route("/admin/index-parity")' >"$tmp/root/$PRODUCER_ROUTE_FILE"
  expect fail "the adapter serves the route but has not vendored the contract" run

  fresh; echo 'route("/admin/index-parity")' >"$tmp/root/$PRODUCER_ROUTE_FILE"
  mkdir -p "$tmp/root/$PRODUCER_DIR"; cp "$tmp/root/$CONSUMER_DIR"/* "$tmp/root/$PRODUCER_DIR/"
  expect ok "the adapter vendors the contract byte for byte" run --require-adapter

  fresh; echo 'route("/admin/index-parity")' >"$tmp/root/$PRODUCER_ROUTE_FILE"
  mkdir -p "$tmp/root/$PRODUCER_DIR"; cp "$tmp/root/$CONSUMER_DIR"/* "$tmp/root/$PRODUCER_DIR/"
  sed -i 's/207/208/g' "$tmp/root/$PRODUCER_DIR/response.in-sync.golden.json"
  expect fail "a vendored copy that drifted (its own pin no longer holds)" run

  fresh; echo 'route("/admin/index-parity")' >"$tmp/root/$PRODUCER_ROUTE_FILE"
  mkdir -p "$tmp/root/$PRODUCER_DIR"; cp "$tmp/root/$CONSUMER_DIR"/* "$tmp/root/$PRODUCER_DIR/"
  sed -i 's/207/208/g' "$tmp/root/$PRODUCER_DIR/response.in-sync.golden.json"
  (cd "$tmp/root/$PRODUCER_DIR" && sha256sum README.md response.in-sync.golden.json response.drift.golden.json >SHA256SUMS)
  expect fail "a vendored copy that drifted and was re-pinned" run

  return "$rc"
}

rc=0
case "${1:-}" in
  --self-test) self_test || exit $?; echo "parity contract self-test: ok"; exit 0 ;;
  --require-adapter) check 1 || rc=$? ;;
  "") check 0 || rc=$? ;;
  *) warn "usage: $0 [--require-adapter | --self-test]"; exit 2 ;;
esac
if (( rc == 0 )); then echo "parity contract: ok"; else warn "parity contract: FAILED"; fi
exit "$rc"
