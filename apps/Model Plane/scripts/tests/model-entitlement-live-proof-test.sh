#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/tests/model-entitlement-live-proof.sh with a stub docker and a stub curl:
# every database read is a SELECT in a read-only transaction, the preflight catches an out-of-order
# deploy, a mode that is not the expected one (a typo included), a missing org-core credential, a
# gateway admin switch that is on early, a shadow that has not soaked, been unavailable or refused;
# the after phase passes or fails by what the operator said they did (refused, unavailable,
# shadow-served, served), and no credential reaches a command line or the output. It never talks to a
# real daemon or a real service.

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/model-entitlement-live-proof.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/model-entitlement-live-proof-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# The proof reads the ancestry of each running revision from the repository it sits in, so give the
# copy a tiny repository: commit one is the stand-in for the wave commit, commit two is "running".
mkdir -p "$TMP/apps/Model Plane/scripts/tests" "$TMP/bin"
SCRIPT="$TMP/apps/Model Plane/scripts/tests/model-entitlement-live-proof.sh"
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
  "inspect --format {{.Image}}"*)
    [[ -n "${STUB_MISSING:-}" && "$args" == *"$STUB_MISSING"* ]] && exit 1
    echo "sha256:cccccccccccccccccccccccc"
    ;;
  "inspect --format {{.State.StartedAt}}"*)
    # Started in flip order a long time ago, unless STUB_SWAP names a container made the oldest, or
    # STUB_YOUNG makes inference-core start now (a shadow that has not soaked). STUB_STARTED_<x>
    # changes inference-core's start time between before and after.
    case "$args" in
      *org-core-service*) t="2020-01-01T10:00:01" ;;
      *inference-core-1*) t="${STUB_INFERENCE_STARTED:-2020-01-01T10:00:02}" ;;
      *model-gateway-1*) t="2020-01-01T10:00:03" ;;
      *execution-core-1*) t="2020-01-01T10:00:04" ;;
    esac
    [[ -n "${STUB_SWAP:-}" && "$args" == *"$STUB_SWAP"* ]] && t="2019-01-01T09:00:00"
    [[ -n "${STUB_YOUNG:-}" && "$args" == *inference-core-1* ]] && t="$(date -u +%Y-%m-%dT%H:%M:%S)"
    echo "$t.000000000Z"
    ;;
  "inspect --format {{range .Config.Env}}"*)
    echo "RUST_LOG=info"
    echo "SOME_TOKEN=$SECRET_IN_ENV"
    if [[ "$args" == *inference-core-1* ]]; then
      [[ -n "${STUB_MODE+x}" ]] && echo "MODEL_ENTITLEMENT_MODE=$STUB_MODE"
      [[ -n "${STUB_TOKEN:-}" ]] && echo "INFERENCE_ORG_CORE_SERVICE_TOKEN=$SECRET_IN_ENV"
    fi
    if [[ "$args" == *verevon-gateway-rs* ]]; then
      [[ -n "${STUB_ADMIN+x}" ]] && echo "MODEL_ENTITLEMENT_ADMIN_ENABLED=$STUB_ADMIN"
    fi
    true
    ;;
  "image inspect --format {{index .Config.Labels"*) echo "$RUNNING" ;;
  *redis-cli*) for _ in $(seq 1 "${STUB_KEYS:-3}"); do echo "mp:gw:cache:k$_"; done ;;
  *psql*)
    case "$args" in
      *"schema_migrations"*) echo "${STUB_MIGRATION:-1}" ;;
      *"information_schema.tables"*) echo "3" ;;
      *"pg_roles"*) echo "${STUB_BYPASS:-t}" ;;
      *"from org_model_policy_revisions"*) echo "2" ;;
      *"from org_model_policy_history"*) echo "${STUB_HISTORY:-4}" ;;
      *"from org_model_policies"*) echo "3" ;;
      *"'zdr' = 'true'"*) echo "${STUB_ZDR_ROWS:-0}" ;;
      *"from threads"*) echo "${STUB_THREADS:-10}" ;;
      *"from messages"*) echo "40" ;;
      *"from runs"*) echo "${STUB_RUNS:-20}" ;;
      *"from feedback_ratings"*) echo "5" ;;
      *"from session_audit_outbox"*) echo "7" ;;
      *) echo "unexpected psql: $args" >&2; exit 99 ;;
    esac
    ;;
  *) echo "unexpected docker call: $args" >&2; exit 99 ;;
