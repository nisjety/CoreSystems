#!/usr/bin/env bash
set -uo pipefail

# Read-only proof for the staged flip of model entitlement: off, shadow, enforce (WS10-S7).
#
#   scripts/tests/model-entitlement-live-proof.sh --phase preflight --expect-mode off|shadow|enforce [--expect-admin true|false]
#   scripts/tests/model-entitlement-live-proof.sh --phase before --state FILE
#   scripts/tests/model-entitlement-live-proof.sh --phase after  --state FILE --expect refused|unavailable|shadow-served|served
#                                                  [--out FILE]
#
# It reads, never writes: SELECT statements run with default_transaction_read_only=on, `docker
# inspect` (each switch reduced to one word, nothing else of the environment is printed), one
# redis-cli --scan for the key count and one GET of inference-core's /metrics. Output is counts,
# names, revisions and timestamps: no content, no tenant identifier, no credential, no model id.
# The turns, the policy edits and the org-core stop are done by the operator and the Playwright
# specs; the order of operations is docs/runbooks/model-entitlement-shadow-to-enforce.md.
#
# Phases:
#   preflight  the stage the stack is in is the stage the operator thinks it is:
#              org-core, inference-core, model-gateway and execution-core started in that order, the
#              three Model Plane services from a revision that contains the wave commit; org-core
#              migration 021 applied and readable past row-level security; MODEL_ENTITLEMENT_MODE on
#              inference-core spelled as --expect-mode says (a mistyped value enforces, so a value that
#              is none of off, shadow, enforce fails here); the org-core credential present when the
#              mode is not off; MODEL_ENTITLEMENT_ADMIN_ENABLED on the frontend gateway is the literal
#              true only when --expect-admin true. Mode off must have counted nothing (no org-core
#              call). Mode shadow must have soaked: inference-core up for SHADOW_MIN_SECONDS (default
#              28800, one working day is the plan's floor: raise it), org-core answered, nothing was
#              unavailable and shadow never refused; would_refuse and service_exempt are printed for
#              the operator to read, they are the soak's findings and carry no verdict.
#   before     record the counters into --state: session-core rows (threads, messages, runs,
#              feedback_ratings, session_audit_outbox, ZDR rows in the outbox), the mp:gw:cache:* key
#              count, org-core policy rows, revisions and history rows, and the model_entitlement_total
#              sums by decision. No verdict.
#   after      read the same and compare with --state. What the operator just did is named by --expect:
#                refused        enforce: a pinned denied model was sent. refuse moved, and no row anywhere
#                               (thread, message, run, feedback, audit envelope, cache key): the refusal
#                               comes before StartRun.
#                unavailable    enforce with org-core stopped past the stale bound: unavailable moved,
#                               and no row anywhere.
#                shadow-served  shadow: a would-be denied model was sent and served. would_refuse moved,
#                               refuse did not.
#                served         an entitled call (the owner, or a model inside the policy): resolved
#                               moved, refuse and unavailable did not.
#              Always: history rows never decrease, no counter went backwards and inference-core was not
#              restarted since before (a restart empties the counters and the cache).
# The evidence is also appended to --out (default docs/compliance/evidence/live-reads-model-entitlement-<date>.txt).
# Container names, databases and the wave commit come from the environment (dev stack defaults).
# MODEL_DRAGONFLY_PASSWORD is handed to redis-cli through the environment of `docker exec`, never on a
# command line, and never printed.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO_ROOT="$(cd "$ROOT_DIR/../.." && pwd)"
ORIG_ARGS=("$@")
PHASE=""
STATE=""
EXPECT_MODE=""
EXPECT_ADMIN="false"
EXPECT=""
OUT="$REPO_ROOT/docs/compliance/evidence/live-reads-model-entitlement-$(date -u +%F).txt"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --phase | --state | --out | --expect-mode | --expect-admin | --expect)
      [[ $# -ge 2 && -n "$2" ]] || { echo "model-entitlement-live-proof: $1 needs a value" >&2; exit 2; }
      case "$1" in
        --phase) PHASE="$2" ;;
        --state) STATE="$2" ;;
        --out) OUT="$2" ;;
        --expect-mode) EXPECT_MODE="$2" ;;
        --expect-admin) EXPECT_ADMIN="$2" ;;
        --expect) EXPECT="$2" ;;
      esac
      shift
      ;;
    -h | --help)
      sed -n '4,6p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "model-entitlement-live-proof: unknown argument '$1'" >&2; exit 2 ;;
  esac
  shift
