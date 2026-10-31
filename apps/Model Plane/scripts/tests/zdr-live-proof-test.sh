#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/tests/zdr-live-proof.sh with a stub docker: every database read is a
# SELECT in a read-only transaction, the preflight catches an out-of-order deploy and a waiver
# switch that is on, the after phase fails on any row or cache key that appeared, and the
# credential never reaches a command line or the output. It never talks to a real daemon.

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/zdr-live-proof.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/zdr-live-proof-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# The proof reads the ancestry of each running revision from the repository it sits in, so give the
# copy a tiny repository: commit one is the stand-in for the wave commit, commit two is "running".
mkdir -p "$TMP/apps/Model Plane/scripts/tests" "$TMP/bin"
SCRIPT="$TMP/apps/Model Plane/scripts/tests/zdr-live-proof.sh"
cp "$SRC" "$SCRIPT"
git -C "$TMP" init -q
git -C "$TMP" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m wave
WAVE="$(git -C "$TMP" rev-parse HEAD)"
git -C "$TMP" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m later
RUNNING="$(git -C "$TMP" rev-parse HEAD)"
CALLS="$TMP/calls.log"
SECRET="stub-secret-value-never-printed"

cat >"$TMP/bin/docker" <<'STUB'
#!/usr/bin/env bash
args="$(echo "$*" | tr '\n' ' ')"
echo "$args" >>"$CALLS"
case "$args" in
  "inspect --format {{.Image}}"*) echo "sha256:cccccccccccccccccccccccc" ;;
  "inspect --format {{.State.StartedAt}}"*)
    # Started in wave order, unless STUB_SWAP names a service that is made the oldest.
    case "$args" in
      *session-core-1*) t="10:00:01" ;;
      *execution-core-1*) t="10:00:02" ;;
      *model-gateway-1*) t="10:00:03" ;;
      *inference-core-1*) t="10:00:04" ;;
    esac
    [[ -n "${STUB_SWAP:-}" && "$args" == *"$STUB_SWAP"-1* ]] && t="09:00:00"
    echo "2026-10-23T$t.000000000Z"
    ;;
  "inspect --format {{range .Config.Env}}"*)
    echo "RUST_LOG=info"
    echo "SOME_TOKEN=$SECRET_IN_ENV"
    [[ -n "${STUB_WAIVER:-}" ]] && echo "ZDR_WAIVER_ENABLED=$STUB_WAIVER"
    true
    ;;
  "image inspect --format {{index .Config.Labels"*) echo "$RUNNING" ;;
  *redis-cli*) for _ in $(seq 1 "${STUB_KEYS:-3}"); do echo "mp:gw:cache:k$_"; done ;;
  *psql*)
    case "$args" in
      *"_sqlx_migrations"*) echo "2" ;;
      *information_schema*) echo "1" ;;
      *"zdr_waived"*) echo "${STUB_WAIVED:-0}" ;;
      *"'zdr' = 'true'"*) echo "${STUB_ZDR_ROWS:-0}" ;;
      *"from threads"*) echo "${STUB_THREADS:-10}" ;;
      *"from messages"*) echo "40" ;;
      *"from runs"*) echo "20" ;;
      *"from feedback_ratings"*) echo "5" ;;
      *"from session_audit_outbox"*) echo "7" ;;
      *) echo "unexpected psql: $args" >&2; exit 99 ;;
    esac
    ;;
  *) echo "unexpected docker call: $args" >&2; exit 99 ;;
esac
STUB
chmod +x "$TMP/bin/docker" "$SCRIPT"

export CALLS RUNNING SECRET_IN_ENV="$SECRET" PATH="$TMP/bin:$PATH" ZDR_WAVE_COMMIT="$WAVE"
failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }
EVIDENCE="$TMP/evidence.txt"
proof() { : >"$CALLS"; bash "$SCRIPT" --out "$EVIDENCE" "$@" >"$TMP/out" 2>&1; }

proof --phase preflight || { cat "$TMP/out" >&2; fail "an ordered, clean wave must pass the preflight"; }
grep -q "failures=0" "$TMP/out" || fail "a clean preflight must report 0 failures"
grep -q "waiver-minting-off PASS" "$TMP/out" || fail "an unset waiver switch must pass"

STUB_SWAP=inference-core proof --phase preflight && fail "inference-core started first must fail the preflight"
grep -q "CHECK deploy-order FAIL" "$TMP/out" || fail "the order check must be named"

STUB_WAIVER=true proof --phase preflight && fail "waiver minting on must fail the preflight"
grep -q "CHECK waiver-minting-off FAIL" "$TMP/out" || fail "the waiver check must be named"
STUB_WAIVER=false proof --phase preflight || fail "a waiver switch that is not the literal true must pass"

STUB_WAIVED=1 proof --phase preflight && fail "a waived run must fail the preflight"
STUB_ZDR_ROWS=2 proof --phase preflight && fail "a ZDR row in the outbox must fail the preflight"

ZDR_WAVE_COMMIT=0000000000000000000000000000000000000000 proof --phase preflight && fail "an unknown wave commit must fail the revision check"

# before records, after compares.
STATE="$TMP/state.txt"
MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase before --state "$STATE" || fail "before must never fail"
grep -q '^threads=10$' "$STATE" || fail "the state file must hold the thread count"
grep -q '^cache_keys=3$' "$STATE" || fail "the state file must hold the cache key count"

MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" || { cat "$TMP/out" >&2; fail "unchanged counters must pass"; }
grep -q "failures=0" "$TMP/out" || fail "unchanged counters must report 0 failures"

STUB_THREADS=11 MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" && fail "a new thread row must fail"
grep -q "CHECK unchanged-threads FAIL" "$TMP/out" || fail "the failing counter must be named"
STUB_KEYS=4 MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" && fail "a new cache key must fail"
STUB_ZDR_ROWS=1 MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" && fail "a ZDR outbox row must fail"
proof --phase after --state "$STATE" && fail "an unread cache keyspace must fail the after phase"

proof --phase nonsense && fail "an unknown phase must be refused"
proof --phase after --state "$TMP/missing.txt" && fail "after without a state file must be refused"

# Read-only, and the credential stays off command lines and out of the evidence.
MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" || true
psql_calls="$(grep ' psql ' "$CALLS" || true)"
[[ -n "$psql_calls" ]] || fail "the proof must read the database"
while IFS= read -r line; do
  [[ "$line" == *"PGOPTIONS=-c default_transaction_read_only=on"* ]] || fail "a database read without the read-only option: ${line:0:80}"
done <<<"$psql_calls"
if grep -qiE '\b(insert|update|delete|drop|alter|truncate|create|grant|revoke)\b' <<<"$psql_calls"; then
  fail "a database call contains a mutating statement"
fi
if grep -q "$SECRET" "$CALLS"; then fail "the credential reached a docker command line"; fi
if grep -q "$SECRET" "$TMP/out" "$EVIDENCE" "$STATE"; then fail "the credential reached the output, the evidence or the state file"; fi
grep -q "ZDR live proof" "$EVIDENCE" || fail "the evidence file must receive the output"

[[ "$failed" == "0" ]] || exit 1
echo "zdr-live-proof contract test: ok"