esac
STUB
cat >"$TMP/bin/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >>"$CALLS"
[[ -n "${STUB_CURL_FAIL:-}" ]] && exit 22
cat "$STUB_METRICS_FILE"
STUB
chmod +x "$TMP/bin/docker" "$TMP/bin/curl" "$SCRIPT"

export CALLS RUNNING SECRET_IN_ENV="$SECRET" PATH="$TMP/bin:$PATH" MODEL_ENTITLEMENT_WAVE_COMMIT="$WAVE"
failed=0
fail() { echo "FAIL: $*" >&2; failed=1; }
EVIDENCE="$TMP/evidence.txt"
proof() { : >"$CALLS"; bash "$SCRIPT" --out "$EVIDENCE" "$@" >"$TMP/out" 2>&1; }

# metrics FILE decision:mode=value ...: a Prometheus exposition holding those series.
metrics() {
  local file="$1" spec decision mode value
  shift
  {
    echo "# HELP inference_requests_total requests"
    echo 'inference_requests_total{rpc="infer",outcome="ok"} 9'
    for spec in "$@"; do
      decision="${spec%%:*}"
      mode="${spec#*:}"; mode="${mode%%=*}"
      value="${spec##*=}"
      echo "model_entitlement_total{decision=\"$decision\",mode=\"$mode\"} $value"
    done
  } >"$file"
}
NONE="$TMP/m-none.txt"
metrics "$NONE"
SOAKED="$TMP/m-soaked.txt"
metrics "$SOAKED" resolved:shadow=40 would_refuse:shadow=3 service_exempt:shadow=12
export STUB_METRICS_FILE="$NONE"

# ---- preflight, mode off: nothing counted, switches unset.
proof --phase preflight --expect-mode off || { cat "$TMP/out" >&2; fail "a clean off stack must pass the preflight"; }
grep -q "failures=0" "$TMP/out" || fail "a clean off preflight must report 0 failures"
grep -q "CHECK off-makes-no-org-core-call PASS" "$TMP/out" || fail "off with no decision counted must pass"
grep -q "CHECK admin-flag-is-false PASS" "$TMP/out" || fail "an unset admin switch must pass as false"
STUB_MODE=off proof --phase preflight --expect-mode off || fail "an explicit off must pass"
STUB_MODE=shadow proof --phase preflight --expect-mode off && fail "shadow when off is expected must fail"
grep -q "CHECK mode-is-off FAIL" "$TMP/out" || fail "the mode check must be named"
STUB_METRICS_FILE="$SOAKED" proof --phase preflight --expect-mode off && fail "decisions counted under off must fail"
grep -q "CHECK off-makes-no-org-core-call FAIL" "$TMP/out" || fail "the no-call check must be named"
STUB_CURL_FAIL=1 proof --phase preflight --expect-mode off && fail "unread metrics must fail the off check"

# ---- preflight, deploy shape.
STUB_SWAP=inference-core proof --phase preflight --expect-mode off && fail "inference-core started first must fail the preflight"
grep -q "CHECK deploy-order FAIL" "$TMP/out" || fail "the order check must be named"
STUB_MISSING=execution-core proof --phase preflight --expect-mode off && fail "a missing service must fail the preflight"
MODEL_ENTITLEMENT_WAVE_COMMIT=0000000000000000000000000000000000000000 proof --phase preflight --expect-mode off && fail "an unknown wave commit must fail the revision check"
grep -q "CHECK wave-revision FAIL" "$TMP/out" || fail "the revision check must be named"
STUB_MIGRATION=0 proof --phase preflight --expect-mode off && fail "a missing org-core migration must fail"
grep -q "CHECK org-core-migration-021 FAIL" "$TMP/out" || fail "the migration check must be named"
STUB_BYPASS=f proof --phase preflight --expect-mode off && fail "a reader that cannot see past row-level security must fail"
grep -q "CHECK org-core-reader-sees-all-rows FAIL" "$TMP/out" || fail "the reader check must be named"

