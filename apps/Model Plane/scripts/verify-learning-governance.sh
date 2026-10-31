#!/usr/bin/env bash
# verify-learning-governance.sh — the learning-review governance proof (WS07-S10).
#
# It proves, without docker and without a live model, that:
#   - the review keeps its gates in the order the privacy and cost rules need:
#     retention, then owner, then memory and org policy, all before the
#     transcript read, and the cost gate before the model call;
#   - a learned skill can only be written as an owner-scoped, disabled proposal;
#   - the cost controls stay off unless LEARNING_REVIEW_COST_CONTROLS is "true",
#     and the review never counts a ZDR run against a cap;
#   - the Space-origin control is enforced upstream, in session-core, before a run
#     is announced;
#   - the seeded cases replay to their expectations, deterministically, and the
#     live harness compiles under -tags liveeval without reading any credential.
#
# Each check prints PASS or FAIL. The first FAIL stops the script with exit 1.
#
# Usage:    bash "apps/Model Plane/scripts/verify-learning-governance.sh"
# Requires: go on PATH. No docker, no database, no live model.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CC="$ROOT/go/services/capability-core"
CONSUMER="$CC/internal/sessionreview/consumer.go"
SINK="$CC/internal/skillsink/sink.go"
EVAL_DIR="$CC/internal/learning/eval"
SESSION_EVENTS="$ROOT/rust/services/session-core/src/learning_events.rs"
PLAN="$ROOT/docs/AGENT_QUALITY_PLAN_2026-07-29.md"
PASSED=0

pass() { PASSED=$((PASSED + 1)); echo "PASS: $1"; }
fail() { echo "FAIL: $1" >&2; exit 1; }

# line_of FILE TEXT: the first line of FILE that contains TEXT, or nothing.
line_of() { grep -nF -- "$2" "$1" | head -n 1 | cut -d: -f1 || true; }

# ordered NAME A B: both lines exist and A comes before B.
ordered() {
  local name="$1" a="$2" b="$3"
  if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ]; then
    pass "$name (lines $a < $b)"
  else
    fail "$name: expected line '${a:-missing}' before '${b:-missing}'"
  fi
}

# contains NAME FILE TEXT: FILE contains TEXT.
contains() {
  if grep -qF -- "$3" "$2"; then pass "$1"; else fail "$1: '$3' not found in ${2#"$ROOT/"}"; fi
}

# absent NAME FILE TEXT: FILE does not contain TEXT.
absent() {
  if grep -qF -- "$3" "$2"; then fail "$1: '$3' is still present in ${2#"$ROOT/"}"; else pass "$1"; fi
}

echo "==> gate order in the learning review (consumer.go)"
retention=$(line_of "$CONSUMER" "parseRetentionPosture(data)")
owner=$(line_of "$CONSUMER" 'ref.UserID == ""')
memory=$(line_of "$CONSUMER" "learningSkip(ctx, options.controls, ref)")
reader=$(line_of "$CONSUMER" "var src TranscriptSource = NewSessionCoreTranscriptSource(sc)")
capped=$(line_of "$CONSUMER" "src = costGatedSource{inner: src")
model=$(line_of "$CONSUMER" "OnRunCompleted(ctx, ref, src, reviewer, sink)")
ordered "retention posture gate precedes owner gate" "$retention" "$owner"
ordered "owner gate precedes memory and org policy gate" "$owner" "$memory"
ordered "memory and org policy gate precedes the transcript reader" "$memory" "$reader"
ordered "cost gate wraps the transcript reader" "$reader" "$capped"
ordered "cost gate precedes the model call" "$capped" "$model"

echo "==> proposal shape (skillsink/sink.go)"
if grep -Eq 'Enabled: +false,' "$SINK" && grep -Eq 'Scope: +"user",' "$SINK" && grep -Eq 'ReviewState: +"proposed",' "$SINK"; then
  pass "a learned skill is written disabled, user-scoped and proposed"
else
  fail "skillsink does not force a disabled, user-scoped, proposed row"
fi

echo "==> cost controls (consumer.go)"
contains "cost controls are switched on only by the literal \"true\"" "$CONSUMER" 'lookup(learningReviewCostEnv) != "true"'
contains "a skipped review is counted by reason" "$CONSUMER" 'learning_review_skipped_total'
contains "a counter that cannot be read fails closed" "$CONSUMER" 'return skipBudgetUnavailable'
contains "a ZDR run is refused before any cost gate can count it" "$CONSUMER" 'if posture := parseRetentionPosture(data); !posture.AllowsDerivedPersistence() {'

echo "==> Space origin is enforced upstream (session-core)"
contains "a non-chat thread origin is dropped before the outbox insert" "$SESSION_EVENTS" 'identity.thread_origin.as_deref() != Some("chat")'
contains "the origin gate has its precedence test" "$SESSION_EVENTS" 'memory_state_and_origin_gates_precede_the_outbox_insert'

echo "==> seeded cases and live harness"
cases=$(find "$EVAL_DIR/testdata/seeded" -name '*.json' | wc -l | tr -d '[:space:]')
if [ "$cases" -ge 15 ]; then pass "$cases seeded cases present"; else fail "only $cases seeded cases; the suite needs at least 15"; fi
absent "the eval and the live harness read no credential" "$EVAL_DIR/replay.go" "SERVICE_TOKEN"
absent "the fixture loader reads no credential" "$EVAL_DIR/fixture.go" "SERVICE_TOKEN"
absent "the live harness reads no provider key" "$EVAL_DIR/live_test.go" "API_KEY"

echo "==> go tests: learning, sessionreview, skillsink, llmreviewer, eval"
LOG="$(mktemp)"
if (cd "$CC" && go test -count=1 ./internal/learning/... ./internal/sessionreview/... ./internal/skillsink/... ./internal/llmreviewer/... >"$LOG" 2>&1); then
  pass "go test passes for the learning packages ($(grep -c '^ok' "$LOG") packages)"
else
  tail -n 40 "$LOG" >&2
  rm -f "$LOG"
  fail "go test failed for the learning packages"
fi
rm -f "$LOG"

echo "==> live harness compiles under -tags liveeval"
if (cd "$CC" && go vet -tags liveeval ./internal/learning/eval/); then
  pass "the liveeval harness compiles"
else
  fail "the liveeval harness does not compile"
fi

echo "==> quality plan no longer describes the review as on by default"
absent "the stale sampling rule is gone from the quality plan" "$PLAN" "plus a sampling/eligibility rule"
contains "the quality plan records the cost controls as built" "$PLAN" "LEARNING_REVIEW_COST_CONTROLS"

echo "==> evidence validator present"
contains "scripts/eval-evidence.sh validates evidence directories" "$ROOT/scripts/eval-evidence.sh" "validate)"

echo "governance proof: $PASSED checks passed"
