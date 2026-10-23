#!/usr/bin/env bash
# crawl-scope-robots.sh: real-stack harness for the crawl scope and robots work
# (plan WS14.S1). It crawls the hermetic fixture site and asserts what the two
# fixture hosts received, read from their nginx access logs.
#
# Status: BUILT, NOT RUN. The build of WS14.S1 does not bring up the stack and
# does not run this script. The first run is the WS14.S1 evidence step, on a
# rebuilt root stack with the crawl-fixture overlay. Every assertion below is a
# planned expectation until that run confirms it.
#
# Fixture: apps/Ingestion Plane/docker-compose.crawl-fixture.yml with the content
# in Quarry-v2/deploy/compose/crawl-fixture/{a,b}. Hosts:
#   fixture-a.crawl.test       index, /docs/a, /blog/b, /private/secret, robots.txt
#                              (Disallow: /private/, Crawl-delay: 2)
#   docs.fixture-a.crawl.test  same content as fixture-a
#   fixture-b.crawl.test       /ext only (the external host the index links to)
#
# Assertion sets, chosen with --expect:
#   legacy   today's behaviour, no crawl scope and no robots. Expected: the
#            crawler follows the external link (GET /ext on fixture-b) and never
#            asks for robots.txt on fixture-a. This is the baseline run.
#   observe  QUARRY_EDGE__CRAWL_SCOPE_MODE=observe and robots observe-only.
#            Expected: robots.txt is fetched on fixture-a and /private/secret is
#            still fetched (observe reports, it does not block).
#   enforce  scope and robots enforced. Expected: robots.txt is fetched, no
#            /private/secret and no /ext are fetched, and consecutive page
#            fetches on fixture-a are at least 2 s apart (Crawl-delay: 2).
# Only legacy is expected to pass first. observe and enforce wait for the slices
# that build them (WS14.S2 to S4).
#
# Inputs come from the environment. The dev bearer is written to a private header
# file and is never printed or put on a command line.
#   DEV_BEARER           bearer for POST /v1/crawl (needs ENVIRONMENT=dev on the
#                        edge; the dev bypass is refused in production)  [required
#                        for a run, not for --guard or --self-test]
# Optional:
#   EDGE_URL             default http://127.0.0.1:8082 (published on loopback)
#   CONTROL_URL          default http://127.0.0.1:28081 (quarry-control, loopback)
#   ORG_ID               default org_placeholder (the dev-bypass stub org)  [UNVERIFIED]
#   SEED_URL             default http://fixture-a.crawl.test/
#   MAX_PAGES            default 20
#   POLL_SECONDS         default 300 (wait for the control job to carry statistics)
#   EVIDENCE_DIR         default a new temp dir; logs and JSON are kept there
#   FIXTURE_A_CONTAINER  default crawl-fixture-a
#   FIXTURE_B_CONTAINER  default crawl-fixture-b
#
# Usage (run from anywhere):
#   crawl-scope-robots.sh --self-test          # parser and check() on canned data, no network
#   crawl-scope-robots.sh --guard              # CI grep guard: no production file names the overlay
#   crawl-scope-robots.sh --bring-up --expect legacy   # up the stack with the overlay, then run
#   crawl-scope-robots.sh --expect legacy      # stack already up
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# scripts/e2e -> Quarry-v2 -> Ingestion Plane (the root stack's directory).
INGESTION_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"

EXPECT="legacy"
BRING_UP=0
RESULTS=()

EDGE_URL="${EDGE_URL:-http://127.0.0.1:8082}"
CONTROL_URL="${CONTROL_URL:-http://127.0.0.1:28081}"
ORG_ID="${ORG_ID:-org_placeholder}"
SEED_URL="${SEED_URL:-http://fixture-a.crawl.test/}"
MAX_PAGES="${MAX_PAGES:-20}"
POLL_SECONDS="${POLL_SECONDS:-300}"
FIXTURE_A_CONTAINER="${FIXTURE_A_CONTAINER:-crawl-fixture-a}"
FIXTURE_B_CONTAINER="${FIXTURE_B_CONTAINER:-crawl-fixture-b}"

# check <name> <expected> <actual>: records and prints PASS or FAIL.
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
  echo "SUMMARY expect=$EXPECT pass=$pass fail=$fail not_run=$skip"
  [ "$fail" -eq 0 ] && [ "$skip" -eq 0 ]
}

# access_lines <logfile> <method-and-path>: how many access-log lines carry
# that request line, e.g. "GET /ext". The path match is exact (the next
# character after the path is a space).
access_lines() {
  local n
  n=$(grep -c -F "\"$2 " "$1" 2>/dev/null)
  echo "${n:-0}"
}

# page_timestamps <logfile>: the msec timestamps of page fetches (not robots.txt),
# one per line, in log order.
page_timestamps() {
  awk '$3 == "\"GET" && $4 !~ /^\/robots\.txt/ { print $1 }' "$1"
}

# min_gap_seconds <logfile>: the smallest gap between consecutive page fetches,
# or "none" when there are fewer than two.
min_gap_seconds() {
  page_timestamps "$1" | awk 'NR > 1 { gap = $1 - prev; if (min == "" || gap < min) min = gap } { prev = $1 } END { if (min == "") print "none"; else printf "%.3f\n", min }'
}

