#!/usr/bin/env bash
# Failing-set ratchet for suites with known pre-existing failures (slice M0-CI,
# plan 4.5.5 and 6.6): a NEW failing test id fails the gate, an old one does not.
#
#   bash scripts/gates/space-baseline-compare.sh <id>[,<id>...]    run the suites and compare
#   bash scripts/gates/space-baseline-compare.sh --self-test       prove a new failing id goes red
#
# <id> is a command id from `node scripts/tests/baseline-capture.mjs --list`
# (auth-core-jest, spa-vitest, spa-typecheck, spa-lint, spa-node-tests,
# convex-test, convex-typecheck, convex-lint, ...). Dependencies must already be
# installed: this script runs the project's own commands and nothing else.
#
# The stored baseline is the newest docs/plans/evidence/baseline-<date>/ that
# holds a record for the id (written by baseline-capture.mjs on the BASE commit,
# slice M0-T0). When there is none for an id:
#   * the run has ZERO failing ids  -> pass (nothing can have regressed);
#   * the run has failing ids       -> FAIL with a message that says why, because
#     "everything that fails is pre-existing" cannot be proven without a baseline.
# Exit codes are never trusted on their own: baseline-capture records a non-zero
# exit with no parsed id as `<id>::exit-<code>`, so it shows up in the diff.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CAPTURE="$ROOT/scripts/tests/baseline-capture.mjs"
DIFF="$ROOT/scripts/tests/baseline-diff.mjs"
EVIDENCE="$ROOT/docs/plans/evidence"

# A command id's stored record: newest baseline-* dir that has <id>.json.
find_record() {
  local id="$1" d
  for d in $(ls -d "$EVIDENCE"/baseline-* 2>/dev/null | sort -r); do
    [ -f "$d/$id.json" ] && { printf '%s\n' "$d/$id.json"; return 0; }
  done
  return 1
}

self_test() {
  local tmp rc=0; tmp="$(mktemp -d)"
  mkdir -p "$tmp/base" "$tmp/same" "$tmp/worse"
  printf '{"id":"demo","failing_ids":["a.test > old failure"]}\n' > "$tmp/base/demo.json"
  printf '{"id":"demo","failing_ids":["a.test > old failure"]}\n' > "$tmp/same/demo.json"
  printf '{"id":"demo","failing_ids":["a.test > old failure","b.test > new failure"]}\n' > "$tmp/worse/demo.json"
  node "$DIFF" "$tmp/base" "$tmp/same" > /dev/null || { echo "FAIL: an unchanged failing set was reported as a regression"; rc=1; }
  if node "$DIFF" "$tmp/base" "$tmp/worse" > /dev/null; then
    echo "FAIL: a new failing id passed; the ratchet is not checking anything"; rc=1
  fi
  rm -rf "$tmp"
  [ "$rc" -eq 0 ] && echo "self-test passed: unchanged set green, new failing id red"
  return "$rc"
}

main() {
  case "${1:-}" in
    --self-test) self_test; return ;;
    '') echo "usage: space-baseline-compare.sh <id>[,<id>...] | --self-test" >&2; return 2 ;;
  esac
  local ids="$1" rc=0 id
  current="$(mktemp -d)"; base_dir="$(mktemp -d)"
  trap 'rm -rf "$current" "$base_dir"' EXIT

  # Capture exits non-zero never; it records failures as ids.
  node "$CAPTURE" --root "$ROOT" --only "$ids" --out "$current"

  IFS=',' read -r -a want <<< "$ids"
  for id in "${want[@]}"; do
    [ -f "$current/$id.json" ] || { echo "::error::$id produced no record (directory missing or the command is not defined)"; rc=1; continue; }
    if rec="$(find_record "$id")"; then
      cp "$rec" "$base_dir/$id.json"
    else
      count="$(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).failing_ids.length)' "$current/$id.json")"
      if [ "$count" -eq 0 ]; then
        echo "$id: no stored baseline and no failing ids: pass"
        printf '{"id":"%s","failing_ids":[]}\n' "$id" > "$base_dir/$id.json"
      else
        echo "::error::$id has $count failing id(s) and no stored baseline under docs/plans/evidence/baseline-*/. Capture one on the base commit: node scripts/tests/baseline-capture.mjs --only $id (slice M0-T0)."
        rc=1
      fi
    fi
  done
  [ "$rc" -eq 0 ] || return "$rc"
  node "$DIFF" "$base_dir" "$current"
}

main "$@"