done
usage_error() { echo "model-entitlement-live-proof: $1" >&2; exit 2; }
case "$PHASE" in preflight | before | after) ;; *) usage_error "--phase must be preflight, before or after" ;; esac
case "$EXPECT_ADMIN" in true | false) ;; *) usage_error "--expect-admin must be true or false" ;; esac
if [[ "$PHASE" == "preflight" ]]; then
  case "$EXPECT_MODE" in off | shadow | enforce) ;; *) usage_error "--phase preflight needs --expect-mode off, shadow or enforce" ;; esac
fi
if [[ "$PHASE" != "preflight" && -z "$STATE" ]]; then usage_error "--state FILE is required for before and after"; fi
if [[ "$PHASE" == "after" ]]; then
  [[ -f "$STATE" ]] || usage_error "the state file $STATE does not exist: run --phase before first"
  case "$EXPECT" in refused | unavailable | shadow-served | served) ;; *) usage_error "--phase after needs --expect refused, unavailable, shadow-served or served" ;; esac
fi

command -v docker >/dev/null 2>&1 || { echo "model-entitlement-live-proof: docker is not available" >&2; exit 2; }
command -v curl >/dev/null 2>&1 || { echo "model-entitlement-live-proof: curl is not available" >&2; exit 2; }

MP_PREFIX="${MP_CONTAINER_PREFIX:-model-plane}"
MP_POSTGRES="${MP_POSTGRES_CONTAINER:-$MP_PREFIX-postgres-1}"
MP_DB="${MP_SESSION_CORE_DB:-session_core}"
MP_DB_USER="${MP_POSTGRES_USER:-postgres}"
CP_POSTGRES="${CP_POSTGRES_CONTAINER:-controlplane-postgres}"
CP_DB="${CP_ORG_CORE_DB:-org_core}"
CP_DB_USER="${CP_POSTGRES_USER:-coresystem}"
ORG_CORE_CONTAINER="${CP_ORG_CORE_CONTAINER:-org-core-service}"
GATEWAY_CONTAINER="${FRONTEND_GATEWAY_CONTAINER:-verevon-gateway-rs}"
DRAGONFLY_CONTAINER="${MP_DRAGONFLY_CONTAINER:-$MP_PREFIX-dragonfly-1}"
INFERENCE_HTTP="${INFERENCE_CORE_HTTP:-http://127.0.0.1:18082}"
WAVE_COMMIT="${MODEL_ENTITLEMENT_WAVE_COMMIT:-HEAD}"
SHADOW_MIN_SECONDS="${SHADOW_MIN_SECONDS:-28800}"
MIGRATION_VERSION="021_org_model_entitlements"
CACHE_PATTERN='mp:gw:cache:*'
# Deploy order of the flip. org-core first (the answer and the credential exist before anyone asks),
# inference-core next (the enforcement point), then the services that ask it. Only the Model Plane
# services carry the image revision label; org-core is proven by its migration instead.
ORDER_CONTAINERS=("$ORG_CORE_CONTAINER" "$MP_PREFIX-inference-core-1" "$MP_PREFIX-model-gateway-1" "$MP_PREFIX-execution-core-1")
ORDER_NAMES=(org-core inference-core model-gateway execution-core)
REVISION_FROM=1

# Run again through tee so the evidence file and the terminal get the same bytes, and the exit
# status is the script's own (a process substitution would let the shell exit before tee flushed).
if [[ -z "${MODEL_ENTITLEMENT_PROOF_TEE:-}" ]]; then
  mkdir -p "$(dirname "$OUT")"
  MODEL_ENTITLEMENT_PROOF_TEE=1 bash "${BASH_SOURCE[0]}" "${ORIG_ARGS[@]}" 2>&1 | tee -a "$OUT"
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

block() { # <title> <command text shown in the evidence>
  echo
  echo "## $1"
  echo "## command: $2"
}