# json_field <file> <dot.path> [<path> ...]: the first JSON value found at one of
# the candidate paths (e.g. data.job_id or data.jobId), or __undefined__.
json_field() {
  local file="$1" path
  shift
  node -e '
    const fs = require("fs");
    let doc;
    try { doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); }
    catch (e) { process.stdout.write("__undefined__"); process.exit(0); }
    for (const p of process.argv.slice(2)) {
      const v = p.split(".").reduce((n, k) => (n == null ? undefined : n[k]), doc);
      if (v !== undefined && v !== null) { process.stdout.write(typeof v === "string" ? v : JSON.stringify(v)); process.exit(0); }
    }
    process.stdout.write("__undefined__");
  ' "$file" "$@"
}

# guard: no production compose file may name the fixture overlay.
guard() {
  local files=(
    "$INGESTION_DIR/docker-compose.yml"
    "$INGESTION_DIR/docker-compose.production.yml"
    "$INGESTION_DIR/Quarry-v2/deploy/compose/docker-compose.yml"
  ) f rc
  for f in "${files[@]}"; do
    if [ ! -f "$f" ]; then
      echo "GUARD ERROR: missing $f" >&2
      return 2
    fi
  done
  grep -l -F "crawl-fixture" "${files[@]}" >/dev/null 2>&1
  rc=$?
  case "$rc" in
    0) echo "GUARD FAIL: a production compose file names crawl-fixture"; return 1 ;;
    1) echo "GUARD PASS: no production compose file names crawl-fixture"; return 0 ;;
    *) echo "GUARD ERROR: grep exit $rc" >&2; return 2 ;;
  esac
}

self_test() {
  local dir failures=0 out
  dir="$(mktemp -d)"
  cat > "$dir/a.log" <<'EOF'
1700000000.000 172.18.0.9 "GET /robots.txt HTTP/1.1" 200 60 "ua"
1700000001.000 172.18.0.9 "GET /docs/a HTTP/1.1" 200 120 "ua"
1700000003.500 172.18.0.9 "GET /blog/b HTTP/1.1" 200 120 "ua"
1700000004.000 172.18.0.9 "GET /private/secret HTTP/1.1" 200 80 "ua"
EOF
  cat > "$dir/b.log" <<'EOF'
1700000002.000 172.18.0.9 "GET /ext HTTP/1.1" 200 90 "ua"
EOF
  printf '%s' '{"data":{"job_id":"job_selftest"}}' > "$dir/ack.json"
  printf '%s' '{"data":{"jobId":"job_selftest_camel"}}' > "$dir/ack2.json"
  printf '%s' '{"data":{"statistics":{"pages_visited":3}}}' > "$dir/job.json"

  local got
  got=$(access_lines "$dir/b.log" "GET /ext"); check selftest_ext_count 1 "$got"
  got=$(access_lines "$dir/a.log" "GET /robots.txt"); check selftest_robots_count 1 "$got"
  got=$(access_lines "$dir/a.log" "GET /private"); check selftest_prefix_is_not_a_match 0 "$got"
  got=$(min_gap_seconds "$dir/a.log"); check selftest_min_gap 0.500 "$got"
  got=$(json_field "$dir/ack.json" data.job_id data.jobId); check selftest_job_id_snake job_selftest "$got"
  got=$(json_field "$dir/ack2.json" data.job_id data.jobId); check selftest_job_id_camel job_selftest_camel "$got"
  got=$(json_field "$dir/job.json" data.statistics); check selftest_statistics_present '{"pages_visited":3}' "$got"
  got=$(json_field "$dir/missing.json" data.job_id); check selftest_missing_file __undefined__ "$got"
  rm -rf "$dir"
  for r in "${RESULTS[@]}"; do [ "$r" = "FAIL" ] && failures=$((failures + 1)); done
  out="SELF-TEST: $failures failing check(s)"
  echo "$out"
  [ "$failures" -eq 0 ]
}

wait_for_edge() {
  local i
  for i in $(seq 1 60); do
    if curl -fsS -o /dev/null "$EDGE_URL/health" 2>/dev/null; then return 0; fi
    sleep 2
  done
  return 1
}

bring_up() {
  echo "bring-up: root stack with the crawl-fixture overlay (env-file chain as in the deploy recipe)"
  (cd "$INGESTION_DIR" && docker compose --env-file ../../.env --env-file .env \
    -f docker-compose.yml -f docker-compose.crawl-fixture.yml up -d --wait --wait-timeout 300)
}