# ---- preflight, shadow: soaked, answered, nothing unavailable, never refused.
export STUB_TOKEN=1 STUB_MODE=shadow STUB_METRICS_FILE="$SOAKED"
proof --phase preflight --expect-mode shadow || { cat "$TMP/out" >&2; fail "a soaked shadow stack must pass"; }
grep -q "would_refuse=3 service_exempt=12" "$TMP/out" || fail "the soak's findings must be printed for the operator"
STUB_YOUNG=1 proof --phase preflight --expect-mode shadow && fail "a shadow that has not soaked must fail"
grep -q "CHECK shadow-soaked FAIL" "$TMP/out" || fail "the soak check must be named"
STUB_METRICS_FILE="$NONE" proof --phase preflight --expect-mode shadow && fail "a shadow org-core never answered must fail"
grep -q "CHECK shadow-org-core-answered FAIL" "$TMP/out" || fail "the answered check must be named"
UNAVAIL="$TMP/m-unavail.txt"; metrics "$UNAVAIL" resolved:shadow=40 would_unavailable:shadow=2
STUB_METRICS_FILE="$UNAVAIL" proof --phase preflight --expect-mode shadow && fail "an unavailable shadow answer must fail"
grep -q "CHECK shadow-nothing-unavailable FAIL" "$TMP/out" || fail "the unavailable check must be named"
REFUSED="$TMP/m-refused.txt"; metrics "$REFUSED" resolved:shadow=40 refuse:shadow=1
STUB_METRICS_FILE="$REFUSED" proof --phase preflight --expect-mode shadow && fail "a refusal under shadow must fail"
grep -q "CHECK shadow-never-refuses FAIL" "$TMP/out" || fail "the shadow-never-refuses check must be named"
STUB_TOKEN= proof --phase preflight --expect-mode shadow && fail "shadow without the org-core credential must fail"
grep -q "CHECK org-core-credential FAIL" "$TMP/out" || fail "the credential check must be named"

# ---- preflight, enforce, and the spelling of the mode.
STUB_MODE=enforce proof --phase preflight --expect-mode enforce || { cat "$TMP/out" >&2; fail "enforce with its credential must pass"; }
STUB_MODE=" Enforce " proof --phase preflight --expect-mode enforce || fail "the mode is trimmed and case-insensitive, like the service"
STUB_MODE=enforced proof --phase preflight --expect-mode enforce && fail "a mistyped mode must fail even when enforce is expected"
grep -q "none of off, shadow, enforce" "$TMP/out" || fail "the typo must be explained"
STUB_MODE=enforced proof --phase preflight --expect-mode off && fail "a mistyped mode must fail when off is expected"
STUB_MODE=enforce STUB_TOKEN= proof --phase preflight --expect-mode enforce && fail "enforce without the credential must fail"

# ---- the admin switch is the literal true.
STUB_ADMIN=true proof --phase preflight --expect-mode off && fail "the admin switch on before step 5 must fail"
STUB_MODE=enforce STUB_ADMIN=true proof --phase preflight --expect-mode enforce --expect-admin true || fail "the admin switch on must pass when expected"
STUB_MODE=enforce STUB_ADMIN=True proof --phase preflight --expect-mode enforce --expect-admin true && fail "only the literal true enables the admin switch"
STUB_MODE=enforce STUB_ADMIN=false proof --phase preflight --expect-mode enforce || fail "a false admin switch must pass as false"

