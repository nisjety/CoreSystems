#!/usr/bin/env bash
# Volume soak for the run trace (WS10.S9, plan 2019-2041; owner decision D15: stack-session work,
# written in the build wave and NOT run there).
#
# What it proves, against a side-by-side Model Plane + Quarry stack (never the live one):
# 50 concurrent fake runs x 40 steps on Quarry's stub driver, NATS up, with ONE injected 5 s NATS
# outage, then these assertions about what reached session-core's run_trace_events table:
#
#   1. producer_seq is dense per run except the gaps Quarry itself logged as dropped
#   2. no duplicate rows (the unique constraints make a duplicate impossible; this checks that the
#      count of distinct event ids equals the row count, so a constraint dropped by accident shows)
#   3. rows = published - dropped
#   4. consumer lag p99 (received_at - occurred_at) is under 2 s
#   5. Quarry step latency p99 with the publisher ON is within 5 percent of the publisher OFF
#   6. recorded, not asserted: session-core CPU % and the Postgres write rate on the table
#
# The Quarry half is `quarry-trace-soak` (apps/Ingestion Plane/Quarry-v2/lab/evals, Quarry-owned,
# NOT written yet). Its command-line contract, which this script depends on:
#
#   cargo run -p quarry-evals --bin quarry-trace-soak -- \
#       --runs 50 --steps 40 --publisher on|off --run-prefix soak- --org-id <org> \
#       --nats-url <url> --summary-out <file.json> [--outage-marker <file>]
#
# It publishes through the real trace publisher (mp.v1.trace.run.<run_id>, explicit "zdr": false,
# dense producer_seq from 1) for run ids <prefix>0 .. <prefix>49, and writes a JSON summary:
#   { "published": N, "dropped": N, "step_latency_p99_ms": F,
#     "runs": { "<run_id>": { "max_seq": N, "dropped": N } } }
# `dropped` counts events the publisher gave up on (its logged gaps). The binary touches the file
# named by --outage-marker when it is mid-run, which is this script's cue to start the outage.
#
# This script seeds the 50 Model Plane run rows the consumer needs (a run row must exist for the
# insert to select from `runs`) in the database it is pointed at, and deletes them (and, by
# cascade, their trace rows) at the end. Every dangerous input is explicit and has no default.
#
#   SOAK_PG_CONTAINER   docker container running the session-core Postgres        (required)
#   SOAK_PG_USER        database role                                              (required)
#   SOAK_PG_DB          database name; must contain "soak" or "test"               (required)
#   SOAK_NATS_CONTAINER container of the SIDE-BY-SIDE Model Plane broker to pause  (required)
#   SOAK_SC_CONTAINER   session-core container, for the CPU sample                 (optional)
#   SOAK_NATS_URL       broker URL for the publisher                               (required)
#   SOAK_ORG_ID         org id the fake runs belong to        (default soak-org-<epoch>)
#   SOAK_RUNS           default 50    SOAK_STEPS  default 40    SOAK_OUTAGE_SECS  default 5
#   SOAK_ALLOW_PAUSE=1  you confirm pausing SOAK_NATS_CONTAINER pauses every client of it
#
# No credential is read or printed here: the Quarry binary takes its NATS password from its own
# environment (MODEL_QUARRY_TRACE_NATS_PASSWORD), which this script never touches.
#
#   scripts/tests/run-trace-soak.sh --list      print the steps and assertions, run nothing
#   scripts/tests/run-trace-soak.sh --dry-run   validate the inputs, print the plan, run nothing
#   scripts/tests/run-trace-soak.sh             run the soak
#
# Status: written against the table and the consumer as built; the Quarry binary does not exist,
# so this has NEVER been run, not even partially.
set -Eeuo pipefail

RUNS="${SOAK_RUNS:-50}"
STEPS="${SOAK_STEPS:-40}"
OUTAGE_SECS="${SOAK_OUTAGE_SECS:-5}"
RUN_PREFIX="soak-"
EXPECTED_LAG_P99_SECS="2"
LATENCY_TOLERANCE_PERCENT="5"