run_recipe() {
  if [ -z "${DEV_BEARER:-}" ]; then
    echo "missing input: DEV_BEARER (see the header of this script)" >&2
    return 2
  fi
  if ! command -v docker >/dev/null 2>&1; then
    echo "docker is not on PATH" >&2
    return 2
  fi
  [ "$BRING_UP" -eq 1 ] && { bring_up || return 2; }

  local dir="${EVIDENCE_DIR:-$(mktemp -d)}" header start http_code job_id stats deadline
  mkdir -p "$dir"
  echo "evidence: $dir"
  header="$dir/.auth-header"
  ( umask 077; printf 'Authorization: Bearer %s\n' "$DEV_BEARER" > "$header" )

  if ! wait_for_edge; then
    not_run edge_health "quarry-edge did not answer $EDGE_URL/health in 120 s"
    summary
    return $?
  fi
  echo "edge health: ok"

  # Only log lines written after this point belong to this run.
  start=$(date -u -d '-2 seconds' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)

  http_code=$(curl -sS -o "$dir/crawl-response.json" -w '%{http_code}' \
    -X POST "$EDGE_URL/v1/crawl" \
    -H 'Content-Type: application/json' \
    -H "@$header" \
    --data "{\"url\":\"$SEED_URL\",\"max_pages\":$MAX_PAGES}") || http_code="curl-failed"
  case "$http_code" in
    2??) echo "PASS crawl_accepted: HTTP $http_code"; RESULTS+=("PASS") ;;
    *) check crawl_accepted accepted "$http_code" ;;
  esac

  job_id=$(json_field "$dir/crawl-response.json" data.job_id data.jobId | tr -d '"')
  if [ -z "$job_id" ] || [ "$job_id" = "__undefined__" ]; then
    not_run control_job "no job id in $dir/crawl-response.json"
    docker logs --since "$start" "$FIXTURE_A_CONTAINER" > "$dir/fixture-a.log" 2>&1
    docker logs --since "$start" "$FIXTURE_B_CONTAINER" > "$dir/fixture-b.log" 2>&1
    summary
    return $?
  fi
  echo "job: $job_id"

  # The control record carries statistics once the job has finished.
  deadline=$(( $(date +%s) + POLL_SECONDS ))
  while :; do
    curl -sS -o "$dir/job.json" "$CONTROL_URL/v1/jobs/$job_id?org_id=$ORG_ID" >/dev/null 2>&1 || true
    stats=$(json_field "$dir/job.json" data.statistics)
    if [ "$stats" != "__undefined__" ] && [ "$stats" != "null" ]; then break; fi
    [ "$(date +%s)" -ge "$deadline" ] && break
    sleep 5
  done
  if [ "$stats" = "__undefined__" ] || [ "$stats" = "null" ]; then
    not_run crawl_finished "no statistics after ${POLL_SECONDS} s; see $dir/job.json"
  else
    echo "PASS crawl_finished: statistics present"; RESULTS+=("PASS")
  fi

  docker logs --since "$start" "$FIXTURE_A_CONTAINER" > "$dir/fixture-a.log" 2>&1
  docker logs --since "$start" "$FIXTURE_B_CONTAINER" > "$dir/fixture-b.log" 2>&1
  rm -f "$header"

  local a="$dir/fixture-a.log" b="$dir/fixture-b.log"
  echo "pages fetched per fixture host (robots.txt excluded):"
  echo "  $FIXTURE_A_CONTAINER: $(page_timestamps "$a" | wc -l | tr -d ' ')"
  echo "  $FIXTURE_B_CONTAINER: $(page_timestamps "$b" | wc -l | tr -d ' ')"

  case "$EXPECT" in
    legacy)
      check legacy_external_followed 1 "$([ "$(access_lines "$b" "GET /ext")" -ge 1 ] && echo 1 || echo 0)"
      check legacy_no_robots_fetch 0 "$(access_lines "$a" "GET /robots.txt")"
      ;;
    observe)
      check observe_robots_fetched 1 "$([ "$(access_lines "$a" "GET /robots.txt")" -ge 1 ] && echo 1 || echo 0)"
      check observe_private_still_fetched 1 "$([ "$(access_lines "$a" "GET /private/secret")" -ge 1 ] && echo 1 || echo 0)"
      ;;
    enforce)
      check enforce_robots_fetched 1 "$([ "$(access_lines "$a" "GET /robots.txt")" -ge 1 ] && echo 1 || echo 0)"
      check enforce_private_not_fetched 0 "$(access_lines "$a" "GET /private/secret")"
      check enforce_external_not_fetched 0 "$(access_lines "$b" "GET /ext")"
      check enforce_crawl_delay_at_least_2s ok "$(awk -v g="$(min_gap_seconds "$a")" 'BEGIN { if (g == "none" || g + 0 >= 2) print "ok"; else print "too-fast " g }')"
      ;;
  esac
  echo "evidence files: $dir (fixture-a.log, fixture-b.log, job.json, crawl-response.json)"
  summary
}

# Argument parsing.
while [ $# -gt 0 ]; do
  case "$1" in
    --self-test) self_test; exit $? ;;
    --guard) guard; exit $? ;;
    --bring-up) BRING_UP=1; shift ;;
    --expect)
      EXPECT="${2:-}"
      case "$EXPECT" in
        legacy|observe|enforce) ;;
        *) echo "usage: --expect legacy|observe|enforce" >&2; exit 2 ;;
      esac
      shift 2
      ;;
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    *) echo "usage: $0 [--self-test|--guard] [--bring-up] [--expect legacy|observe|enforce]" >&2; exit 2 ;;
  esac
done

run_recipe
exit $?