psql_ro() { # <container> <user> <database> <sql>
  docker exec -e PGOPTIONS='-c default_transaction_read_only=on' "$1" \
    psql -U "$2" -d "$3" -At -F '|' -v ON_ERROR_STOP=1 -c "$4" 2>&1
}
# First field of a single-row result, or ERR when the read failed.
scalar() { # <container> <user> <database> <sql>
  local out
  if out="$(psql_ro "$1" "$2" "$3" "$4")"; then printf '%s' "${out%%|*}"; else printf 'ERR'; fi
}
mp_scalar() { scalar "$MP_POSTGRES" "$MP_DB_USER" "$MP_DB" "$1"; }
cp_scalar() { scalar "$CP_POSTGRES" "$CP_DB_USER" "$CP_DB" "$1"; }

# One word for a switch on a container: the value reduced to what the proof needs, never the value
# of anything else. mode: unset|off|shadow|enforce|unrecognised. admin: unset|ON|set-not-true.
# token: set|unset (presence only).
env_word() { # <container> <name> <kind>
  docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$1" 2>/dev/null |
    awk -F= -v name="$2" -v kind="$3" '
      $1 == name {
        found = 1
        value = substr($0, length(name) + 2)
        if (kind == "token") { print (value == "" ? "unset" : "set"); next }
        if (kind == "admin") { print (value == "true" ? "ON" : "set-not-true"); next }
        v = tolower(value); gsub(/^[ \t]+|[ \t]+$/, "", v)
        if (v == "") print "off"; else if (v == "off" || v == "shadow" || v == "enforce") print v; else print "unrecognised"
      }
      END { if (!found) print (kind == "mode" ? "off" : "unset") }' | head -n 1
}

# ---------------------------------------------------------------- metrics

