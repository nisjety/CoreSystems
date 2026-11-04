#!/usr/bin/env bash
# Tests for control-train-repin.sh (plan P-CFG). Runs the real script against a
# scratch tree (CONTROL_TRAIN_ROOT), never the repository, and never reads an env
# file or a credential.
#
#   apps/Control Plane/scripts/control-train-repin-test.sh
set -Eeuo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/control-train-repin.sh"
NAME="cross-plane-token-requests.json"
ORIGINAL_REL="apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/$NAME"
VENDORED_REL="apps/Control Plane/auth-core/src/auth/fixtures/$NAME"
SYNC_REL="scripts/tests/zdr-contract-sync.test.sh"

failures=0
checks=0
ok() { checks=$((checks + 1)); printf 'ok - %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf 'not ok - %s\n' "$1" >&2; }
expect() { # description, then a command that must succeed
  local description="$1"; shift
  if "$@"; then ok "$description"; else fail "$description"; fi
}

SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/control-train-repin.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

lf_sha() { tr -d '\015' <"$1" | sha256sum | cut -d' ' -f1; }

# A consistent scratch tree: original manifest, vendored copy, both pins, and a
# zdr-contract-sync GOLDENS array carrying the original's row plus two others
# that must never be touched.
OTHER_ROW_A='  "apps/Model Plane/api/contracts/infer-statuses.json|c5223c4671db759a6c335c7438d506992e3a7664008ddd359e02ef52e56d5975"'
OTHER_ROW_B='  "apps/Control Plane/auth-core/src/auth/fixtures/inference-service-tokens.golden.json|afa8844a55345f3c2222cfb45cb6c3a73acd026e600a5941205411a1cfc3411b"'

make_tree() { # $1 = root
  local root="$1" digest
  mkdir -p "$root/$(dirname "$ORIGINAL_REL")" "$root/$(dirname "$VENDORED_REL")" "$root/$(dirname "$SYNC_REL")"
  printf '{\n  "rows": [{"principal": "quarry", "granted": true}]\n}\n' >"$root/$ORIGINAL_REL"
  cp "$root/$ORIGINAL_REL" "$root/$VENDORED_REL"
  digest="$(lf_sha "$root/$ORIGINAL_REL")"
  printf '%s  %s\n' "$digest" "$NAME" >"$root/$ORIGINAL_REL.sha256"
  printf '%s  %s\n' "$digest" "$NAME" >"$root/$VENDORED_REL.sha256"
  {
    printf 'GOLDENS=(\n'
    printf '%s\n' "$OTHER_ROW_A"
    printf '  "%s|%s"\n' "$ORIGINAL_REL" "$digest"
    printf '%s\n' "$OTHER_ROW_B"
    printf ')\n'
  } >"$root/$SYNC_REL"
}

run() { # $1 = root, then script args; sets RC and OUT
  set +e
  OUT="$(CONTROL_TRAIN_ROOT="$1" bash "$SCRIPT" "${@:2}" 2>&1)"
  RC=$?
  set -e
}

# 1. A consistent tree passes the check, in both spellings of check mode.
T1="$SCRATCH/consistent"; make_tree "$T1"
run "$T1"
expect "check mode exits 0 on a consistently pinned tree" test "$RC" -eq 0
expect "check mode says it is pinned consistently" grep -q "is pinned consistently" <<<"$OUT"
run "$T1" --check
expect "--check is the same as no argument" test "$RC" -eq 0

# 2. Quarry edits its manifest (a row's granted flag): every mirror goes stale.
T2="$SCRATCH/edited"; make_tree "$T2"
sed -i 's/"granted": true/"granted": false/' "$T2/$ORIGINAL_REL"
OLD_DIGEST="$(lf_sha "$T1/$ORIGINAL_REL")"
NEW_DIGEST="$(lf_sha "$T2/$ORIGINAL_REL")"
expect "the edit changed the manifest digest" test "$OLD_DIGEST" != "$NEW_DIGEST"
run "$T2"
expect "check mode exits 1 when the manifest moved" test "$RC" -eq 1
expect "check names Quarry's pin" grep -q "stale - Quarry pin" <<<"$OUT"
expect "check names the Auth Core vendored copy" grep -q "stale - Auth Core vendored copy" <<<"$OUT"
expect "check names the Auth Core pin" grep -q "stale - Auth Core pin" <<<"$OUT"
expect "check names the GOLDENS row" grep -q "stale - GOLDENS row" <<<"$OUT"
expect "check says how to fix it" grep -q "run with --apply" <<<"$OUT"
expect "check mode wrote nothing: the vendored copy is still the old one" \
  bash -c '! cmp -s "$1" "$2"' _ "$T2/$ORIGINAL_REL" "$T2/$VENDORED_REL"
expect "check mode wrote nothing: the old GOLDENS digest is still there" grep -q "$OLD_DIGEST" "$T2/$SYNC_REL"

# 3. --apply re-vendors the copy and rewrites the pin, the copy's pin and the GOLDENS row.
ORIGINAL_BEFORE="$(sha256sum <"$T2/$ORIGINAL_REL")"
run "$T2" --apply
expect "--apply exits 0" test "$RC" -eq 0
expect "--apply reports the new digest in four places" grep -q "re-pinned to $NEW_DIGEST (4 places)" <<<"$OUT"
expect "--apply never edits the original manifest" test "$(sha256sum <"$T2/$ORIGINAL_REL")" = "$ORIGINAL_BEFORE"
expect "the vendored copy is byte-identical to the original" cmp -s "$T2/$ORIGINAL_REL" "$T2/$VENDORED_REL"
expect "Quarry's pin carries the new digest in the 'digest  name' format" \
  test "$(cat "$T2/$ORIGINAL_REL.sha256")" = "$NEW_DIGEST  $NAME"
expect "Auth Core's pin carries the new digest in the same format" \
  test "$(cat "$T2/$VENDORED_REL.sha256")" = "$NEW_DIGEST  $NAME"
expect "the GOLDENS row carries the new digest" grep -qF "\"$ORIGINAL_REL|$NEW_DIGEST\"" "$T2/$SYNC_REL"
expect "the old digest is gone from the GOLDENS array" bash -c '! grep -q "$1" "$2"' _ "$OLD_DIGEST" "$T2/$SYNC_REL"
expect "only the original's row changed: the first other row is intact" grep -qxF "$OTHER_ROW_A" "$T2/$SYNC_REL"
expect "only the original's row changed: the second other row is intact" grep -qxF "$OTHER_ROW_B" "$T2/$SYNC_REL"
expect "the GOLDENS array still has exactly three rows" test "$(grep -c '|[0-9a-f]\{64\}"' "$T2/$SYNC_REL")" -eq 3
run "$T2"
expect "check mode exits 0 after --apply" test "$RC" -eq 0
run "$T2" --apply
expect "a second --apply is idempotent" test "$RC" -eq 0
run "$T2"
expect "and the tree is still consistent" test "$RC" -eq 0

# 4. The digest is over the LF-normalised bytes, so a CRLF checkout of the same
#    manifest pins the same digest as its LF twin.
T3="$SCRATCH/crlf"; make_tree "$T3"
sed -i 's/$/\r/' "$T3/$ORIGINAL_REL"
run "$T3"
expect "a CRLF original is not byte-identical to an LF vendored copy" \
  bash -c 'grep -q "stale - Auth Core vendored copy" <<<"$1"' _ "$OUT"
run "$T3" --apply
expect "--apply on a CRLF original exits 0" test "$RC" -eq 0
expect "the pin is the LF-normalised digest" test "$(cut -d' ' -f1 "$T3/$ORIGINAL_REL.sha256")" = "$OLD_DIGEST"
expect "the GOLDENS row is the LF-normalised digest" grep -qF "\"$ORIGINAL_REL|$OLD_DIGEST\"" "$T3/$SYNC_REL"

# 5. A pin written with CRLF line endings is read correctly (autocrlf checkouts).
T4="$SCRATCH/crlf-pin"; make_tree "$T4"
sed -i 's/$/\r/' "$T4/$ORIGINAL_REL.sha256" "$T4/$VENDORED_REL.sha256"
run "$T4"
expect "CRLF pins are read as LF" test "$RC" -eq 0

# 6. A missing vendored copy is stale, and --apply creates it.
T5="$SCRATCH/no-copy"; make_tree "$T5"
rm "$T5/$VENDORED_REL" "$T5/$VENDORED_REL.sha256"
run "$T5"
expect "a missing vendored copy is reported stale" bash -c 'test "$1" -eq 1 && grep -q "stale - Auth Core vendored copy" <<<"$2"' _ "$RC" "$OUT"
run "$T5" --apply
expect "--apply recreates the vendored copy and its pin" bash -c 'cmp -s "$1" "$2" && test -f "$3"' _ "$T5/$ORIGINAL_REL" "$T5/$VENDORED_REL" "$T5/$VENDORED_REL.sha256"

# 7. Missing inputs and bad usage fail loudly.
T6="$SCRATCH/no-original"; make_tree "$T6"; rm "$T6/$ORIGINAL_REL"
run "$T6"
expect "a missing original is an error, not a pass" bash -c 'test "$1" -eq 1 && grep -q "required file is missing" <<<"$2"' _ "$RC" "$OUT"
T7="$SCRATCH/no-sync"; make_tree "$T7"; rm "$T7/$SYNC_REL"
run "$T7"
expect "a missing zdr-contract-sync script is an error" test "$RC" -eq 1
run "$T1" --nonsense
expect "an unknown argument exits 2 with usage" bash -c 'test "$1" -eq 2 && grep -q "usage:" <<<"$2"' _ "$RC" "$OUT"

# 8. The script stays inside its remit: it names no env file and no credential.
expect "the script never touches an env file" bash -c '! grep -Eq "\.env|TOKEN|SECRET|PASSWORD" "$1"' _ "$SCRIPT"

printf '%d checks, %d failed\n' "$checks" "$failures"
[[ "$failures" -eq 0 ]]
