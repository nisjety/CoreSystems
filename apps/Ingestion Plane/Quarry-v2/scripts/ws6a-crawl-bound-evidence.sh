#!/usr/bin/env bash
# ws6a-crawl-bound-evidence.sh: the 7-page crawl recipe for the W1 crawl bound
# (plan WS6.A; SC7, crawl clause). It is a recipe for the exit-gate stack. The
# build does not run it, and it must not be run against the live stack.
#
# What the recipe checks, on a stack whose edge and orchestrator were started
# with QUARRY_CRAWL_DEFAULT_MAX_PAGES=7:
#   1. the gateway's POST /api/v1/knowledge/crawl, sent with no max_pages, answers
#      data.limits.max_pages {effective: 7, clamped_by: ["default"]};
#   2. the crawl it started runs to the bound: the job record in quarry-control
#      shows params.max_pages == 7 and, once the job has statistics,
#      statistics.pages_visited == 7. The seed must be a public site with more
#      than 7 internal links, or the bound is never reached.
# Each check prints PASS, FAIL or NOT-RUN and the value it read.
#
# Not covered here, and recorded by hand in docs/RUNBOOKS.md (WS6.A section):
# the pre-fix capture on an image built before the crawl bound (5995eeca), and
# the recorded-history replay of the workflow.
#
# Inputs come from the environment. No secret goes on the command line or into
# this file: the session cookie is read by curl from the file you name, and it
# is never printed.
#   GATEWAY_URL          the gateway of the exit-gate stack, e.g. https://localhost:8443
#   CONTROL_URL          quarry-control of the exit-gate stack, e.g. http://localhost:8081
#   ORG_ID               the organization of the seeded dev account that owns the cookie
#   SESSION_COOKIE_FILE  a cookie file for that seeded account (curl -b reads it)
#   SEED_URL             a documented public site with more than 7 internal links
# Optional:
#   EVIDENCE_DIR         where the JSON transcripts are kept (default: a new temp dir)
#   POLL_SECONDS         how long to wait for the job to finish (default 900)
#   CURL_OPTS            extra curl options, e.g. --cacert /path/to/ca.pem
#
# Usage:
#   ./scripts/ws6a-crawl-bound-evidence.sh               # the recipe (exit gate only)
#   ./scripts/ws6a-crawl-bound-evidence.sh --self-test   # check() and json_get() on canned JSON; no network
set -uo pipefail

RESULTS=()

# json_get <file> <dot.path>: prints the JSON value at the path, or __undefined__.
json_get() {
  node -e '
    const fs = require("fs");
    const doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const value = process.argv[2].split(".").reduce(
      (node, key) => (node == null ? undefined : node[key]), doc);
    process.stdout.write(value === undefined ? "__undefined__" : JSON.stringify(value));
  ' "$1" "$2"
}

# check <name> <expected-json> <actual-json>: records and prints PASS or FAIL.
check() {
  if [ "$3" = "$2" ]; then
    echo "PASS $1: $3"
    RESULTS+=("PASS")
  else
    echo "FAIL $1: expected $2, got $3"
    RESULTS+=("FAIL")
  fi
}

# not_run <name> <reason>: records a check that was not made.
not_run() {
  echo "NOT-RUN $1: $2"
  RESULTS+=("NOT-RUN")
}

summary() {
  local pass=0 fail=0 skip=0 r
  for r in "${RESULTS[@]}"; do
    case "$r" in
      PASS) pass=$((pass + 1)) ;;
      FAIL) fail=$((fail + 1)) ;;
      *) skip=$((skip + 1)) ;;
    esac
  done
  echo "SUMMARY pass=$pass fail=$fail not_run=$skip"
  [ "$fail" -eq 0 ] && [ "$skip" -eq 0 ]
}