METRICS=""
METRICS_OK=0
read_metrics() {
  if METRICS="$(curl -fsS --max-time 10 "$INFERENCE_HTTP/metrics" 2>/dev/null)"; then METRICS_OK=1; else METRICS=""; METRICS_OK=0; fi
}
# Sum of model_entitlement_total for one decision, optionally one mode; ERR when /metrics was not read.
metric_sum() { # <decision> [mode]
  if [[ "$METRICS_OK" != "1" ]]; then printf 'ERR'; return; fi
  awk -v d="$1" -v m="${2:-}" '
    /^model_entitlement_total\{/ && index($0, "decision=\"" d "\"") && (m == "" || index($0, "mode=\"" m "\"")) { s += $NF }
    END { printf "%d", s }' <<<"$METRICS"
}
METRIC_DECISIONS=(resolved stale unavailable would_unavailable refuse would_refuse service_exempt)

echo "# Model entitlement live proof, phase $PHASE. Read-only: SELECT (read-only transaction), docker inspect, one redis-cli --scan, one GET of /metrics."
echo "# Counts, names, revisions, timestamps. No content, no tenant identifier, no model id, no credential."
echo "# Read at $(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ---------------------------------------------------------------- preflight

if [[ "$PHASE" == "preflight" ]]; then
  block "BLOCK 1: what is deployed, in the order of the flip" "docker inspect (started, image id, revision label) for ${ORDER_NAMES[*]}"
  previous_started=""
  previous_name=""
  order_ok="yes"
  revisions_ok="yes"
  inference_started=""
  for i in "${!ORDER_NAMES[@]}"; do
    name="${ORDER_NAMES[$i]}"
    container="${ORDER_CONTAINERS[$i]}"
    image_id="$(docker inspect --format '{{.Image}}' "$container" 2>/dev/null || true)"
    if [[ -z "$image_id" ]]; then
      echo "$name: container $container not found"
      order_ok="no"
      [[ "$i" -ge "$REVISION_FROM" ]] && revisions_ok="no"
      continue
    fi
    started="$(docker inspect --format '{{.State.StartedAt}}' "$container" 2>/dev/null || true)"
    [[ "$name" == "inference-core" ]] && inference_started="$started"
    if [[ "$i" -ge "$REVISION_FROM" ]]; then
      revision="$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$image_id" 2>/dev/null || true)"
      echo "$name: started=$started image=${image_id:0:19} revision=${revision:-unknown}"
      if [[ -n "$revision" && "$revision" != "<no value>" ]] &&
        git -C "$ROOT_DIR" cat-file -e "$revision^{commit}" 2>/dev/null &&
        git -C "$ROOT_DIR" merge-base --is-ancestor "$WAVE_COMMIT" "$revision" 2>/dev/null; then
        :
      else
        echo "  REVISION: $name is not built from a revision that contains $WAVE_COMMIT"
        revisions_ok="no"
      fi
    else
      echo "$name: started=$started image=${image_id:0:19} (no revision label on Control Plane images: its migration is checked in block 2)"
    fi
    if [[ -n "$previous_started" && "$started" < "$previous_started" ]]; then
      echo "  ORDER: $name started before $previous_name"
      order_ok="no"
    fi
    previous_started="$started"
    previous_name="$name"
  done
  if [[ "$order_ok" == "yes" ]]; then
    check "deploy-order" PASS "org-core, inference-core, model-gateway, execution-core started in that order"
  else
    check "deploy-order" FAIL "a service is missing or started before an earlier one of the flip: restart in order"
  fi
  if [[ "$revisions_ok" == "yes" ]]; then
    check "wave-revision" PASS "inference-core, model-gateway, execution-core contain $WAVE_COMMIT"
  else
    check "wave-revision" FAIL "a Model Plane service is not built from a revision containing $WAVE_COMMIT (set MODEL_ENTITLEMENT_WAVE_COMMIT to the commit that landed WS10-S4)"
  fi

  block "BLOCK 2: org-core migration 021 and a reader that sees past row-level security" "select count(*) from schema_migrations where version = '$MIGRATION_VERSION'; select count(*) from information_schema.tables where table_name in the three model policy tables; select rolsuper or rolbypassrls from pg_roles where rolname = current_user"
  migration_ok="$(cp_scalar "select count(*) from schema_migrations where version = '$MIGRATION_VERSION'")"
  tables_ok="$(cp_scalar "select count(*) from information_schema.tables where table_name in ('org_model_policies', 'org_model_policy_revisions', 'org_model_policy_history')")"
  bypass="$(cp_scalar "select (rolsuper or rolbypassrls) from pg_roles where rolname = current_user")"
  echo "migration applied: $migration_ok   policy tables: $tables_ok   reader bypasses row-level security: $bypass"
  if [[ "$migration_ok" == "1" && "$tables_ok" == "3" ]]; then
    check "org-core-migration-021" PASS "applied, three tables present"
  else
    check "org-core-migration-021" FAIL "migration=$migration_ok tables=$tables_ok"
  fi
  if [[ "$bypass" == "t" ]]; then
    check "org-core-reader-sees-all-rows" PASS "the proof's database role bypasses row-level security"
  else
    check "org-core-reader-sees-all-rows" FAIL "the role CP_POSTGRES_USER=$CP_DB_USER does not bypass row-level security: policy counts would read 0 whatever is stored (use the database owner)"
  fi

  block "BLOCK 3: the switches (one word each)" "docker inspect --format env, reduced to one word for MODEL_ENTITLEMENT_MODE and INFERENCE_ORG_CORE_SERVICE_TOKEN (presence only) on inference-core and MODEL_ENTITLEMENT_ADMIN_ENABLED on the frontend gateway"
  mode="$(env_word "$MP_PREFIX-inference-core-1" MODEL_ENTITLEMENT_MODE mode)"
  token="$(env_word "$MP_PREFIX-inference-core-1" INFERENCE_ORG_CORE_SERVICE_TOKEN token)"
  admin="$(env_word "$GATEWAY_CONTAINER" MODEL_ENTITLEMENT_ADMIN_ENABLED admin)"
  echo "inference-core MODEL_ENTITLEMENT_MODE: ${mode:-unreadable}   org-core credential: ${token:-unreadable}   gateway MODEL_ENTITLEMENT_ADMIN_ENABLED: ${admin:-unreadable}"
  if [[ "$mode" == "unrecognised" ]]; then
    check "mode-is-$EXPECT_MODE" FAIL "the value is none of off, shadow, enforce: inference-core treats it as enforce. Spell it exactly"
  elif [[ "$mode" == "$EXPECT_MODE" ]]; then
    check "mode-is-$EXPECT_MODE" PASS "inference-core runs $mode"
  else
    check "mode-is-$EXPECT_MODE" FAIL "inference-core runs ${mode:-unreadable}, expected $EXPECT_MODE"
  fi
  if [[ "$EXPECT_MODE" == "off" || "$token" == "set" ]]; then
    check "org-core-credential" PASS "credential $token, mode $EXPECT_MODE"
  else
    check "org-core-credential" FAIL "mode $EXPECT_MODE needs INFERENCE_ORG_CORE_SERVICE_TOKEN: without it every user call and the catalogue are refused under enforce, and shadow cannot resolve"
  fi
  if [[ "$EXPECT_ADMIN" == "true" && "$admin" == "ON" ]] || [[ "$EXPECT_ADMIN" == "false" && "$admin" != "ON" ]]; then
    check "admin-flag-is-$EXPECT_ADMIN" PASS "gateway admin switch: $admin"
  else
    check "admin-flag-is-$EXPECT_ADMIN" FAIL "gateway admin switch is ${admin:-unreadable}, expected $EXPECT_ADMIN (only the literal true enables it)"
  fi

  block "BLOCK 4: what inference-core has decided since it started" "GET $INFERENCE_HTTP/metrics, model_entitlement_total by decision and mode"
  read_metrics
  if [[ "$METRICS_OK" == "1" ]]; then
    for decision in "${METRIC_DECISIONS[@]}"; do
      echo "$decision: shadow=$(metric_sum "$decision" shadow) enforce=$(metric_sum "$decision" enforce)"
    done
  else
    echo "/metrics not read from $INFERENCE_HTTP"
  fi
  if [[ "$EXPECT_MODE" == "off" ]]; then
    total=0
    for decision in "${METRIC_DECISIONS[@]}"; do
      value="$(metric_sum "$decision")"
      [[ "$value" =~ ^[0-9]+$ ]] && total=$((total + value)) || total="ERR"
      [[ "$total" == "ERR" ]] && break
    done
    if [[ "$total" == "0" ]]; then
      check "off-makes-no-org-core-call" PASS "no entitlement decision counted"
    else
      check "off-makes-no-org-core-call" FAIL "decisions counted=$total, or /metrics unread: mode off resolves nothing"
    fi
  fi
  if [[ "$EXPECT_MODE" == "shadow" ]]; then
    started_epoch="$(date -u -d "$inference_started" +%s 2>/dev/null || true)"
    now_epoch="$(date -u +%s)"
    if [[ "$started_epoch" =~ ^[0-9]+$ ]]; then
      uptime=$((now_epoch - started_epoch))
      echo "inference-core uptime: ${uptime}s (soak floor ${SHADOW_MIN_SECONDS}s)"
      if [[ "$uptime" -ge "$SHADOW_MIN_SECONDS" ]]; then
        check "shadow-soaked" PASS "up ${uptime}s"
      else
        check "shadow-soaked" FAIL "up ${uptime}s, floor ${SHADOW_MIN_SECONDS}s: the soak restarts with every restart of inference-core"
      fi
    else
      check "shadow-soaked" FAIL "the start time of inference-core was not read"
    fi
    resolved="$(metric_sum resolved shadow)"
    unavailable="$(metric_sum unavailable shadow)"
    would_unavailable="$(metric_sum would_unavailable shadow)"
    refused="$(metric_sum refuse shadow)"
    if [[ "$resolved" =~ ^[0-9]+$ && "$resolved" -gt 0 ]]; then
      check "shadow-org-core-answered" PASS "resolved=$resolved"
    else
      check "shadow-org-core-answered" FAIL "resolved=$resolved: org-core never answered inference-core (credential, ORG_CORE_URL or registry), or no user call has been made"
    fi
    if [[ "$unavailable" == "0" && "$would_unavailable" == "0" ]]; then
      check "shadow-nothing-unavailable" PASS "0"
    else
      check "shadow-nothing-unavailable" FAIL "unavailable=$unavailable would_unavailable=$would_unavailable: enforce would have refused these calls"
    fi
    if [[ "$refused" == "0" ]]; then
      check "shadow-never-refuses" PASS "0"
    else
      check "shadow-never-refuses" FAIL "refuse=$refused under shadow: shadow must never refuse"
    fi
    echo "# for the operator to read, no verdict: would_refuse=$(metric_sum would_refuse shadow) service_exempt=$(metric_sum service_exempt shadow). Every would_refuse is a call enforce will refuse; every service_exempt is a platform job that policy never touches."
  fi
