#!/usr/bin/env bash
set -uo pipefail

# Read-only proof that a Zero Data Retention turn fails closed and writes nothing (Z-60, pass 1).
#
#   scripts/tests/zdr-live-proof.sh --phase preflight|before|after [--state FILE] [--out FILE]
#
# It reads, never writes: SELECT statements run with default_transaction_read_only=on, `docker
# inspect` (one switch reduced to a word, nothing else of the environment is printed), one
# redis-cli --scan for the key count. Output is counts, names, revisions and timestamps: no
# content, no tenant identifier, no credential. The turn itself is sent by the opt-in Playwright
# spec tests/e2e/zdr-live-turn.spec.ts (ZDR-TEST org only); the order of operations is in
# docs/runbooks/zdr-attestation-and-live-proof.md.
#
# Phases:
#   preflight  the wave is deployed in order and nothing is switched on that must be off:
#              the four services started session-core, execution-core, model-gateway,
#              inference-core in that order, each from a revision that contains the wave
#              commit; migrations 0044 and 0045 applied; waiver minting off; no waived run.
#   before     record the counters (threads, messages, runs, feedback_ratings,
#              session_audit_outbox, ZDR rows in the outbox, the mp:gw:cache:* key count)
#              into --state. No verdict.
#   after      read the same counters and compare with --state: every one must be unchanged.
#              A refused turn leaves no row anywhere.
# The evidence is also appended to --out (default docs/compliance/evidence/live-reads-zdr-<date>.txt).
# Container names, the database and the wave commit come from the environment (dev stack defaults).
# The cache key count needs MODEL_DRAGONFLY_PASSWORD in the environment; it is handed to redis-cli
# through the environment of `docker exec`, never on a command line, and never printed.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO_ROOT="$(cd "$ROOT_DIR/../.." && pwd)"
ORIG_ARGS=("$@")
PHASE=""
STATE=""
OUT="$REPO_ROOT/docs/compliance/evidence/live-reads-zdr-$(date -u +%F).txt"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --phase | --state | --out)
      [[ $# -ge 2 && -n "$2" ]] || { echo "zdr-live-proof: $1 needs a value" >&2; exit 2; }
      case "$1" in --phase) PHASE="$2" ;; --state) STATE="$2" ;; --out) OUT="$2" ;; esac
      shift
      ;;
    -h | --help)
      sed -n '4,4p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "zdr-live-proof: unknown argument '$1'" >&2; exit 2 ;;
  esac
  shift
done
case "$PHASE" in preflight | before | after) ;; *) echo "zdr-live-proof: --phase must be preflight, before or after" >&2; exit 2 ;; esac
if [[ "$PHASE" != "preflight" && -z "$STATE" ]]; then
  echo "zdr-live-proof: --state FILE is required for before and after" >&2
  exit 2
fi
if [[ "$PHASE" == "after" && ! -f "$STATE" ]]; then
  echo "zdr-live-proof: the state file $STATE does not exist: run --phase before first" >&2
  exit 2
fi

command -v docker >/dev/null 2>&1 || { echo "zdr-live-proof: docker is not available" >&2; exit 2; }

MP_PREFIX="${MP_CONTAINER_PREFIX:-model-plane}"
MP_POSTGRES="${MP_POSTGRES_CONTAINER:-$MP_PREFIX-postgres-1}"
MP_DB="${MP_SESSION_CORE_DB:-session_core}"
MP_DB_USER="${MP_POSTGRES_USER:-postgres}"
DRAGONFLY_CONTAINER="${MP_DRAGONFLY_CONTAINER:-$MP_PREFIX-dragonfly-1}"
WAVE_COMMIT="${ZDR_WAVE_COMMIT:-HEAD}"
# Deploy order of the wave. inference-core is last: it is the service that refuses.
WAVE_SERVICES=(session-core execution-core model-gateway inference-core)
CACHE_PATTERN='mp:gw:cache:*'

# Run again through tee so the evidence file and the terminal get the same bytes, and the exit
# status is the script's own (a process substitution would let the shell exit before tee flushed).
if [[ -z "${ZDR_PROOF_TEE:-}" ]]; then
  mkdir -p "$(dirname "$OUT")"
  ZDR_PROOF_TEE=1 bash "${BASH_SOURCE[0]}" "${ORIG_ARGS[@]}" 2>&1 | tee -a "$OUT"
  exit "${PIPESTATUS[0]}"
fi

failures=0
checks=0

# check <name> <PASS|FAIL> <detail>: the before phase reports the line and never fails.
check() {
  checks=$((checks + 1))
  local name="$1" result="$2" detail="$3"
  if [[ "$PHASE" == "before" ]]; then
    echo "CHECK $name INFO ($result) $detail"
    return
  fi
  echo "CHECK $name $result $detail"
  [[ "$result" == "PASS" ]] || failures=$((failures + 1))
}

psql_ro() { # <sql>
  docker exec -e PGOPTIONS='-c default_transaction_read_only=on' "$MP_POSTGRES" \
    psql -U "$MP_DB_USER" -d "$MP_DB" -At -F '|' -v ON_ERROR_STOP=1 -c "$1" 2>&1
}