self_test() {
  local dir good_limits good_job short_job
  dir="$(mktemp -d)"
  good_limits="$dir/limits.json"
  good_job="$dir/job-good.json"
  short_job="$dir/job-short.json"
  printf '%s' '{"data":{"jobId":"job_selftest","limits":{"max_pages":{"requested":null,"effective":7,"clamped_by":["default"]},"max_depth":{"requested":null,"effective":5,"clamped_by":["default"]}}}}' > "$good_limits"
  printf '%s' '{"data":{"params":{"max_pages":7},"statistics":{"pages_visited":7}}}' > "$good_job"
  printf '%s' '{"data":{"params":{"max_pages":7},"statistics":{"pages_visited":6}}}' > "$short_job"

  # The self-test calls the real check() and json_get(), with the same paths and
  # expected values that run_recipe uses, and asserts the verdict that check()
  # records and prints. A check() that passes everything, a check() that refuses
  # the good values, or a json_get that reads the wrong path is reported here
  # before any stack is involved. The comparison is not copied: check() decides,
  # and self_check only compares that verdict with the wanted one. RESULTS is
  # reset here because this process runs no recipe.
  local failures=0 line printed verdict before after
  RESULTS=()
  self_check() { # self_check <want: PASS|FAIL> <name> <expected-json> <actual-json>
    before=${#RESULTS[@]}
    check "$2" "$3" "$4" > "$dir/check.out"
    after=${#RESULTS[@]}
    line=$(head -n 1 "$dir/check.out")
    printed=${line%% *}
    verdict=none
    if [ $((after - before)) -eq 1 ]; then verdict="${RESULTS[$((after - 1))]}"; fi
    if [ "$verdict" = "$1" ] && [ "$printed" = "$1" ]; then
      echo "self-test ok: $line"
    else
      echo "self-test MISMATCH $2: recorded $verdict, printed $printed, expected $1"
      failures=$((failures + 1))
    fi
  }
  self_check PASS limits_max_pages_effective 7 "$(json_get "$good_limits" data.limits.max_pages.effective)"
  self_check PASS limits_max_pages_clamped_by '["default"]' "$(json_get "$good_limits" data.limits.max_pages.clamped_by)"
  self_check PASS control_params_max_pages 7 "$(json_get "$good_job" data.params.max_pages)"
  self_check PASS statistics_pages_visited 7 "$(json_get "$good_job" data.statistics.pages_visited)"
  self_check FAIL statistics_pages_visited_short_job 7 "$(json_get "$short_job" data.statistics.pages_visited)"
  self_check FAIL limits_max_pages_missing 7 "$(json_get "$good_limits" data.limits.max_pages.missing)"
  rm -rf "$dir"
  if [ "$failures" -eq 0 ]; then
    echo "SELF-TEST OK: check() passes the four good values, refuses the short job and a missing value"
    return 0
  fi
  echo "SELF-TEST FAILED: $failures unexpected verdicts"
  return 1
}

run_recipe() {
  local missing=0 name
  for name in GATEWAY_URL CONTROL_URL ORG_ID SESSION_COOKIE_FILE SEED_URL; do
    if [ -z "${!name:-}" ]; then
      echo "missing input: $name (see the header of this script)" >&2
      missing=1
    fi
  done
  [ "$missing" -eq 0 ] || return 2
  if [ ! -r "$SESSION_COOKIE_FILE" ]; then
    echo "SESSION_COOKIE_FILE is not readable" >&2
    return 2
  fi

  local dir="${EVIDENCE_DIR:-$(mktemp -d)}" http_code job_id deadline stats
  mkdir -p "$dir"
  echo "transcripts: $dir"

  # 1. The gateway starts the crawl with no max_pages, so the edge default applies.
  http_code=$(curl -sS ${CURL_OPTS:-} -o "$dir/crawl-response.json" -w '%{http_code}' \
    -X POST "$GATEWAY_URL/api/v1/knowledge/crawl" \
    -H 'Content-Type: application/json' \
    -b "$SESSION_COOKIE_FILE" \
    --data "{\"url\":\"$SEED_URL\"}") || http_code="curl-failed"
  check gateway_crawl_http_status 200 "$http_code"
  check limits_max_pages_effective 7 "$(json_get "$dir/crawl-response.json" data.limits.max_pages.effective)"
  check limits_max_pages_clamped_by '["default"]' "$(json_get "$dir/crawl-response.json" data.limits.max_pages.clamped_by)"
  job_id=$(json_get "$dir/crawl-response.json" data.jobId 2>/dev/null | tr -d '"')
  if [ -z "$job_id" ] || [ "$job_id" = "__undefined__" ]; then
    not_run control_job_record "no jobId in the gateway response"
    summary
    return $?
  fi
  echo "job: $job_id"

  # 2. Wait for the job record to carry statistics (absent while the job runs).
  deadline=$(( $(date +%s) + ${POLL_SECONDS:-900} ))
  while :; do
    curl -sS ${CURL_OPTS:-} -o "$dir/job.json" \
      "$CONTROL_URL/v1/jobs/$job_id?org_id=$ORG_ID" >/dev/null 2>&1 || true
    stats=$(json_get "$dir/job.json" data.statistics 2>/dev/null)
    if [ -n "$stats" ] && [ "$stats" != "null" ] && [ "$stats" != "__undefined__" ]; then
      break
    fi
    if [ "$(date +%s)" -ge "$deadline" ]; then
      break
    fi
    sleep 5
  done
  if [ -z "${stats:-}" ] || [ "$stats" = "null" ] || [ "$stats" = "__undefined__" ]; then
    not_run control_statistics "the job has no statistics after ${POLL_SECONDS:-900} s; see $dir/job.json"
  fi
  check control_params_max_pages 7 "$(json_get "$dir/job.json" data.params.max_pages)"
  check statistics_pages_visited 7 "$(json_get "$dir/job.json" data.statistics.pages_visited)"
  summary
}

case "${1:-}" in
  --self-test) self_test ;;
  "") run_recipe ;;
  *) echo "usage: $0 [--self-test]" >&2; exit 2 ;;
esac