fi

# ---------------------------------------------------------------- before / after

declare -A COUNTER
SESSION_COUNTERS=(threads messages runs feedback_ratings session_audit_outbox outbox_zdr_rows cache_keys)
COUNTER_NAMES=("${SESSION_COUNTERS[@]}" org_policy_rows org_policy_revisions org_policy_history)
for decision in "${METRIC_DECISIONS[@]}"; do COUNTER_NAMES+=("m_$decision"); done

read_counters() {
  local table
  for table in threads messages runs feedback_ratings session_audit_outbox; do
    COUNTER[$table]="$(mp_scalar "select count(*) from $table")"
  done
  COUNTER[outbox_zdr_rows]="$(mp_scalar "select count(*) from session_audit_outbox where payload->'details'->>'zdr' = 'true'")"
  COUNTER[cache_keys]="ERR"
  if [[ -n "${MODEL_DRAGONFLY_PASSWORD:-}" ]]; then
    local keys
    if keys="$(REDISCLI_AUTH="$MODEL_DRAGONFLY_PASSWORD" docker exec -e REDISCLI_AUTH "$DRAGONFLY_CONTAINER" \
      redis-cli --scan --pattern "$CACHE_PATTERN" 2>/dev/null)"; then
      COUNTER[cache_keys]="$(printf '%s' "$keys" | grep -c . || true)"
    fi
  fi
  COUNTER[org_policy_rows]="$(cp_scalar "select count(*) from org_model_policies")"
  COUNTER[org_policy_revisions]="$(cp_scalar "select count(*) from org_model_policy_revisions")"
  COUNTER[org_policy_history]="$(cp_scalar "select count(*) from org_model_policy_history")"
  read_metrics
  local decision
  for decision in "${METRIC_DECISIONS[@]}"; do COUNTER[m_$decision]="$(metric_sum "$decision")"; done
  STARTED="$(docker inspect --format '{{.State.StartedAt}}' "$MP_PREFIX-inference-core-1" 2>/dev/null || true)"
}

