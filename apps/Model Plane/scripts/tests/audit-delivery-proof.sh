#!/usr/bin/env bash
set -uo pipefail

# Read-only proof that the Model Plane audit trail reaches audit-core.
#
#   scripts/tests/audit-delivery-proof.sh [--stage baseline|canary|final]
#
# It reads, never writes: SELECT statements run with default_transaction_read_only=on, one
# NATS monitoring GET, one metrics scrape, `docker inspect`. Output is counts, names, subjects,
# timestamps and one boolean per row (details.zdr): no payload, no tenant identifier, no
# credential. Each block names the command that produced it, so the output can be pasted into
# an evidence file such as docs/compliance/evidence/live-reads-audit-delivery-<date>.txt
# (precedent: the 2026-10-06 file). The replay itself is a separate operator step:
# docs/runbooks/audit-delivery-replay.md.
#
# Stages decide which CHECK lines can fail (exit 1):
#   baseline  read only, no verdict (before the publisher fix is deployed, most checks would fail)
#   canary    fix deployed, migration 0043 applied, the first requeued rows delivered
#   final     nothing left terminal or pending, both durables drained, no ZDR row anywhere
# Container names and the metrics URL come from the environment (defaults are the dev stack).

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGE="baseline"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --stage)
      [[ $# -ge 2 ]] || { echo "audit-delivery-proof: --stage needs a value" >&2; exit 2; }
      STAGE="$2"
      shift
      ;;
    -h | --help)
      sed -n '4,5p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "audit-delivery-proof: unknown argument '$1'" >&2; exit 2 ;;
  esac
  shift
done
case "$STAGE" in baseline | canary | final) ;; *) echo "audit-delivery-proof: stage must be baseline, canary or final" >&2; exit 2 ;; esac

MP_POSTGRES="${MP_POSTGRES_CONTAINER:-model-plane-postgres-1}"
MP_DB="${MP_SESSION_CORE_DB:-session_core}"
MP_DB_USER="${MP_POSTGRES_USER:-postgres}"
CP_POSTGRES="${CP_POSTGRES_CONTAINER:-controlplane-postgres}"
CP_DB="${CP_AUDIT_DB:-controlplane}"
CP_DB_USER="${CP_POSTGRES_USER:-coresystem}"
NATS_CONTAINER="${MP_NATS_CONTAINER:-model-plane-nats-1}"
SESSION_CORE_CONTAINER="${MP_SESSION_CORE_CONTAINER:-model-plane-session-core-1}"
METRICS_URL="${SESSION_CORE_METRICS_URL:-http://127.0.0.1:18081/metrics}"
STREAM="VEREVON_CONTROL_OBSERVABILITY"
AUDIT_SUBJECT_LIKE='verevon.audit.%'
FIX_COMMIT="${AUDIT_FIX_COMMIT:-b9ed9197}"

command -v docker >/dev/null 2>&1 || { echo "audit-delivery-proof: docker is not available" >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "audit-delivery-proof: jq is required to read the NATS monitoring output" >&2; exit 2; }

failures=0
checks=0

# check <name> <PASS|FAIL> <detail>: baseline reports the line but never fails.
check() {
  checks=$((checks + 1))
  local name="$1" result="$2" detail="$3"
  if [[ "$STAGE" == "baseline" ]]; then
    echo "CHECK $name INFO ($result at baseline) $detail"
    return
  fi
  echo "CHECK $name $result $detail"
  [[ "$result" == "PASS" ]] || failures=$((failures + 1))
}

psql_ro() { # <container> <user> <db> <sql>
  docker exec -e PGOPTIONS='-c default_transaction_read_only=on' "$1" \
    psql -U "$2" -d "$3" -At -F '|' -v ON_ERROR_STOP=1 -c "$4" 2>&1
}
mp() { psql_ro "$MP_POSTGRES" "$MP_DB_USER" "$MP_DB" "$1"; }
cp_() { psql_ro "$CP_POSTGRES" "$CP_DB_USER" "$CP_DB" "$1"; }

block() { # <title> <command text shown in the evidence>
  echo
  echo "## $1"
  echo "## command: $2"
}