STEPS_LIST=(
  "seed ${RUNS} Model Plane run rows (${RUN_PREFIX}0..)"
  "baseline: quarry-trace-soak --publisher off  (step latency p99 without the publisher)"
  "soak: quarry-trace-soak --publisher on, ${OUTAGE_SECS} s NATS outage injected mid-run"
  "assert 1: producer_seq dense per run except the logged drops"
  "assert 2: zero duplicate rows"
  "assert 3: rows = published - dropped"
  "assert 4: consumer lag p99 < ${EXPECTED_LAG_P99_SECS} s"
  "assert 5: step latency p99 within ${LATENCY_TOLERANCE_PERCENT} percent of the baseline"
  "record: session-core CPU and the table's insert rate"
  "cleanup: delete the ${RUN_PREFIX}* run rows and their threads"
)

if [[ "${1:-}" == "--list" ]]; then
  printf '%s\n' "${STEPS_LIST[@]}"
  exit 0
fi

need() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "${name} must be set (no default: this script writes a database and pauses a broker)" >&2
    return 1
  fi
}

missing=0
for var in SOAK_PG_CONTAINER SOAK_PG_USER SOAK_PG_DB SOAK_NATS_CONTAINER SOAK_NATS_URL; do
  need "$var" || missing=1
done
[[ "$missing" == 0 ]] || exit 2

if [[ ! "$SOAK_PG_DB" =~ (soak|test) ]]; then
  echo "SOAK_PG_DB must contain \"soak\" or \"test\": refusing to seed runs into ${SOAK_PG_DB}" >&2
  exit 2
fi
if [[ "${SOAK_ALLOW_PAUSE:-}" != "1" ]]; then
  echo "set SOAK_ALLOW_PAUSE=1 to confirm that pausing ${SOAK_NATS_CONTAINER} is acceptable" >&2
  exit 2
fi
for number in "$RUNS" "$STEPS" "$OUTAGE_SECS"; do
  [[ "$number" =~ ^[1-9][0-9]*$ ]] || { echo "SOAK_RUNS, SOAK_STEPS and SOAK_OUTAGE_SECS must be positive integers" >&2; exit 2; }
done
ORG_ID="${SOAK_ORG_ID:-soak-org-$(date +%s)}"
[[ "$ORG_ID" =~ ^[A-Za-z0-9_-]+$ ]] || { echo "SOAK_ORG_ID must be [A-Za-z0-9_-]+" >&2; exit 2; }

if [[ "${1:-}" == "--dry-run" ]]; then
  echo "plan (nothing is run):"
  printf '  - %s\n' "${STEPS_LIST[@]}"
  echo "database: ${SOAK_PG_DB} in container ${SOAK_PG_CONTAINER}; org ${ORG_ID}; broker container ${SOAK_NATS_CONTAINER}"
  exit 0
fi

for tool in docker cargo; do
  command -v "$tool" >/dev/null 2>&1 || { echo "${tool} is required" >&2; exit 2; }
done

WORK="$(mktemp -d)"
OUTAGE_MARKER="${WORK}/outage-now"
BASELINE_JSON="${WORK}/baseline.json"
SOAK_JSON="${WORK}/soak.json"
PIDS=()

psql_q() {
  docker exec -i "$SOAK_PG_CONTAINER" psql -U "$SOAK_PG_USER" -d "$SOAK_PG_DB" -v ON_ERROR_STOP=1 -tA "$@"
}

cleanup() {
  local status=$?
  # Always unpause the broker, even when something failed mid-outage.
  docker unpause "$SOAK_NATS_CONTAINER" >/dev/null 2>&1 || true
  for pid in "${PIDS[@]:-}"; do kill "$pid" >/dev/null 2>&1 || true; done
  psql_q -c "DELETE FROM runs WHERE id LIKE '${RUN_PREFIX}%' AND org_id = '${ORG_ID}'" >/dev/null 2>&1 || true
  psql_q -c "DELETE FROM threads WHERE id LIKE '${RUN_PREFIX}%' AND org_id = '${ORG_ID}'" >/dev/null 2>&1 || true
  rm -rf "$WORK"
  exit "$status"
}
trap cleanup EXIT