print_counters() {
  block "COUNTERS" "select count(*) from threads, messages, runs, feedback_ratings, session_audit_outbox (session-core); ZDR rows in the outbox; redis-cli --scan --pattern '$CACHE_PATTERN' (count only); select count(*) from org_model_policies, org_model_policy_revisions, org_model_policy_history (org-core); model_entitlement_total by decision from /metrics"
  local name
  for name in "${COUNTER_NAMES[@]}"; do echo "$name=${COUNTER[$name]}"; done
  echo "inference_core_started=$STARTED"
  if [[ "${COUNTER[cache_keys]}" == "ERR" ]]; then
    echo "# cache_keys not read: MODEL_DRAGONFLY_PASSWORD unset or $DRAGONFLY_CONTAINER unreachable"
  fi
}

if [[ "$PHASE" == "before" ]]; then
  read_counters
  print_counters
  : >"$STATE"
  for name in "${COUNTER_NAMES[@]}"; do echo "$name=${COUNTER[$name]}" >>"$STATE"; done
  echo "inference_core_started=$STARTED" >>"$STATE"
  echo "# recorded in $STATE"
  for name in "${COUNTER_NAMES[@]}"; do
    [[ "${COUNTER[$name]}" == "ERR" ]] && check "read-$name" FAIL "the counter was not read, so the after phase cannot compare it"
  done
fi

