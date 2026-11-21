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
# A psql call fed its SQL on stdin (exec -i): the SQL joins the logged line, so the patterns below match it.
case "$args" in "exec -i "*) args="$args stdin: $(tr '\n' ' ')" ;; esac
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
      *"from threads"*)
        # Another org's threads are in the stack-wide count only (STUB_OTHER_THREADS), never in an org-bound read.
        if [[ "$args" == *"where org_id"* ]]; then echo "${STUB_THREADS:-10}"; else echo "$(( ${STUB_THREADS:-10} + ${STUB_OTHER_THREADS:-0} ))"; fi ;;
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
unset ZDR_PROOF_ORG_ID STUB_OTHER_THREADS
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

# Org scope: one org's counters do not move when another org's traffic lands, while the stack-wide
# ones still do. The id reaches psql as the variable org, and the output names no id.
ORG_TEST_ID="fixture-org-aaaaaaaa"
ORG_STATE="$TMP/org-state.txt"
ZDR_PROOF_ORG_ID="$ORG_TEST_ID" MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase before --state "$ORG_STATE" ||
  fail "org-scoped before must never fail"
grep -qx "org_scope=on" "$TMP/out" || fail "the org scope must be named in the evidence"
grep -qx "threads=10 scope=org" "$TMP/out" || fail "org counters must be labelled scope=org"
grep -qx "feedback_ratings=5 scope=stack" "$TMP/out" || fail "feedback_ratings must be labelled scope=stack"
grep -qF "where org_id = :'org'" "$CALLS" || fail "the threads read must bind the org"
grep -qF "join threads t on t.id = m.thread_id where t.org_id = :'org'" "$CALLS" || fail "the messages read must reach the org through its thread"
grep -qF "payload->>'org_id' = :'org'" "$CALLS" || fail "the outbox reads must bind the org"
grep -qF -- "-v org=$ORG_TEST_ID" "$CALLS" || fail "the org must reach psql as the variable org"
if grep -qF "$ORG_TEST_ID" "$TMP/out" "$EVIDENCE" "$ORG_STATE"; then
  fail "the org id must not reach the output, the evidence or the state file"
fi

# Another org's threads move the stack-wide count only: the org-scoped after phase passes, and the
# stack-wide after phase still fails on the same traffic.
STUB_OTHER_THREADS=5 ZDR_PROOF_ORG_ID="$ORG_TEST_ID" MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$ORG_STATE" ||
  { cat "$TMP/out" >&2; fail "another org's threads must not fail an org-scoped after phase"; }
grep -q "failures=0" "$TMP/out" || fail "an org-scoped after phase with other traffic must report 0 failures"
grep -qx "threads: before=10 after=10 delta=0 scope=org" "$TMP/out" || fail "the org delta must be labelled scope=org"
STUB_OTHER_THREADS=5 MODEL_DRAGONFLY_PASSWORD="$SECRET" proof --phase after --state "$STATE" &&
  fail "the stack-wide after phase must still fail on other traffic"
grep -q "CHECK unchanged-threads FAIL" "$TMP/out" || fail "the stack-wide failure must name the counter"

# Unset means the old output: no scope line and no org binding.
proof --phase before --state "$TMP/unset-state.txt" || fail "unset org scope: before must never fail"
if grep -qE "org_scope=on|scope=(org|stack)" "$TMP/out"; then fail "an unset org scope must not change the output"; fi
if grep -qF ":'org'" "$CALLS"; then fail "an unset org scope must not bind an org"; fi

# A value that is not an org id, and an empty one, is refused before anything is read or written.
ZDR_PROOF_ORG_ID="x'; drop table threads; --" proof --phase before --state "$TMP/bad-state.txt" &&
  fail "an org id with SQL characters must be refused"
grep -q "is not an org id" "$TMP/out" || fail "the refusal must say why"
if grep -q "drop table" "$TMP/out"; then fail "the refused value must not be printed"; fi
ZDR_PROOF_ORG_ID= proof --phase before --state "$TMP/bad-state.txt" && fail "an empty org id must be refused, not read as stack-wide"
[[ ! -e "$TMP/bad-state.txt" ]] || fail "a refused org id must write no state"

[[ "$failed" == "0" ]] || exit 1
echo "zdr-live-proof contract test: ok"