passes=0
failures=0
pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

json_number() { # json_number <file> <jq-free python expression over d>
  python3 -I - "$1" "$2" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(eval(sys.argv[2], {"__builtins__": {"sum": sum, "len": len}}, {"d": d}))
PY
}

# ---- seed ------------------------------------------------------------------------------------
seed_sql="INSERT INTO threads (id, session_key, org_id, user_id)
  SELECT '${RUN_PREFIX}t' || n, '${RUN_PREFIX}t' || n, '${ORG_ID}', 'soak-user' FROM generate_series(0, ${RUNS} - 1) n;
INSERT INTO runs (id, thread_id, goal, org_id, user_id)
  SELECT '${RUN_PREFIX}' || n, '${RUN_PREFIX}t' || n, 'soak', '${ORG_ID}', 'soak-user' FROM generate_series(0, ${RUNS} - 1) n;"
psql_q -c "$seed_sql" >/dev/null
pass "seeded ${RUNS} run rows for ${ORG_ID}"

soak_binary() {
  (cd "apps/Ingestion Plane/Quarry-v2" && cargo run -q -p quarry-evals --bin quarry-trace-soak -- \
    --runs "$RUNS" --steps "$STEPS" --run-prefix "$RUN_PREFIX" --org-id "$ORG_ID" \
    --nats-url "$SOAK_NATS_URL" "$@")
}

# ---- baseline: publisher off -----------------------------------------------------------------
soak_binary --publisher off --summary-out "$BASELINE_JSON"
pass "baseline run finished"

# ---- soak: publisher on, one outage ----------------------------------------------------------
pg_ins_before="$(psql_q -c "SELECT COALESCE(n_tup_ins, 0) FROM pg_stat_user_tables WHERE relname = 'run_trace_events'")"
started_epoch="$(date +%s)"
soak_binary --publisher on --summary-out "$SOAK_JSON" --outage-marker "$OUTAGE_MARKER" &
soak_pid=$!
PIDS+=("$soak_pid")

cpu_sample=""
while kill -0 "$soak_pid" >/dev/null 2>&1; do
  if [[ -e "$OUTAGE_MARKER" ]]; then
    rm -f "$OUTAGE_MARKER"
    docker pause "$SOAK_NATS_CONTAINER" >/dev/null
    sleep "$OUTAGE_SECS"
    docker unpause "$SOAK_NATS_CONTAINER" >/dev/null
    echo "injected one ${OUTAGE_SECS} s outage on ${SOAK_NATS_CONTAINER}"
  fi
  if [[ -n "${SOAK_SC_CONTAINER:-}" ]]; then
    cpu_sample="$(docker stats --no-stream --format '{{.CPUPerc}}' "$SOAK_SC_CONTAINER" 2>/dev/null || true)"
  fi
  sleep 0.2
done
wait "$soak_pid"
ended_epoch="$(date +%s)"
pass "soak run finished"

# The consumer reads from a stream: give it time to drain before counting.
for _ in $(seq 1 60); do
  rows_now="$(psql_q -c "SELECT count(*) FROM run_trace_events WHERE run_id LIKE '${RUN_PREFIX}%' AND org_id = '${ORG_ID}'")"
  sleep 1
  rows_later="$(psql_q -c "SELECT count(*) FROM run_trace_events WHERE run_id LIKE '${RUN_PREFIX}%' AND org_id = '${ORG_ID}'")"
  [[ "$rows_now" == "$rows_later" ]] && break
done

published="$(json_number "$SOAK_JSON" 'd["published"]')"
dropped="$(json_number "$SOAK_JSON" 'd["dropped"]')"
rows="$rows_later"
where="run_id LIKE '${RUN_PREFIX}%' AND org_id = '${ORG_ID}'"