# First field of a single-row result, or ERR when the read failed.
scalar() { # <fn> <sql>
  local out
  if out="$("$1" "$2")"; then printf '%s' "${out%%|*}"; else printf 'ERR'; fi
}

echo "# Audit delivery proof, stage $STAGE. Read-only: SELECT (read-only transaction), docker inspect, the NATS monitoring endpoint, one metrics scrape."
echo "# Counts, names, subjects, timestamps. No payload, no tenant identifier, no credential."
echo "# Read at $(date -u +%Y-%m-%dT%H:%M:%SZ)"

block "BLOCK 1: what is deployed" "docker inspect $SESSION_CORE_CONTAINER (started, image id, image revision label and creation time)"
image_id="$(docker inspect --format '{{.Image}}' "$SESSION_CORE_CONTAINER" 2>/dev/null || true)"
revision=""
if [[ -n "$image_id" ]]; then
  started="$(docker inspect --format '{{.State.StartedAt}}' "$SESSION_CORE_CONTAINER" 2>/dev/null || true)"
  revision="$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$image_id" 2>/dev/null || true)"
  created="$(docker image inspect --format '{{.Created}}' "$image_id" 2>/dev/null || true)"
  echo "started=$started image=${image_id:0:19} revision=${revision:-unknown} created=$created"
else
  echo "container $SESSION_CORE_CONTAINER not found"
fi
fix_in_image="unknown"
if [[ -n "$revision" && "$revision" != "<no value>" ]] && git -C "$ROOT_DIR" cat-file -e "$revision^{commit}" 2>/dev/null &&
  git -C "$ROOT_DIR" cat-file -e "$FIX_COMMIT^{commit}" 2>/dev/null; then
  if git -C "$ROOT_DIR" merge-base --is-ancestor "$FIX_COMMIT" "$revision" 2>/dev/null; then fix_in_image="yes"; else fix_in_image="no"; fi
fi
echo "publisher fix $FIX_COMMIT is an ancestor of the running revision: $fix_in_image"
if [[ "$fix_in_image" == "yes" ]]; then
  check "publisher-fix-deployed" PASS "revision ${revision:0:8}"
else
  check "publisher-fix-deployed" FAIL "ancestor test says $fix_in_image for revision ${revision:-unknown}"
fi

block "BLOCK 2: migration 0043 and the requeue function" "select version, success from _sqlx_migrations where version = 43;  select count(*) from pg_proc where proname = 'requeue_session_audit_terminal'"
mp "select version, success from _sqlx_migrations where version = 43"
migration_ok="$(scalar mp "select count(*) from _sqlx_migrations where version = 43 and success")"
function_ok="$(scalar mp "select count(*) from pg_proc where proname = 'requeue_session_audit_terminal'")"
echo "function rows: $function_ok"
if [[ "$migration_ok" == "1" && "$function_ok" == "1" ]]; then
  check "migration-0043-applied" PASS "migration row and function present"
else
  check "migration-0043-applied" FAIL "migration rows=$migration_ok function rows=$function_ok"
fi

block "BLOCK 3: the outbox by event (published, pending, terminal and unpublished, most attempts)" "select payload->>'event', count(*) filter (published), count(*) filter (neither), count(*) filter (terminal), max(attempts) from session_audit_outbox where subject like '$AUDIT_SUBJECT_LIKE' group by 1"
mp "select coalesce(payload->>'event', '(none)'),
           count(*) filter (where published_at is not null),
           count(*) filter (where published_at is null and terminal_at is null),
           count(*) filter (where published_at is null and terminal_at is not null),
           max(attempts)
    from session_audit_outbox where subject like '$AUDIT_SUBJECT_LIKE' group by 1 order by 1"
block "BLOCK 3b: why the terminal rows stopped" "select last_error, count(*) from session_audit_outbox where terminal_at is not null and published_at is null and subject like '$AUDIT_SUBJECT_LIKE' group by 1"
mp "select coalesce(last_error, '(null)'), count(*) from session_audit_outbox
    where terminal_at is not null and published_at is null and subject like '$AUDIT_SUBJECT_LIKE' group by 1 order by 1"
