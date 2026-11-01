#!/usr/bin/env bash
# Live smoke for the run trace NATS principal (WS10.S5).
#
# Proves, against a RUNNING Model Plane broker that has loaded the new
# deploy/nats.conf and been through the nats-provisioner one-shot, that the
# `quarry-trace-producer` principal can publish the run trace subject and
# nothing else:
#
#   allowed   publish  mp.v1.trace.run.smoke
#   refused   publish  mp.v1.run.smoke.event        (run event stream)
#   refused   publish  tools.completions.smoke      (tool completions)
#   refused   publish  verevon.audit.v2.model.x     (audit observability)
#   refused   publish  mp.v1.trace.run.smoke.extra  (a dotted run id)
#   refused   subscribe mp.v1.>                     (no reading the bus)
#
# The same rules are proved without a broker by the Go tests in
# apps/Model Plane/go/services/nats-provisioner (run_trace_test.go: an in-process
# broker built from the real nats.conf); this script is the live-stack twin and
# belongs to the stack session, not to a build wave. It publishes ONE message to
# the trace stream (subject mp.v1.trace.run.smoke, Nats-Msg-Id smoke-<epoch>); it
# creates no stream, consumer or credential.
#
# Needs the `nats` CLI (https://github.com/nats-io/natscli) on PATH.
#
#   NATS_URL                          broker URL            (default nats://127.0.0.1:4222)
#   NATS_USER                         principal             (default quarry-trace-producer)
#   MODEL_QUARRY_TRACE_NATS_PASSWORD  its password, from the environment only
#                                     (never read from a file by this script)
#
#   scripts/tests/model-trace-nats-smoke.sh            run the checks
#   scripts/tests/model-trace-nats-smoke.sh --list     print the checks and exit
#
# A refused publish is dropped by the broker and reported asynchronously, so a
# check passes only when the CLI output carries "Permissions Violation". Status:
# written against the Go-verified rules; NOT yet run against a live broker.
set -Eeuo pipefail

NATS_URL="${NATS_URL:-nats://127.0.0.1:4222}"
NATS_USER="${NATS_USER:-quarry-trace-producer}"

ALLOWED_PUBLISH=("mp.v1.trace.run.smoke")
REFUSED_PUBLISH=(
  "mp.v1.run.smoke.event"
  "tools.completions.smoke"
  "verevon.audit.v2.model.x"
  "mp.v1.trace.run.smoke.extra"
)
REFUSED_SUBSCRIBE=("mp.v1.>")

if [[ "${1:-}" == "--list" ]]; then
  for s in "${ALLOWED_PUBLISH[@]}"; do printf 'allowed  publish   %s\n' "$s"; done
  for s in "${REFUSED_PUBLISH[@]}"; do printf 'refused  publish   %s\n' "$s"; done
  for s in "${REFUSED_SUBSCRIBE[@]}"; do printf 'refused  subscribe %s\n' "$s"; done
  exit 0
fi

if ! command -v nats >/dev/null 2>&1; then
  echo "the nats CLI is required (a check that cannot run is a failure)" >&2
  exit 2
fi
if [[ -z "${MODEL_QUARRY_TRACE_NATS_PASSWORD:-}" ]]; then
  echo "MODEL_QUARRY_TRACE_NATS_PASSWORD must be set in the environment" >&2
  exit 2
fi

passes=0
failures=0
pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

# The password travels in the environment of the child only (natscli reads
# NATS_PASSWORD), never in argv. A granted subscription would block waiting for
# traffic, so every call is bounded; a refusal is reported at once.
run_nats() {
  NATS_PASSWORD="$MODEL_QUARRY_TRACE_NATS_PASSWORD" timeout 8 nats --server "$NATS_URL" --user "$NATS_USER" "$@" 2>&1
}

for subject in "${ALLOWED_PUBLISH[@]}"; do
  out="$(run_nats pub "$subject" "smoke-$(date +%s)" || true)"
  if grep -qi 'permissions violation\|authorization violation\|error' <<<"$out"; then
    fail "publish to $subject must be allowed: $out"
  else
    pass "publish to $subject is allowed"
  fi
done

for subject in "${REFUSED_PUBLISH[@]}"; do
  out="$(run_nats pub "$subject" "smoke-refused" || true)"
  if grep -qi 'permissions violation' <<<"$out"; then
    pass "publish to $subject is refused"
  else
    fail "publish to $subject must be refused, the broker said: ${out:-nothing}"
  fi
done

for subject in "${REFUSED_SUBSCRIBE[@]}"; do
  out="$(run_nats sub "$subject" --count 1 || true)"
  if grep -qi 'permissions violation' <<<"$out"; then
    pass "subscribe to $subject is refused"
  else
    fail "subscribe to $subject must be refused, the broker said: ${out:-nothing}"
  fi
done

printf '%d passed, %d failed\n' "$passes" "$failures"
[[ "$failures" -eq 0 ]]