# 1. dense except the logged drops: per run, max_seq - rows must equal that run's reported drops,
#    and the producer's own max_seq must be what the table holds as its top.
runs_seen="$(psql_q -c "SELECT count(*) FROM (SELECT run_id, max(producer_seq) AS top, count(*) AS n FROM run_trace_events WHERE ${where} GROUP BY run_id) s" )"
gaps_total="$(psql_q -c "SELECT COALESCE(sum(top - n), 0) FROM (SELECT max(producer_seq) AS top, count(*) AS n FROM run_trace_events WHERE ${where} GROUP BY run_id) s")"
if [[ "$gaps_total" -le "$dropped" ]]; then
  pass "1. producer_seq gaps (${gaps_total}) are within the logged drops (${dropped}) across ${runs_seen} runs"
else
  fail "1. producer_seq has ${gaps_total} gaps but Quarry logged only ${dropped} drops"
fi

# 2. no duplicates.
distinct_ids="$(psql_q -c "SELECT count(DISTINCT event_id) FROM run_trace_events WHERE ${where}")"
dup_keys="$(psql_q -c "SELECT count(*) FROM (SELECT 1 FROM run_trace_events WHERE ${where} GROUP BY run_id, producer, producer_seq HAVING count(*) > 1) s")"
if [[ "$distinct_ids" == "$rows" && "$dup_keys" == "0" ]]; then
  pass "2. no duplicate rows (${rows} rows, ${distinct_ids} distinct event ids)"
else
  fail "2. duplicates: ${rows} rows, ${distinct_ids} distinct event ids, ${dup_keys} repeated keys"
fi

# 3. rows = published - dropped.
expected=$((published - dropped))
if [[ "$rows" == "$expected" ]]; then
  pass "3. rows (${rows}) = published (${published}) - dropped (${dropped})"
else
  fail "3. rows (${rows}) != published (${published}) - dropped (${dropped}) = ${expected}"
fi

# 4. consumer lag p99.
lag_p99="$(psql_q -c "SELECT COALESCE(round(percentile_cont(0.99) WITHIN GROUP (ORDER BY extract(epoch FROM received_at - occurred_at))::numeric, 3), 0) FROM run_trace_events WHERE ${where}")"
if python3 -I -c "import sys; sys.exit(0 if float(sys.argv[1]) < float(sys.argv[2]) else 1)" "$lag_p99" "$EXPECTED_LAG_P99_SECS"; then
  pass "4. consumer lag p99 ${lag_p99} s < ${EXPECTED_LAG_P99_SECS} s"
else
  fail "4. consumer lag p99 ${lag_p99} s is not under ${EXPECTED_LAG_P99_SECS} s"
fi

# 5. step latency p99 within tolerance of the baseline.
base_p99="$(json_number "$BASELINE_JSON" 'd["step_latency_p99_ms"]')"
on_p99="$(json_number "$SOAK_JSON" 'd["step_latency_p99_ms"]')"
if python3 -I -c "import sys; base, on, tol = map(float, sys.argv[1:4]); sys.exit(0 if on <= base * (1 + tol / 100) else 1)" "$base_p99" "$on_p99" "$LATENCY_TOLERANCE_PERCENT"; then
  pass "5. step latency p99 ${on_p99} ms vs ${base_p99} ms without the publisher (within ${LATENCY_TOLERANCE_PERCENT} percent)"
else
  fail "5. step latency p99 ${on_p99} ms exceeds ${base_p99} ms + ${LATENCY_TOLERANCE_PERCENT} percent"
fi

# 6. recorded, not asserted.
pg_ins_after="$(psql_q -c "SELECT COALESCE(n_tup_ins, 0) FROM pg_stat_user_tables WHERE relname = 'run_trace_events'")"
seconds=$((ended_epoch - started_epoch))
[[ "$seconds" -gt 0 ]] || seconds=1
echo "record - table inserts: $((pg_ins_after - pg_ins_before)) in ${seconds} s ($(( (pg_ins_after - pg_ins_before) / seconds )) per s); session-core CPU last sample: ${cpu_sample:-not sampled}"

printf '\n%d passed, %d failed\n' "$passes" "$failures"
[[ "$failures" == 0 ]]
