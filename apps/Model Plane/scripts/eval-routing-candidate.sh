#!/usr/bin/env bash
set -euo pipefail

# EP-14 S10: the live leg of a RoutingPolicy candidate. It runs a named subset of the eval-lab live suite against a
# non-ZDR test org, then records the result on the candidate with session-core's RecordCandidateEvaluation. It prints
# counts only, never case text or model output, and it refuses a ZDR bearer before any live call. The offline diff is
# reviewed by a person: ROUTING_EVAL_DIFF_ACCEPTED carries that decision, and this script never makes it.

usage() {
  cat <<'EOF'
Usage:
  ROUTING_EVAL_CANDIDATE_ID=<cand_...> \
  ROUTING_EVAL_DIFF_ACCEPTED=<true|false> \
  ROUTING_EVAL_BEARER=<admin bearer for session-core> \
  ROUTING_EVAL_ORG_ID=<non-ZDR test org> \
  ROUTING_EVAL_SESSION_CORE=<host:port of session-core's gRPC listener> \
    scripts/eval-routing-candidate.sh

Optional:
  ROUTING_EVAL_SUBSET           eval-lab pytest -k expression (default: baseline)
  ROUTING_EVAL_WAIVER_REASON    written reason to promote without a passing live eval
  ROUTING_EVAL_DECIDED_BY       identity recorded on the review (default: routing-candidate-eval)
  ROUTING_EVAL_OUT              file for the archived output (default: a new file in TMPDIR)

Requires grpcurl and python3, and the eval-lab venv (make venv in python/eval-lab-py).

The bearer is the token alone, without the word Bearer. The script never prints it, but grpcurl receives it as an
argument for one call, so run the script on a host you trust. It refuses a bearer whose claims mark the org as ZDR.
A rejected diff (ROUTING_EVAL_DIFF_ACCEPTED=false) records the rejection and makes no live call.

The live eval measures what the target routes. Load the candidate's config_json (ListCandidates returns it) into the
non-ZDR test target before running this script; the script does not load it.
EOF
}

die() {
  echo "routing candidate eval: $*" >&2
  exit 1
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  "") ;;
  *)
    usage >&2
    exit 2
    ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
model_plane_dir="$script_dir/.."
eval_dir="$model_plane_dir/python/eval-lab-py"
proto_dir="$model_plane_dir/proto"
[[ -f "$proto_dir/model_plane/v1/routing_policy.proto" ]] || die "the routing_policy proto is not at $proto_dir"
[[ -d "$eval_dir" ]] || die "the eval-lab package is not at $eval_dir"

for tool in grpcurl python3; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done

[[ -n "${ROUTING_EVAL_CANDIDATE_ID:-}" ]] || die "set ROUTING_EVAL_CANDIDATE_ID to the candidate to review"
case "${ROUTING_EVAL_DIFF_ACCEPTED:-}" in
  true | false) ;;
  *) die "set ROUTING_EVAL_DIFF_ACCEPTED to true or false, the reviewer's decision on the offline diff" ;;
esac
[[ -n "${ROUTING_EVAL_BEARER:-}" ]] || die "set ROUTING_EVAL_BEARER to an admin bearer for session-core"
[[ -n "${ROUTING_EVAL_ORG_ID:-}" ]] || die "set ROUTING_EVAL_ORG_ID to the non-ZDR test org"
[[ -n "${ROUTING_EVAL_SESSION_CORE:-}" ]] || die "set ROUTING_EVAL_SESSION_CORE to session-core's gRPC host:port"

out="${ROUTING_EVAL_OUT:-$(mktemp "${TMPDIR:-/tmp}/routing-candidate-eval.XXXXXX")}"
subset="${ROUTING_EVAL_SUBSET:-baseline}"
decided_by="${ROUTING_EVAL_DECIDED_BY:-routing-candidate-eval}"
waiver="${ROUTING_EVAL_WAIVER_REASON:-}"

# A ZDR bearer is refused before any live call. The token travels in the environment, not in argv.
ROUTING_EVAL_TOKEN_CHECK="$ROUTING_EVAL_BEARER" python3 - <<'PY' || die "the bearer is not a usable admin JWT, or it marks its org as ZDR"
import base64
import json
import os
import sys

parts = os.environ["ROUTING_EVAL_TOKEN_CHECK"].strip().split(".")
if len(parts) != 3:
    sys.exit(1)
claims = json.loads(base64.urlsafe_b64decode(parts[1] + "=" * (-len(parts[1]) % 4)))
if claims.get("zdr") is True:
    sys.exit(1)