# ---- before and after.
STATE="$TMP/state.txt"
export STUB_MODE=enforce STUB_TOKEN=1 MODEL_DRAGONFLY_PASSWORD="$SECRET"
BEFORE_M="$TMP/m-before.txt"; metrics "$BEFORE_M" resolved:enforce=10 refuse:enforce=2 unavailable:enforce=0
AFTER_REFUSED="$TMP/m-after-refused.txt"; metrics "$AFTER_REFUSED" resolved:enforce=11 refuse:enforce=3 unavailable:enforce=0
AFTER_UNAVAIL="$TMP/m-after-unavail.txt"; metrics "$AFTER_UNAVAIL" resolved:enforce=10 refuse:enforce=2 unavailable:enforce=1
AFTER_SERVED="$TMP/m-after-served.txt"; metrics "$AFTER_SERVED" resolved:enforce=11 refuse:enforce=2 unavailable:enforce=0
AFTER_RESTART="$TMP/m-after-restart.txt"; metrics "$AFTER_RESTART" resolved:enforce=1
before() { STUB_METRICS_FILE="$BEFORE_M" proof --phase before --state "$STATE"; }

before || fail "before must never fail"
grep -q '^threads=10$' "$STATE" || fail "the state file must hold the thread count"
grep -q '^cache_keys=3$' "$STATE" || fail "the state file must hold the cache key count"
grep -q '^org_policy_history=4$' "$STATE" || fail "the state file must hold the history row count"
grep -q '^m_refuse=2$' "$STATE" || fail "the state file must hold the refuse sum"

STUB_METRICS_FILE="$AFTER_REFUSED" proof --phase after --state "$STATE" --expect refused || { cat "$TMP/out" >&2; fail "a refusal that wrote nothing must pass"; }
grep -q "failures=0" "$TMP/out" || fail "a clean refused after must report 0 failures"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_THREADS=11 proof --phase after --state "$STATE" --expect refused && fail "a new thread row after a refusal must fail"
grep -q "CHECK unchanged-threads FAIL" "$TMP/out" || fail "the failing counter must be named"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_RUNS=21 proof --phase after --state "$STATE" --expect refused && fail "a new run row after a refusal must fail"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_KEYS=4 proof --phase after --state "$STATE" --expect refused && fail "a new cache key after a refusal must fail"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_ZDR_ROWS=1 proof --phase after --state "$STATE" --expect refused && fail "a new ZDR outbox row after a refusal must fail"
STUB_METRICS_FILE="$BEFORE_M" proof --phase after --state "$STATE" --expect refused && fail "no refuse counted must fail the refused expectation"
grep -q "CHECK refuse-moved FAIL" "$TMP/out" || fail "the refuse check must be named"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_MODE=shadow proof --phase after --state "$STATE" --expect refused && fail "refused under shadow must fail"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_HISTORY=3 proof --phase after --state "$STATE" --expect refused && fail "shrinking history must fail"
grep -q "CHECK history-append-only FAIL" "$TMP/out" || fail "the history check must be named"
STUB_METRICS_FILE="$AFTER_RESTART" proof --phase after --state "$STATE" --expect served && fail "counters that went backwards must fail"
grep -q "CHECK metrics-monotonic FAIL" "$TMP/out" || fail "the monotonic check must be named"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_INFERENCE_STARTED=2020-02-02T10:00:00 proof --phase after --state "$STATE" --expect refused && fail "a restart between before and after must fail"
grep -q "CHECK inference-core-not-restarted FAIL" "$TMP/out" || fail "the restart check must be named"
STUB_METRICS_FILE="$AFTER_REFUSED" STUB_CURL_FAIL=1 proof --phase after --state "$STATE" --expect refused && fail "unread metrics must fail"
env -u MODEL_DRAGONFLY_PASSWORD STUB_METRICS_FILE="$AFTER_REFUSED" bash "$SCRIPT" --out "$EVIDENCE" --phase after --state "$STATE" --expect refused >"$TMP/out" 2>&1 && fail "an unread cache keyspace must fail"

STUB_METRICS_FILE="$AFTER_UNAVAIL" proof --phase after --state "$STATE" --expect unavailable || { cat "$TMP/out" >&2; fail "an unavailable refusal that wrote nothing must pass"; }
STUB_METRICS_FILE="$AFTER_UNAVAIL" STUB_THREADS=11 proof --phase after --state "$STATE" --expect unavailable && fail "a thread row under unavailable must fail"
STUB_METRICS_FILE="$AFTER_REFUSED" proof --phase after --state "$STATE" --expect unavailable && fail "no unavailable counted must fail"

