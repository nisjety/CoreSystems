#!/usr/bin/env bash
# Golden re-pin for the Control PR train (plan P-CFG, docs/plans/space-hybrid-build-plan-2026-10-14.md
# 4.4.2 and 4.5.4 row 19; runbook: apps/Control Plane/config/PRINCIPAL-TRAIN.md).
#
# A train PR that adds a principal, a scope or an ACL row also edits the token-request
# manifest Quarry owns (a row's `granted` flag) whenever the change touches a lane Quarry
# presents. That one edit must be mirrored in four places, and forgetting one is the usual
# way the train goes red:
#
#   1. Quarry's .sha256 pin beside its manifest (the original, the file Quarry edits)
#   2. Auth Core's byte-identical vendored copy of the manifest
#   3. Auth Core's .sha256 pin beside that copy
#   4. the digest row in scripts/tests/zdr-contract-sync.test.sh (GOLDENS)
#
# Pins are the sha256 of the file with CRLF normalised to LF, written as
# "<digest>  <file name>" (the format the Rust and Jest checks already read).
#
#   control-train-repin.sh            check only: prints what is out of step, exit 1 if anything is
#   control-train-repin.sh --apply    re-vendors the copy and rewrites the three pins from the original
#
# The original is the source of truth: this never edits the manifest itself and never reads
# or prints an env file or a credential. Run scripts/tests/principal-registry-parity.test.sh
# and scripts/tests/zdr-contract-sync.test.sh afterwards (the train PR runs both).
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CONTROL_TRAIN_ROOT:-$(cd "$HERE/../../.." && pwd)}"

NAME="cross-plane-token-requests.json"
ORIGINAL="$ROOT/apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/$NAME"
VENDORED="$ROOT/apps/Control Plane/auth-core/src/auth/fixtures/$NAME"
SYNC="$ROOT/scripts/tests/zdr-contract-sync.test.sh"
ORIGINAL_ROW="apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/$NAME"

mode="check"
case "${1:-}" in
  "") ;;
  --check) ;;
  --apply) mode="apply" ;;
  *) printf 'usage: %s [--check|--apply]\n' "$0" >&2; exit 2 ;;
esac

for tool in sha256sum tr cmp sed grep; do
  command -v "$tool" >/dev/null 2>&1 || { printf 'not ok - required tool is missing: %s\n' "$tool" >&2; exit 1; }
done
for file in "$ORIGINAL" "$SYNC"; do
  [[ -f "$file" ]] || { printf 'not ok - required file is missing: %s\n' "$file" >&2; exit 1; }
done

lf_sha() { tr -d '\015' <"$1" | sha256sum | cut -d' ' -f1; }
pin_of() { if [[ -f "$1" ]]; then tr -d '\015' <"$1" | cut -d' ' -f1 | head -n 1; fi; }
sync_row_of() { grep -F "\"$ORIGINAL_ROW|" "$SYNC" | head -n 1 | sed -E 's/.*\|([0-9a-f]{64})".*/\1/'; }

digest="$(lf_sha "$ORIGINAL")"
drift=0
say() { printf '%s\n' "$1"; }
stale() { say "stale - $1"; drift=1; }

[[ "$(pin_of "$ORIGINAL.sha256")" == "$digest" ]] || stale "Quarry pin beside the original ($ORIGINAL.sha256)"
[[ -f "$VENDORED" ]] && cmp -s "$ORIGINAL" "$VENDORED" || stale "Auth Core vendored copy ($VENDORED) is not byte-identical to the original"
[[ "$(pin_of "$VENDORED.sha256")" == "$digest" ]] || stale "Auth Core pin beside the vendored copy ($VENDORED.sha256)"
[[ "$(sync_row_of)" == "$digest" ]] || stale "GOLDENS row in scripts/tests/zdr-contract-sync.test.sh"

if [[ "$mode" == "check" ]]; then
  if [[ "$drift" -eq 0 ]]; then say "ok - $NAME is pinned consistently ($digest)"; exit 0; fi
  say "run with --apply to re-pin from the original"
  exit 1
fi

cp "$ORIGINAL" "$VENDORED"
printf '%s  %s\n' "$digest" "$NAME" >"$ORIGINAL.sha256"
printf '%s  %s\n' "$digest" "$NAME" >"$VENDORED.sha256"
# One row, one digest: replace the 64 hex characters after the pipe on the original's row only.
sed -i -E "\#\"$ORIGINAL_ROW\|[0-9a-f]{64}\"#s#\|[0-9a-f]{64}\"#|$digest\"#" "$SYNC"
say "applied - $NAME re-pinned to $digest (4 places)"