terminal_unpublished="$(scalar mp "select count(*) from session_audit_outbox where terminal_at is not null and published_at is null and subject like '$AUDIT_SUBJECT_LIKE'")"
pending_rows="$(scalar mp "select count(*) from session_audit_outbox where terminal_at is null and published_at is null and subject like '$AUDIT_SUBJECT_LIKE'")"
echo "terminal and unpublished: $terminal_unpublished   pending: $pending_rows"

block "BLOCK 4: rows put back by the requeue function" "select count(*), published, terminal, max(requeue_count) from session_audit_outbox where requeued_at is not null"
requeue_line="$(mp "select count(*), count(*) filter (where published_at is not null), count(*) filter (where terminal_at is not null), coalesce(max(requeue_count), 0) from session_audit_outbox where requeued_at is not null")"
echo "$requeue_line"
if [[ "$requeue_line" =~ ^[0-9]+\|[0-9]+\|[0-9]+\|[0-9]+$ ]]; then
  IFS='|' read -r rq_total rq_published rq_terminal _ <<<"$requeue_line"
  if [[ "$rq_total" -gt 0 && "$rq_terminal" -eq 0 && "$rq_published" -eq "$rq_total" ]]; then
    check "requeued-rows-delivered" PASS "$rq_published of $rq_total published, $rq_terminal terminal"
  else
    check "requeued-rows-delivered" FAIL "requeued=$rq_total published=$rq_published terminal=$rq_terminal (wait about 15 s and run again)"
  fi
else
  check "requeued-rows-delivered" FAIL "the read failed (migration 0043 columns missing?)"
fi

block "BLOCK 5: ZDR rows in the outbox (must be 0: a ZDR run writes no audit row)" "select count(*) from session_audit_outbox where subject like '$AUDIT_SUBJECT_LIKE' and payload->'details'->>'zdr' = 'true'"
zdr_outbox="$(scalar mp "select count(*) from session_audit_outbox where subject like '$AUDIT_SUBJECT_LIKE' and payload->'details'->>'zdr' = 'true'")"
echo "$zdr_outbox"
if [[ "$zdr_outbox" == "0" ]]; then check "no-zdr-row-in-outbox" PASS "0 rows"; else check "no-zdr-row-in-outbox" FAIL "count=$zdr_outbox"; fi

block "BLOCK 6: the session-core metrics scrape" "curl -s $METRICS_URL, lines mp_session_audit_outbox* and mp_session_tool_audit* (bounded subject_class and reason labels)"
metrics="$(curl -fsS --max-time 10 "$METRICS_URL" 2>/dev/null | grep -E '^mp_session_(audit_outbox|tool_audit)' | sort || true)"
if [[ -n "$metrics" ]]; then echo "$metrics"; else echo "(no scrape: $METRICS_URL unreachable or no series)"; fi
by_subject="$(printf '%s\n' "$metrics" | grep -E '^mp_session_audit_outbox_terminal_by_subject\{' || true)"
if [[ -n "$by_subject" ]]; then
  terminal_gauge_sum="$(printf '%s\n' "$by_subject" | awk '{s += $NF} END {print s + 0}')"
  if [[ "$terminal_gauge_sum" == "0" ]]; then
    check "terminal-gauge-zero" PASS "sum over subject_class = 0"
  else
    check "terminal-gauge-zero" FAIL "sum over subject_class = $terminal_gauge_sum"
  fi
else
  check "terminal-gauge-zero" FAIL "mp_session_audit_outbox_terminal_by_subject is absent (old build or stopped reporter)"
fi