STUB_METRICS_FILE="$AFTER_SERVED" proof --phase after --state "$STATE" --expect served || { cat "$TMP/out" >&2; fail "an entitled call that was served must pass"; }
STUB_METRICS_FILE="$AFTER_REFUSED" proof --phase after --state "$STATE" --expect served && fail "a refusal during a served call must fail"
STUB_METRICS_FILE="$AFTER_SERVED" STUB_THREADS=11 STUB_RUNS=21 proof --phase after --state "$STATE" --expect served || fail "a served call may write rows"

SHADOW_BEFORE="$TMP/m-shadow-before.txt"; metrics "$SHADOW_BEFORE" resolved:shadow=10 would_refuse:shadow=1
SHADOW_AFTER="$TMP/m-shadow-after.txt"; metrics "$SHADOW_AFTER" resolved:shadow=11 would_refuse:shadow=2
SHADOW_REFUSES="$TMP/m-shadow-refuses.txt"; metrics "$SHADOW_REFUSES" resolved:shadow=11 would_refuse:shadow=2 refuse:shadow=1
STUB_MODE=shadow STUB_METRICS_FILE="$SHADOW_BEFORE" proof --phase before --state "$STATE" || fail "before must never fail"
STUB_MODE=shadow STUB_METRICS_FILE="$SHADOW_AFTER" proof --phase after --state "$STATE" --expect shadow-served || { cat "$TMP/out" >&2; fail "a would-refuse that was served must pass under shadow"; }
STUB_MODE=shadow STUB_METRICS_FILE="$SHADOW_REFUSES" proof --phase after --state "$STATE" --expect shadow-served && fail "a refusal under shadow must fail"
STUB_MODE=shadow STUB_METRICS_FILE="$SHADOW_BEFORE" proof --phase after --state "$STATE" --expect shadow-served && fail "no would_refuse counted must fail"
STUB_MODE=enforce STUB_METRICS_FILE="$SHADOW_AFTER" proof --phase after --state "$STATE" --expect shadow-served && fail "shadow-served under enforce must fail"

# ---- argument handling.
proof --phase nonsense && fail "an unknown phase must be refused"
proof --phase preflight && fail "preflight without --expect-mode must be refused"
proof --phase preflight --expect-mode maybe && fail "an unknown expected mode must be refused"
proof --phase preflight --expect-mode off --expect-admin maybe && fail "an unknown expected admin value must be refused"
proof --phase before && fail "before without a state file must be refused"
proof --phase after --state "$TMP/missing.txt" --expect served && fail "after without a state file must be refused"
proof --phase after --state "$STATE" && fail "after without --expect must be refused"
proof --phase after --state "$STATE" --expect whatever && fail "an unknown expectation must be refused"

# ---- read-only, and the credential stays off command lines and out of the evidence.
STUB_METRICS_FILE="$AFTER_SERVED" proof --phase after --state "$STATE" --expect served || true
psql_calls="$(grep ' psql ' "$CALLS" || true)"
[[ -n "$psql_calls" ]] || fail "the proof must read the databases"
while IFS= read -r line; do
  [[ "$line" == *"PGOPTIONS=-c default_transaction_read_only=on"* ]] || fail "a database read without the read-only option: ${line:0:80}"
done <<<"$psql_calls"
if grep -qiE '\b(insert|update|delete|drop|alter|truncate|create|grant|revoke)\b' <<<"$psql_calls"; then
  fail "a database call contains a mutating statement"
fi
if grep -qE '(^| )docker (stop|start|restart|rm|kill|run|compose|exec .* (sh|bash))\b' "$CALLS"; then fail "the proof must not change the stack"; fi
if grep -q "$SECRET" "$CALLS"; then fail "the credential reached a command line"; fi
if grep -q "$SECRET" "$TMP/out" "$EVIDENCE" "$STATE"; then fail "the credential reached the output, the evidence or the state file"; fi
grep -q "Model entitlement live proof" "$EVIDENCE" || fail "the evidence file must receive the output"

[[ "$failed" == "0" ]] || exit 1
echo "model-entitlement-live-proof contract test: ok"