block() { # <title> <command text shown in the evidence>
  echo
  echo "## $1"
  echo "## command: $2"
}

# First field of a single-row result, or ERR when the read failed.
scalar() { # <sql>
  local out
  if out="$(psql_ro "$1")"; then printf '%s' "${out%%|*}"; else printf 'ERR'; fi
}

container_of() { printf '%s-%s-1' "$MP_PREFIX" "$1"; }

echo "# ZDR live proof, phase $PHASE. Read-only: SELECT (read-only transaction), docker inspect, one redis-cli --scan."
echo "# Counts, names, revisions, timestamps. No content, no tenant identifier, no credential."
echo "# Read at $(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ---------------------------------------------------------------- counters (before and after)

declare -A COUNTER
read_counters() {
  local table
  for table in threads messages runs feedback_ratings session_audit_outbox; do
    COUNTER[$table]="$(scalar "select count(*) from $table")"
  done
  COUNTER[outbox_zdr_rows]="$(scalar "select count(*) from session_audit_outbox where payload->'details'->>'zdr' = 'true'")"
  if [[ -n "${MODEL_DRAGONFLY_PASSWORD:-}" ]]; then
    local keys
    if keys="$(REDISCLI_AUTH="$MODEL_DRAGONFLY_PASSWORD" docker exec -e REDISCLI_AUTH "$DRAGONFLY_CONTAINER" \
      redis-cli --scan --pattern "$CACHE_PATTERN" 2>/dev/null)"; then
      COUNTER[cache_keys]="$(printf '%s' "$keys" | grep -c . || true)"
    else
      COUNTER[cache_keys]="ERR"
    fi
  else
    COUNTER[cache_keys]="ERR"
  fi
}
COUNTER_NAMES=(threads messages runs feedback_ratings session_audit_outbox outbox_zdr_rows cache_keys)

print_counters() {
  block "COUNTERS" "select count(*) from threads, messages, runs, feedback_ratings, session_audit_outbox; count of ZDR rows in the outbox; redis-cli --scan --pattern '$CACHE_PATTERN' (count only, key names are not printed)"
  local name
  for name in "${COUNTER_NAMES[@]}"; do echo "$name=${COUNTER[$name]}"; done
  if [[ "${COUNTER[cache_keys]}" == "ERR" ]]; then
    echo "# cache_keys not read: MODEL_DRAGONFLY_PASSWORD unset or $DRAGONFLY_CONTAINER unreachable"
  fi
}

# ---------------------------------------------------------------- preflight