block "BLOCK 7: the stream and audit-core's durables" "docker exec $NATS_CONTAINER wget -qO- 'http://127.0.0.1:8222/jsz?streams=true&consumers=true&config=true' | jq (stream $STREAM)"
jsz="$(docker exec "$NATS_CONTAINER" wget -qO- 'http://127.0.0.1:8222/jsz?streams=true&consumers=true&config=true' 2>/dev/null || true)"
if [[ -n "$jsz" ]] && printf '%s' "$jsz" | jq -e . >/dev/null 2>&1; then
  printf '%s' "$jsz" | jq -r --arg s "$STREAM" '
    .account_details[]?.stream_detail[]? | select(.name == $s)
    | "STREAM \(.name) messages=\(.state.messages) last_seq=\(.state.last_seq) subjects=\(.config.subjects | join(","))",
      (.consumer_detail[]? | "  CONSUMER \(.name) filter=\(.config.filter_subject // "") delivered=\(.delivered.consumer_seq) num_pending=\(.num_pending) num_ack_pending=\(.num_ack_pending) num_redelivered=\(.num_redelivered) push_bound=\(.push_bound // "n/a")")'
  durables="$(printf '%s' "$jsz" | jq -r --arg s "$STREAM" '
    [.account_details[]?.stream_detail[]? | select(.name == $s) | .consumer_detail[]?
     | select(.name | startswith("audit-core-model-v3-"))]
    | "\(length)|\(map(.num_pending) | add // 0)|\(map(.num_ack_pending) | add // 0)|\(map(.num_redelivered) | add // 0)"')"
  IFS='|' read -r d_count d_pending d_ack d_redelivered <<<"$durables"
  if [[ "${d_count:-0}" -eq 2 && "${d_pending:-1}" -eq 0 && "${d_ack:-1}" -eq 0 && "${d_redelivered:-1}" -eq 0 ]]; then
    check "durables-drained" PASS "2 durables, pending 0, ack_pending 0, redelivered 0"
  else
    check "durables-drained" FAIL "durables=${d_count:-0} pending=${d_pending:-?} ack_pending=${d_ack:-?} redelivered=${d_redelivered:-?}"
  fi
else
  echo "(the monitoring endpoint of $NATS_CONTAINER did not answer)"
  check "durables-drained" FAIL "no jsz answer"
fi

block "BLOCK 8: what audit-core holds for plane model" "select event, count(*), min(occurred_at), max(occurred_at) from audit_events where plane = 'model' group by 1  (controlplane database)"
cp_ "select event, count(*), min(occurred_at), max(occurred_at) from audit_events where plane = 'model' group by 1 order by 1"
block "BLOCK 8b: the de-duplication a replay relies on" "select indexname from pg_indexes where tablename = 'audit_events' and indexname like 'uq_%'"
cp_ "select indexname from pg_indexes where tablename = 'audit_events' and indexname like 'uq_%' order by 1"
tool_action_rows="$(scalar cp_ "select count(*) from audit_events where plane = 'model' and event = 'tool_action'")"
zdr_audit="$(scalar cp_ "select count(*) from audit_events where plane = 'model' and details->>'zdr' = 'true'")"
block "BLOCK 9: ZDR rows in audit-core (must be 0)" "select count(*) from audit_events where plane = 'model' and details->>'zdr' = 'true'"
echo "$zdr_audit"
if [[ "$zdr_audit" == "0" ]]; then check "no-zdr-row-in-audit-core" PASS "0 rows"; else check "no-zdr-row-in-audit-core" FAIL "count=$zdr_audit"; fi
if [[ "$tool_action_rows" =~ ^[0-9]+$ && "$tool_action_rows" -ge 1 ]]; then
  check "tool-action-in-audit-core" PASS "$tool_action_rows rows"
else
  check "tool-action-in-audit-core" FAIL "count=$tool_action_rows"
fi

# Verdicts that only the final stage owns: the canary legitimately still has rows left.
if [[ "$STAGE" == "final" ]]; then
  [[ "$terminal_unpublished" == "0" ]] && check "no-terminal-unpublished" PASS "0 rows" || check "no-terminal-unpublished" FAIL "count=$terminal_unpublished"
  [[ "$pending_rows" == "0" ]] && check "no-pending-rows" PASS "0 rows" || check "no-pending-rows" FAIL "count=$pending_rows"
fi

echo
echo "## MANUAL (needs a user session, not read here): one tool turn adds tool_action_reserved and tool_action to audit_events within seconds; GET /api/v1/audit?event=tool_action through the gateway returns rows."
echo "## stage=$STAGE checks=$checks failures=$failures"
if [[ "$STAGE" != "baseline" && "$failures" -gt 0 ]]; then
  exit 1
fi
exit 0
