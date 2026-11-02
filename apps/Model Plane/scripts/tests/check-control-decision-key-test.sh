#!/usr/bin/env bash
set -euo pipefail

# Behaviour test for scripts/check-control-decision-key.sh using throwaway env
# files with generated keys. It touches no real ledger and no Docker, and fails if
# the script ever prints a key value.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT_DIR/scripts/check-control-decision-key.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# 32 deterministic bytes as unpadded base64url and as standard base64.
make_key() { # make_key SEED -> prints "<url> <std>"
  local seed="$1" std url
  std="$(printf '%032d' "$seed" | base64 | tr -d '\n')"
  url="$(printf '%s' "$std" | tr '+/' '-_' | tr -d '=')"
  printf '%s %s\n' "$url" "$std"
}
read -r KEY_A_URL KEY_A_STD < <(make_key 1)
read -r KEY_B_URL _ < <(make_key 2)

# A 32-byte key whose standard base64 contains + and /, to prove URL-safe decoding.
special_std="$({
  for _ in 1 2 3 4 5 6 7 8 9 10; do printf '\xfb\xff\xbe'; done
  printf 'ab'
} | base64 | tr -d '\n')"
[[ "$special_std" == *[+/]* ]] || fail "could not build a key with + or / in standard base64"
special_url="$(printf '%s' "$special_std" | tr '+/' '-_' | tr -d '=')"

write_env() { # write_env FILE ID KEY
  printf 'UNRELATED=1\nCONTROL_SPACE_DECISION_KEY_ID=%s\nCONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64=%s\n' "$2" "$3" >"$1"
}

run() { # run ARGS... -> sets OUT and STATUS; stdout+stderr merged
  set +e
  OUT="$(env -u CONTROL_SPACE_DECISION_KEY_ID -u CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 -u CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON "$@" 2>&1)"
  STATUS=$?
  set -e
}

assert_no_key_printed() {
  local secret
  for secret in "$KEY_A_URL" "$KEY_B_URL" "$special_url" "$special_std" "$KEY_A_STD"; do
    [[ "$OUT" != *"$secret"* ]] || fail "a key value was printed"
  done
}

ledger="$work/ledger.env"
write_env "$ledger" "control-space-decision-aaa" "$KEY_A_URL"

# 1. Match: ledger only (the chain resolves to the ledger value).
cp "$ledger" "$work/model-same.env"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-same.env"
[[ "$STATUS" == 0 ]] || fail "matching chain exited $STATUS: $OUT"
[[ "$OUT" == *"OK:"* ]] || fail "matching chain did not report OK"
assert_no_key_printed

# 2. Match when the Model side carries the same key as standard base64.
write_env "$work/model-std.env" "control-space-decision-aaa" "$KEY_A_STD"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-std.env"
[[ "$STATUS" == 0 ]] || fail "standard-base64 form of the same key exited $STATUS: $OUT"

# 3. Key id mismatch.
write_env "$work/model-id.env" "control-space-decision-stale" "$KEY_A_URL"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-id.env"
[[ "$STATUS" == 1 ]] || fail "key id mismatch exited $STATUS"
[[ "$OUT" == *"key id mismatch"* && "$OUT" == *"control-space-decision-stale"* && "$OUT" == *"control-space-decision-aaa"* ]] ||
  fail "key id mismatch did not name both ids: $OUT"
assert_no_key_printed

# 4. Public key mismatch, same id.
write_env "$work/model-key.env" "control-space-decision-aaa" "$KEY_B_URL"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-key.env"
[[ "$STATUS" == 1 ]] || fail "public key mismatch exited $STATUS"
[[ "$OUT" == *"public key mismatch"* && "$OUT" == *"fingerprint"* ]] || fail "public key mismatch not reported: $OUT"
assert_no_key_printed

# 5. Later file wins: a stale deploy env after the ledger overrides it.
run bash "$SCRIPT" --ledger "$ledger" --model-env "$ledger" --model-env "$work/model-id.env"
[[ "$STATUS" == 1 ]] || fail "a stale later file did not win (exit $STATUS)"
# ... and a correct later file repairs an earlier stale one.
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-id.env" --model-env "$ledger"
[[ "$STATUS" == 0 ]] || fail "a correct later file did not win (exit $STATUS): $OUT"

# 6. The process environment overrides the files, as Compose does.
run env CONTROL_SPACE_DECISION_KEY_ID=control-space-decision-env bash "$SCRIPT" --ledger "$ledger" --model-env "$ledger"
[[ "$STATUS" == 1 && "$OUT" == *"process environment"* ]] || fail "process environment override not caught (exit $STATUS): $OUT"

# 7. Unparseable Model key.
write_env "$work/model-bad.env" "control-space-decision-aaa" "not*a*key"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-bad.env"
[[ "$STATUS" == 1 && "$OUT" == *"UNPARSEABLE"* ]] || fail "unparseable key not flagged (exit $STATUS): $OUT"
write_env "$work/model-short.env" "control-space-decision-aaa" "c2hvcnQ"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-short.env"
[[ "$STATUS" == 1 && "$OUT" == *"UNPARSEABLE"* ]] || fail "short key not flagged (exit $STATUS): $OUT"

# 8. Model chain that never defines the pair.
printf 'UNRELATED=1\n' >"$work/model-empty.env"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-empty.env"
[[ "$STATUS" == 1 ]] || fail "an unset Model key exited $STATUS"

# 9. A key with + and / in standard base64 compares equal across encodings.
write_env "$work/ledger-special.env" "control-space-decision-special" "$special_url"
write_env "$work/model-special.env" "control-space-decision-special" "$special_std"
run bash "$SCRIPT" --ledger "$work/ledger-special.env" --model-env "$work/model-special.env"
[[ "$STATUS" == 0 ]] || fail "url-safe and standard forms of one key compared unequal: $OUT"
assert_no_key_printed

# 10. Quotes, CRLF line endings and inline comments parse like Compose.
printf 'CONTROL_SPACE_DECISION_KEY_ID="control-space-decision-aaa"\r\nCONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64=%s # note\r\n' "$KEY_A_URL" >"$work/model-quoted.env"
run bash "$SCRIPT" --ledger "$ledger" --model-env "$work/model-quoted.env"
[[ "$STATUS" == 0 ]] || fail "quoted/CRLF env file mis-parsed (exit $STATUS): $OUT"

# 11. Missing ledger or an incomplete ledger cannot run the check (exit 2).
run bash "$SCRIPT" --ledger "$work/nope.env" --model-env "$ledger"
[[ "$STATUS" == 2 ]] || fail "missing ledger exited $STATUS"
run bash "$SCRIPT" --ledger "$work/model-empty.env" --model-env "$ledger"
[[ "$STATUS" == 2 ]] || fail "incomplete ledger exited $STATUS"
run bash "$SCRIPT" --bogus
[[ "$STATUS" == 2 ]] || fail "unknown argument exited $STATUS"

echo "check-control-decision-key tests passed"