if [[ "$PHASE" == "preflight" ]]; then
  block "BLOCK 1: what is deployed, in the order of the wave" "docker inspect (started, image id, revision label) for ${WAVE_SERVICES[*]}"
  previous_started=""
  previous_service=""
  order_ok="yes"
  revisions_ok="yes"
  for service in "${WAVE_SERVICES[@]}"; do
    container="$(container_of "$service")"
    image_id="$(docker inspect --format '{{.Image}}' "$container" 2>/dev/null || true)"
    if [[ -z "$image_id" ]]; then
      echo "$service: container $container not found"
      order_ok="no"
      revisions_ok="no"
      continue
    fi
    started="$(docker inspect --format '{{.State.StartedAt}}' "$container" 2>/dev/null || true)"
    revision="$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$image_id" 2>/dev/null || true)"
    echo "$service: started=$started image=${image_id:0:19} revision=${revision:-unknown}"
    if [[ -n "$previous_started" && "$started" < "$previous_started" ]]; then
      echo "  ORDER: $service started before $previous_service"
      order_ok="no"
    fi
    previous_started="$started"
    previous_service="$service"
    if [[ -n "$revision" && "$revision" != "<no value>" ]] &&
      git -C "$ROOT_DIR" cat-file -e "$revision^{commit}" 2>/dev/null &&
      git -C "$ROOT_DIR" merge-base --is-ancestor "$WAVE_COMMIT" "$revision" 2>/dev/null; then
      :
    else
      echo "  REVISION: $service is not built from a revision that contains $WAVE_COMMIT"
      revisions_ok="no"
    fi
  done
  if [[ "$order_ok" == "yes" ]]; then
    check "deploy-order" PASS "session-core, execution-core, model-gateway, inference-core started in that order"
  else
    check "deploy-order" FAIL "a service is missing or started before an earlier one of the wave: redeploy in order"
  fi
  if [[ "$revisions_ok" == "yes" ]]; then
    check "wave-revision" PASS "all four contain $WAVE_COMMIT"
  else
    check "wave-revision" FAIL "a service is not built from a revision containing $WAVE_COMMIT (set ZDR_WAVE_COMMIT to the commit that landed WS01-S0)"
  fi

  block "BLOCK 2: migrations 0044 and 0045 and the retention receipt column" "select count(*) from _sqlx_migrations where version in (44, 45) and success; select count(*) from information_schema.columns where table_name = 'runs' and column_name = 'retention_class'"
  migrations_ok="$(scalar "select count(*) from _sqlx_migrations where version in (44, 45) and success")"
  column_ok="$(scalar "select count(*) from information_schema.columns where table_name = 'runs' and column_name = 'retention_class'")"
  echo "migrations applied: $migrations_ok   retention_class column: $column_ok"
  if [[ "$migrations_ok" == "2" && "$column_ok" == "1" ]]; then
    check "migrations-0044-0045" PASS "both applied, column present"
  else
    check "migrations-0044-0045" FAIL "migrations=$migrations_ok column=$column_ok"
  fi

  block "BLOCK 3: waiver minting is off (ZDR_WAIVER_ENABLED, one word per service)" "docker inspect --format env of each wave service, reduced to unset, set-not-true or ON for that one name"
  waiver_on=0
  for service in "${WAVE_SERVICES[@]}"; do
    state="$(docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$(container_of "$service")" 2>/dev/null |
      awk -F= '$1 == "ZDR_WAIVER_ENABLED" { found = 1; print ($2 == "true") ? "ON" : "set-not-true" } END { if (!found) print "unset" }')"
    echo "$service: ${state:-unreadable}"
    [[ "$state" == "ON" ]] && waiver_on=$((waiver_on + 1))
  done
  if [[ "$waiver_on" == "0" ]]; then
    check "waiver-minting-off" PASS "no wave service has ZDR_WAIVER_ENABLED=true"
  else
    check "waiver-minting-off" FAIL "$waiver_on service(s) have ZDR_WAIVER_ENABLED=true"
  fi

  block "BLOCK 4: waived runs and ZDR rows (both must be 0)" "select count(*) from runs where retention_class = 'zdr_waived'; select count(*) from session_audit_outbox where payload->'details'->>'zdr' = 'true'"
  waived="$(scalar "select count(*) from runs where retention_class = 'zdr_waived'")"
  outbox_zdr="$(scalar "select count(*) from session_audit_outbox where payload->'details'->>'zdr' = 'true'")"
  echo "waived runs: $waived   ZDR rows in the outbox: $outbox_zdr"
  [[ "$waived" == "0" ]] && check "no-waived-run" PASS "0 rows" || check "no-waived-run" FAIL "count=$waived"
  [[ "$outbox_zdr" == "0" ]] && check "no-zdr-row-in-outbox" PASS "0 rows" || check "no-zdr-row-in-outbox" FAIL "count=$outbox_zdr"
fi

# ---------------------------------------------------------------- before / after

if [[ "$PHASE" == "before" ]]; then
  read_counters
  print_counters
  : >"$STATE"
  for name in "${COUNTER_NAMES[@]}"; do echo "$name=${COUNTER[$name]}" >>"$STATE"; done
  echo "# recorded in $STATE"
  if [[ "${COUNTER[cache_keys]}" == "ERR" ]]; then
    check "cache-keyspace-read" FAIL "the cache key count was not read, so the after phase cannot prove it unchanged"
  fi
fi

if [[ "$PHASE" == "after" ]]; then
  read_counters
  print_counters
  declare -A BEFORE
  while IFS='=' read -r name value; do
    [[ -n "$name" ]] && BEFORE[$name]="$value"
  done <"$STATE"
  block "DELTA" "after minus before, from $STATE"
  for name in "${COUNTER_NAMES[@]}"; do
    before="${BEFORE[$name]:-missing}"
    after="${COUNTER[$name]}"
    if [[ "$before" =~ ^[0-9]+$ && "$after" =~ ^[0-9]+$ ]]; then
      echo "$name: before=$before after=$after delta=$((after - before))"
      if [[ "$after" == "$before" ]]; then
        check "unchanged-$name" PASS "$after"
      else
        check "unchanged-$name" FAIL "before=$before after=$after (other traffic on the stack also counts: run on a quiet stack)"
      fi
    else
      echo "$name: before=$before after=$after delta=unreadable"
      check "unchanged-$name" FAIL "a counter was not read (before=$before after=$after)"
    fi
  done
  if [[ "${COUNTER[outbox_zdr_rows]}" == "0" ]]; then
    check "no-zdr-row-in-outbox" PASS "0 rows"
  else
    check "no-zdr-row-in-outbox" FAIL "count=${COUNTER[outbox_zdr_rows]}"
  fi
fi

echo
if [[ "$PHASE" == "after" ]]; then
  echo "## MANUAL (needs a user session, not read here): the Playwright spec tests/e2e/zdr-live-turn.spec.ts saw the typed refusal and no streamed content; the PATCH of the org switch was refused 409 zdr_no_attested_provider (tests/e2e/zdr-org-switch-gate.spec.ts with ZDR_GATE_LIVE_PATCH=1); the ZDR-TEST org was switched back off with zdr-seed-test-org.mjs --zdr false --apply."
fi
echo "## phase=$PHASE checks=$checks failures=$failures"
if [[ "$PHASE" != "before" && "$failures" -gt 0 ]]; then
  exit 1
fi
exit 0