PY

printf '== routing candidate %s ==\n' "$ROUTING_EVAL_CANDIDATE_ID" | tee -a "$out"

live_passed=false
passed=0
failed=0
skipped=0
eval_status=0

if [[ "$ROUTING_EVAL_DIFF_ACCEPTED" == "true" ]]; then
  pytest_bin="$eval_dir/.venv/bin/pytest"
  [[ -x "$pytest_bin" ]] || die "the eval-lab venv is missing; run make venv in $eval_dir"
  live_log="$(mktemp "${TMPDIR:-/tmp}/routing-candidate-live.XXXXXX")"
  # The live log stays out of the archive: it can hold model output on a failure. Only counts leave this block.
  (cd "$eval_dir" && EVAL_LIVE=1 "$pytest_bin" tests/test_live_suite.py -q -rs -k "$subset") \
    >"$live_log" 2>&1 || eval_status=$?
  read -r passed failed skipped < <(python3 - "$live_log" <<'PY'
import re
import sys

text = open(sys.argv[1], encoding="utf-8", errors="replace").read()


def count(word):
    found = re.findall(r"(\d+) %s" % word, text)
    return int(found[-1]) if found else 0


print(count("passed"), count("failed"), count("skipped"))
PY
  )
  rm -f "$live_log"
  if [[ "$eval_status" -eq 0 && "$failed" -eq 0 && "$passed" -gt 0 ]]; then
    live_passed=true
  fi
  printf 'live eval (subset %s): passed=%s failed=%s skipped=%s\n' "$subset" "$passed" "$failed" "$skipped" | tee -a "$out"
else
  printf 'the diff is rejected: no live eval is run\n' | tee -a "$out"
fi

# The review is recorded whatever the live result, so a failed eval is on the candidate and not lost in a terminal.
record_body="$(
  ROUTING_EVAL_CANDIDATE_ID="$ROUTING_EVAL_CANDIDATE_ID" \
    ROUTING_EVAL_DIFF_ACCEPTED="$ROUTING_EVAL_DIFF_ACCEPTED" \
    ROUTING_EVAL_LIVE_PASSED="$live_passed" \
    ROUTING_EVAL_PASSED="$passed" \
    ROUTING_EVAL_FAILED="$failed" \
    ROUTING_EVAL_SKIPPED="$skipped" \
    ROUTING_EVAL_SUBSET="$subset" \
    ROUTING_EVAL_WAIVER="$waiver" \
    ROUTING_EVAL_DECIDED_BY="$decided_by" \
    python3 - <<'PY'
import json
import os

env = os.environ
accepted = env["ROUTING_EVAL_DIFF_ACCEPTED"] == "true"
live_eval = {}
if accepted:
    live_eval = {
        "passed": env["ROUTING_EVAL_LIVE_PASSED"] == "true",
        "passed_count": int(env["ROUTING_EVAL_PASSED"]),
        "failed_count": int(env["ROUTING_EVAL_FAILED"]),
        "skipped_count": int(env["ROUTING_EVAL_SKIPPED"]),
        "subset": env["ROUTING_EVAL_SUBSET"],
    }
print(json.dumps({
    "candidate_id": env["ROUTING_EVAL_CANDIDATE_ID"],
    "diff_accepted": accepted,
    "live_eval_json": json.dumps(live_eval),
    "waiver_reason": env["ROUTING_EVAL_WAIVER"],
    "decided_by": env["ROUTING_EVAL_DECIDED_BY"],
}))
PY
)"

printf '%s' "$record_body" |
  grpcurl -plaintext \
    -import-path "$proto_dir" -proto model_plane/v1/routing_policy.proto \
    -H "authorization: Bearer $ROUTING_EVAL_BEARER" \
    -d @ "$ROUTING_EVAL_SESSION_CORE" model_plane.v1.RoutingPolicy/RecordCandidateEvaluation |
  tee -a "$out" >/dev/null || die "session-core refused the review; see $out"

if [[ "$ROUTING_EVAL_DIFF_ACCEPTED" != "true" ]]; then
  echo "routing candidate eval: the rejection is recorded; no live call was made. Output archived at $out"
  exit 0
fi

if [[ "$live_passed" != "true" && -z "$waiver" ]]; then
  echo "routing candidate eval: the live eval did not pass and no waiver was given; promotion stays refused. Output archived at $out" >&2
  exit 1
fi

echo "routing candidate eval: the review is recorded and the candidate is evaluated. PromoteCandidate makes it live. Output archived at $out"