if [[ "$PHASE" == "after" ]]; then
  read_counters
  print_counters
  declare -A BEFORE
  while IFS='=' read -r name value; do
    [[ -n "$name" ]] && BEFORE[$name]="$value"
  done <"$STATE"

  # delta <name>: after minus before, or ERR when either side was not read.
  delta() {
    local before="${BEFORE[$1]:-missing}" after="${COUNTER[$1]}"
    if [[ "$before" =~ ^[0-9]+$ && "$after" =~ ^[0-9]+$ ]]; then printf '%d' $((after - before)); else printf 'ERR'; fi
  }
  block "DELTA" "after minus before, from $STATE"
  for name in "${COUNTER_NAMES[@]}"; do echo "$name: before=${BEFORE[$name]:-missing} after=${COUNTER[$name]} delta=$(delta "$name")"; done

  # The checks that hold whatever the operator did.
  if [[ -n "${BEFORE[inference_core_started]:-}" && "${BEFORE[inference_core_started]}" == "$STARTED" ]]; then
    check "inference-core-not-restarted" PASS "started $STARTED"
  else
    check "inference-core-not-restarted" FAIL "started before=${BEFORE[inference_core_started]:-missing} after=$STARTED: a restart empties the counters and the cache, so no delta is meaningful"
  fi
  backwards=""
  for decision in "${METRIC_DECISIONS[@]}"; do
    d="$(delta "m_$decision")"
    if [[ "$d" == "ERR" ]]; then backwards="$backwards $decision(unread)"; elif [[ "$d" -lt 0 ]]; then backwards="$backwards $decision($d)"; fi
  done
  if [[ -z "$backwards" ]]; then
    check "metrics-monotonic" PASS "no entitlement counter went backwards or was unread"
  else
    check "metrics-monotonic" FAIL "counters went backwards or were unread:$backwards"
  fi
  d="$(delta org_policy_history)"
  if [[ "$d" != "ERR" && "$d" -ge 0 ]]; then
    check "history-append-only" PASS "history rows delta=$d"
  else
    check "history-append-only" FAIL "history rows delta=$d: the policy history only grows"
  fi

  # Under refused and unavailable nothing may have been written anywhere: the refusal is before StartRun.
  unchanged_everywhere() { # <why>
    local name d
    for name in "${SESSION_COUNTERS[@]}"; do
      d="$(delta "$name")"
      if [[ "$d" == "0" ]]; then
        check "unchanged-$name" PASS "${COUNTER[$name]}"
      else
        check "unchanged-$name" FAIL "delta=$d ($1; other traffic on the stack also counts: run on a quiet stack)"
      fi
    done
    if [[ "${COUNTER[outbox_zdr_rows]}" == "${BEFORE[outbox_zdr_rows]:-x}" ]]; then
      check "no-new-zdr-row-in-outbox" PASS "${COUNTER[outbox_zdr_rows]}"
    else
      check "no-new-zdr-row-in-outbox" FAIL "before=${BEFORE[outbox_zdr_rows]:-missing} after=${COUNTER[outbox_zdr_rows]}"
    fi
  }
  # moved <decision> [min]: the entitlement counter for a decision rose by at least min (default 1).
  moved() {
    local d min="${2:-1}"
    d="$(delta "m_$1")"
    if [[ "$d" != "ERR" && "$d" -ge "$min" ]]; then
      check "$1-moved" PASS "delta=$d"
    else
      check "$1-moved" FAIL "delta=$d, expected at least $min"
    fi
  }
  held() { # <decision>: the counter did not move
    local d
    d="$(delta "m_$1")"
    if [[ "$d" == "0" ]]; then check "$1-held" PASS "delta=0"; else check "$1-held" FAIL "delta=$d, expected 0"; fi
  }
  mode_is() { # <mode>
    local m
    m="$(env_word "$MP_PREFIX-inference-core-1" MODEL_ENTITLEMENT_MODE mode)"
    if [[ "$m" == "$1" ]]; then check "mode-is-$1" PASS "inference-core runs $m"; else check "mode-is-$1" FAIL "inference-core runs ${m:-unreadable}, this expectation needs $1"; fi
  }

  case "$EXPECT" in
    refused)
      mode_is enforce
      moved refuse
      held unavailable
      unchanged_everywhere "a refused turn writes nothing: no thread, message, run, feedback, audit envelope or cache key"
      ;;
    unavailable)
      mode_is enforce
      moved unavailable
      unchanged_everywhere "an unavailable entitlement refuses before StartRun and writes nothing"
      ;;
    shadow-served)
      mode_is shadow
      moved would_refuse
      held refuse
      ;;
    served)
      moved resolved
      held refuse
      held unavailable
      ;;
  esac
fi

echo
if [[ "$PHASE" == "after" ]]; then
  case "$EXPECT" in
    refused) echo "## MANUAL (needs a user session, not read here): the refusal the member saw is the typed model_not_entitled (Norwegian text in the SPA), over the stream and over the unary route; the model picker for that member lists the allowed models and the Verevon modes only; the owner's call (--expect served) was not refused." ;;
    unavailable) echo "## MANUAL: org-core was stopped past the stale bound (900 s when the last answer restricted, 120 s when it did not) before the call, and the reply was the typed 503 model_entitlement_unavailable; calls inside the bound were served; after org-core was started again the next call resolved." ;;
    shadow-served) echo "## MANUAL: the call was served, the log line names organisation, revision and reason and holds no content. Read would_refuse and service_exempt over the whole soak before the enforce flip." ;;
    served) echo "## MANUAL: this was the owner, or a model inside the policy, or a call after a policy edit within the 30 s TTL." ;;
  esac
fi
echo "## phase=$PHASE checks=$checks failures=$failures"
if [[ "$PHASE" != "before" && "$failures" -gt 0 ]]; then
  exit 1
fi
exit 0
