#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/tests/audit-delivery-proof.sh with stub docker and curl:
# every database read is a SELECT in a read-only transaction, the verdicts follow the data,
# and the baseline stage never fails. It never talks to a real daemon.

command -v jq >/dev/null 2>&1 || { echo "audit-delivery-proof contract test: SKIP (jq missing)"; exit 0; }

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/audit-delivery-proof.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/audit-delivery-proof-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# The proof reads the running revision's ancestry from the repository it sits in, so give the
# copy a tiny repository: commit one is the stand-in for the publisher fix, commit two is "running".
mkdir -p "$TMP/scripts/tests" "$TMP/bin"
cp "$SRC" "$TMP/scripts/tests/audit-delivery-proof.sh"
git -C "$TMP" init -q
git -C "$TMP" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m fix
FIX="$(git -C "$TMP" rev-parse HEAD)"
git -C "$TMP" -c user.name=t -c user.email=t@example.invalid commit -q --allow-empty -m later
RUNNING="$(git -C "$TMP" rev-parse HEAD)"
CALLS="$TMP/calls.log"

cat >"$TMP/bin/curl" <<'STUB'
#!/usr/bin/env bash
cat <<EOF
mp_session_audit_outbox_pending 0
mp_session_audit_outbox_terminal 0
mp_session_audit_outbox_terminal_by_subject{subject_class="audit_tool_action"} ${STUB_GAUGE:-0}
EOF
STUB

cat >"$TMP/bin/docker" <<'STUB'
#!/usr/bin/env bash
args="$(echo "$*" | tr '\n' ' ')"
echo "$args" >>"$CALLS"
case "$args" in
  "inspect --format {{.Image}}"*) echo "sha256:bbbbbbbbbbbbbbbbbbbbbbbb" ;;
  "inspect --format {{.State.StartedAt}}"*) echo "2026-10-23T10:00:00Z" ;;
  "image inspect --format {{index .Config.Labels"*) echo "$RUNNING" ;;
  "image inspect --format {{.Created}}"*) echo "2026-10-23T09:00:00Z" ;;
  *wget*) cat <<'JSON'
{"account_details":[{"stream_detail":[{"name":"VEREVON_CONTROL_OBSERVABILITY","state":{"messages":5000,"last_seq":5000},"config":{"subjects":["verevon.audit.v2.model.>"]},"consumer_detail":[
 {"name":"audit-core-model-v3-audit","config":{"filter_subject":"verevon.audit.v2.model.>"},"delivered":{"consumer_seq":4990},"num_pending":0,"num_ack_pending":0,"num_redelivered":0,"push_bound":true},
 {"name":"audit-core-model-v3-usage","config":{"filter_subject":"verevon.usage.v2.model.>"},"delivered":{"consumer_seq":0},"num_pending":0,"num_ack_pending":0,"num_redelivered":0,"push_bound":true}]}]}]}
JSON
    ;;
  *psql*)
    case "$args" in
      *"version = 43 and success"*) echo "1" ;;
      *"select version, success"*) echo "43|t" ;;
      *pg_proc*) echo "1" ;;
      *"requeued_at is not null"*) echo "10|10|0|1" ;;
      *"'zdr' = 'true'"*) echo "0" ;;
      *"select coalesce(last_error"*) ;;
      *"count(*) from session_audit_outbox where terminal_at is not null"*) echo "${STUB_TERMINAL:-0}" ;;
      *"terminal_at is null and published_at is null"*) echo "0" ;;
      *"event = 'tool_action'"*) echo "3" ;;
      *) echo "x|1" ;;
    esac
    ;;
  *) echo "unexpected docker call: $args" >&2; exit 99 ;;
esac
STUB
chmod +x "$TMP/bin/docker" "$TMP/bin/curl" "$TMP/scripts/tests/audit-delivery-proof.sh"

export CALLS RUNNING PATH="$TMP/bin:$PATH" AUDIT_FIX_COMMIT="$FIX"
failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }
proof() { : >"$CALLS"; bash "$TMP/scripts/tests/audit-delivery-proof.sh" "$@" >"$TMP/out" 2>&1; }

proof --stage final || { cat "$TMP/out" >&2; fail "clean final state must pass"; }
grep -q "failures=0" "$TMP/out" || fail "clean final state must report 0 failures"

STUB_TERMINAL=4992 proof --stage final && fail "terminal unpublished rows must fail the final stage"
grep -q "CHECK no-terminal-unpublished FAIL" "$TMP/out" || fail "the failing check must be named"

STUB_GAUGE=4992 proof --stage canary && fail "a non-zero terminal gauge must fail the canary stage"

STUB_TERMINAL=4992 STUB_GAUGE=4992 proof --stage baseline || fail "baseline must never fail"
grep -q "CHECK .* INFO" "$TMP/out" || fail "baseline must label its checks INFO"

proof --stage nonsense && fail "an unknown stage must be refused"

# Read-only: every database call carries the read-only option and no statement mutates.
proof --stage final || true
psql_calls="$(grep ' psql ' "$CALLS" || true)"
[[ -n "$psql_calls" ]] || fail "the proof must read the databases"
while IFS= read -r line; do
  [[ "$line" == *"PGOPTIONS=-c default_transaction_read_only=on"* ]] || fail "a database read without the read-only option: ${line:0:80}"
done <<<"$psql_calls"
if grep -qiE '\b(insert|update|delete|drop|alter|truncate|create|grant|revoke)\b' <<<"$psql_calls"; then
  fail "a database call contains a mutating statement"
fi
if grep -qE 'requeue_session_audit_terminal *\(' "$TMP/scripts/tests/audit-delivery-proof.sh"; then
  fail "the proof script must never call the requeue function"
fi

[[ "$failed" == "0" ]] || exit 1
echo "audit-delivery-proof contract test: ok"
