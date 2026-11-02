#!/usr/bin/env bash
# =============================================================================
# qb-preflight.sh  (design task QB-T08; it replaces the design's QB-T15 pre-flight)
#
# A READ-ONLY pre-flight for Quarry option B (POST /v1/rerank). It changes nothing: no compose up,
# down, restart, build, run or rm; no migration; no registry write; no row write; no secret read.
# It prints labels and counts only, never a token, a credential value, an organization id or a
# document. Run it before the live proof and before any web-rerank switch is turned on.
#
#   bash scripts/qb-preflight.sh --offline     files only: the committed registry row, the route
#                                              constants, the dark compose defaults, the OpenAPI,
#                                              the runbook and the matrix rows
#   bash scripts/qb-preflight.sh --live        adds the running stack, through read-only commands:
#                                              the route answers 401 without a token, the switch
#                                              states, the quarry-edge credential variable (set or
#                                              unset only), every migration file recorded, and the
#                                              baseline counters
#   bash scripts/qb-preflight.sh --live --ship-qb-t09
#                                              the live checks, and the QB-T09 ship gate (owner ruling
#                                              2026-10-12): the Data Plane gate is open (WEB_RERANK_ENABLED
#                                              on, quarry-edge on the route's allowlist, the processor gate
#                                              on under a recorded basis, capacity set) and the live
#                                              registry gives quarry-edge data:rerank on data-plane. Run it
#                                              before quarry-edge ships the rerank client. Read-only: it
#                                              reads the registry in place and prints only labels.
#   bash scripts/qb-preflight.sh --self-test   proves each mode fails on a defect, and that the live
#                                              mode issues only read-only docker commands (a fake
#                                              docker records every call)
#
# Exit status: 0 every check passed, 1 a check failed, 2 the command line is wrong.
#
# Environment (all optional):
#   DPV2_COMPOSE_PROJECT          the compose project name (as the matrix uses it)
#   DPV2_PREFLIGHT_DB_USER        the database user for the read-only SELECTs (default dataplane)
#   DPV2_PREFLIGHT_DB_NAME        the database (default dataplane)
#   SOURCE_REVISION, BUILD_DATE   the Data Plane compose requires both to interpolate, so every compose command
#                                 fails without them. Export the values of the stage being checked. A switch
#                                 that cannot be read is reported as unknown (a failed check), never as off.
#   DPV2_PREFLIGHT_ART28_CONFIRMED  yes once counsel has confirmed the Article 28 basis (OD-LEGAL);
#                                 the ship gate accepts the processor gate on under it, and on no other basis
#   QBP_DOCKER                    the docker command (the self-test points it at a fake)
#   QBP_POLICY, QBP_COMPOSE, QBP_OPENAPI, QBP_DOCS, QBP_SERVICE_ROUTES, QBP_RERANK_MOD,
#   QBP_MATRIX, QBP_MIGRATIONS    the files the offline checks read (the self-test overrides them)
# =============================================================================
set -euo pipefail
set +x

script="${BASH_SOURCE[0]}"
root="$(cd "$(dirname "$script")/.." && pwd)"

policy="${QBP_POLICY:-$root/../Control Plane/config/plane-service-principals.json}"
compose="${QBP_COMPOSE:-$root/docker-compose.yml}"
openapi="${QBP_OPENAPI:-$root/openapi/d4d5.yaml}"
docs="${QBP_DOCS:-$root/docs/web-rerank-v1.md}"
service_routes="${QBP_SERVICE_ROUTES:-$root/services/retrieval-engine-rs/src/authz/service_routes.rs}"
rerank_mod="${QBP_RERANK_MOD:-$root/services/retrieval-engine-rs/src/rerank_api/mod.rs}"
matrix="${QBP_MATRIX:-$root/scripts/auth-regression-matrix.sh}"
migrations="${QBP_MIGRATIONS:-$root/infra/postgres/migrations}"
docker_bin="${QBP_DOCKER:-docker}"
db_user="${DPV2_PREFLIGHT_DB_USER:-dataplane}"
db_name="${DPV2_PREFLIGHT_DB_NAME:-dataplane}"

failures=0
# 1 when the QB-T09 ship gate is requested (`--ship-qb-t09`, live mode only).
ship=0

pass() {
  printf '  PASS  %s\n' "$1"
}

fail() {
  printf '  FAIL  %s\n' "$1" >&2
  failures=$((failures + 1))
}

info() {
  printf '  info  %s\n' "$1"
}

# The compose project, as the matrix passes it. Every docker call goes through here.
compose() {
  if [ -n "${DPV2_COMPOSE_PROJECT:-}" ]; then
    "$docker_bin" compose --project-name "$DPV2_COMPOSE_PROJECT" "$@"
  else
    "$docker_bin" compose "$@"
  fi
}

# ---------------------------------------------------------------- offline
offline_checks() {
  printf 'Offline: the files agree with the route\n'

  # The committed registry row: production-bound, audience and both scopes present.
  local verdict
  if verdict=$(node -e '
    const fs = require("node:fs");
    const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const row = policy["quarry-edge"];
    const problems = [];
    if (!row) {
      problems.push("no quarry-edge row");
    } else {
      if (row.allowAnyOrg === true) problems.push("allowAnyOrg is set");
      if (!(row.tenancy === "directory" || (Array.isArray(row.orgIds) && row.orgIds.length > 0))) {
        problems.push("no organization binding");
      }
      if (!(row.audiences || []).includes("data-plane")) problems.push("no data-plane audience");
      for (const scope of ["data:read", "data:rerank"]) {
        if (!(row.scopes || []).includes(scope)) problems.push(`scope ${scope} is missing`);
      }
    }
    if (problems.length) {
      console.log(problems.join("; "));
      process.exit(1);
    }
  ' "$policy" 2>&1); then
    pass "committed registry: quarry-edge is production-bound with data-plane audience, data:read and data:rerank"
  else
    fail "committed registry: $verdict"
  fi

  # The route constants and the service grant for it.
  # Each file is read once into a variable and matched with a here-string. No `pipe | grep -q`: under
  # pipefail, a grep that stops at its first match can make the writer fail, and a present line would
  # then be reported missing.
  local rerank_text routes_text compose_text openapi_text matrix_text
  rerank_text=$(tr -d '\r' <"$rerank_mod")
  routes_text=$(tr -d '\r' <"$service_routes" | tr '\n' ' ')
  compose_text=$(tr -d '\r' <"$compose")
  openapi_text=$(tr -d '\r' <"$openapi")
  matrix_text=$(tr -d '\r' <"$matrix")

  if grep -Fq 'pub const RERANK_PATH: &str = "/v1/rerank";' <<<"$rerank_text"; then
    pass "route: rerank_api serves /v1/rerank"
  else
    fail "route: rerank_api does not define RERANK_PATH as /v1/rerank"
  fi
  if grep -Eq '"service:quarry-edge",[[:space:]]+"POST",[[:space:]]+"/v1/rerank",[[:space:]]+SCOPE_DATA_RERANK' <<<"$routes_text"; then
    pass "route table: service:quarry-edge may POST /v1/rerank with data:rerank"
  else
    fail "route table: the service grant for /v1/rerank is missing or names another scope"
  fi

  # The dark defaults of the compose, which the full contract checks in detail.
  if grep -Fxq '      WEB_RERANK_ENABLED: ${WEB_RERANK_ENABLED:-0}' <<<"$compose_text" &&
    grep -Fxq '      WEB_RERANK_PROCESSOR_APPROVED: ${WEB_RERANK_PROCESSOR_APPROVED:-false}' <<<"$compose_text"; then
    pass "compose: the route and the processor gate default off"
  else
    fail "compose: the web-rerank switches do not default off"
  fi

  if grep -Eq '^  /v1/rerank:$' <<<"$openapi_text"; then
    pass "openapi: /v1/rerank is documented"
  else
    fail "openapi: /v1/rerank is not documented"
  fi

  if [ -f "$docs" ]; then
    pass "runbook: docs/web-rerank-v1.md exists"
  else
    fail "runbook: docs/web-rerank-v1.md is missing"
  fi

  if grep -Fq 'DPV2_SERVICE_BEARER' <<<"$matrix_text" && grep -Fq '/v1/rerank' <<<"$matrix_text"; then
    pass "matrix: the service rows are encoded"
  else
    fail "matrix: the service rows are missing"
  fi
}

# One variable of the retrieval-engine process, read through compose. Prints the value and returns 0 when the
# read ran. Returns 1 when it did not: the exec failed, or the variable is unset (printenv exits 1 for that).
# A caller treats a failed read as unknown, never as the switch's default.
read_engine_env() {
  local value
  if value=$(compose exec -T retrieval-engine printenv "$1" 2>/dev/null); then
    printf '%s' "$value"
    return 0
  fi
  return 1
}

# How a switch is printed: its value, <unset> when the read ran and gave nothing, or <unreadable>.
shown_switch() {
  if [ "$1" != yes ]; then
    printf '%s' '<unreadable>'
  elif [ -n "$2" ]; then
    printf '%s' "$2"
  else
    printf '%s' '<unset>'
  fi
}

# ---------------------------------------------------------------- live (read-only)
live_checks() {
  printf 'Live: the running stack, read-only\n'
  # `docker compose` finds the project's compose file in the working directory, so the live checks run
  # from the Data Plane root whatever directory the operator started in.
  cd "$root"

  # The image has the route: no token is 401, and nothing is processed before the guard.
  local status
  status=$(compose exec -T retrieval-engine curl -s -o /dev/null -w '%{http_code}' --max-time 5 \
    -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:8004/v1/rerank 2>/dev/null || true)
  if [ "$status" = "401" ]; then
    pass "route: /v1/rerank answers 401 without a token"
  else
    fail "route: /v1/rerank answered '${status:-nothing}' without a token (expected 401; the image may be old)"
  fi

  # The switch states, as the process reads them. Values are not secrets; only these names are read.
  # Each read either returns a value or fails. A failed read (the compose exec did not run: the stack is down,
  # or the compose file cannot interpolate, which needs SOURCE_REVISION and BUILD_DATE) is UNKNOWN. It is never
  # read as the switch's default, so it can never pass as "off".
  local enabled approved allowed inflight rps
  local read_enabled=yes read_approved=yes read_allowed=yes read_inflight=yes read_rps=yes
  enabled=$(read_engine_env WEB_RERANK_ENABLED) || read_enabled=no
  approved=$(read_engine_env WEB_RERANK_PROCESSOR_APPROVED) || read_approved=no
  allowed=$(read_engine_env WEB_RERANK_ALLOWED_SERVICES) || read_allowed=no
  inflight=$(read_engine_env RERANK_PROVIDER_MAX_INFLIGHT) || read_inflight=no
  rps=$(read_engine_env RERANK_PROVIDER_RPS) || read_rps=no
  info "WEB_RERANK_ENABLED=$(shown_switch "$read_enabled" "$enabled")"
  info "WEB_RERANK_PROCESSOR_APPROVED=$(shown_switch "$read_approved" "$approved")"
  info "WEB_RERANK_ALLOWED_SERVICES=$(shown_switch "$read_allowed" "$allowed")"
  info "RERANK_PROVIDER_MAX_INFLIGHT=$(shown_switch "$read_inflight" "$inflight")  RERANK_PROVIDER_RPS=$(shown_switch "$read_rps" "$rps")"
  # Read as the process reads them (config.rs parse_switch): trimmed and case-insensitive, so "True" is on.
  # A value that was read but is empty is off, as the process reads an empty switch.
  local enabled_read=unknown approved_read=unknown
  if [ "$read_enabled" = yes ]; then
    enabled_read=$(printf '%s' "$enabled" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
    [ -n "$enabled_read" ] || enabled_read=0
  fi
  if [ "$read_approved" = yes ]; then
    approved_read=$(printf '%s' "$approved" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
  fi
  case "$enabled_read" in
    unknown)
      fail "switches: WEB_RERANK_ENABLED could not be read from retrieval-engine, so the route state is unknown, not off. The read needs the compose file to interpolate: export SOURCE_REVISION and BUILD_DATE for this shell, and check that the stack is running."
      ;;
    1 | true)
      if [ "$approved_read" = "true" ] || [ "$approved_read" = "1" ]; then
        pass "switches: the route is on and the processor gate is approved"
      elif [ "$approved_read" = "unknown" ] || [ "$read_approved" != yes ]; then
        fail "switches: WEB_RERANK_PROCESSOR_APPROVED could not be read while WEB_RERANK_ENABLED is on, so the gate is unknown"
      else
        fail "switches: WEB_RERANK_ENABLED is on but the processor gate is not approved (every call would be skipped)"
      fi
      if [ "$read_inflight" = yes ] && [ "$read_rps" = yes ] &&
        [ "${inflight:-0}" -gt 2 ] 2>/dev/null && [ "${rps:-0}" -gt 0 ] 2>/dev/null; then
        pass "capacity: the route has provider capacity"
      else
        fail "capacity: WEB_RERANK_ENABLED is on without provider capacity (the route answers misconfigured, or its capacity could not be read)"
      fi
      ;;
    *)
      pass "switches: the route is off, so no provider call can happen"
      ;;
  esac

  if [ "$ship" -eq 1 ]; then
    ship_gate_checks "$enabled_read" "$approved_read" "$read_allowed" "${allowed:-}"
  fi

  # The Quarry side holds its credential. Only set or unset is printed, never the value. A probe that cannot
  # run (the exec fails) is unknown, not unset, and any answer other than set or unset is reported as unread.
  local container quarry_state
  container=$("$docker_bin" ps --filter name=quarry-edge --format '{{.Names}}' 2>/dev/null | head -n 1 || true)
  if [ -z "$container" ]; then
    fail "quarry-edge: no running quarry-edge container was found"
  else
    quarry_state=$("$docker_bin" exec "$container" sh -c \
      'if [ -n "$QUARRY_SERVICE_API_KEY" ]; then echo set; else echo unset; fi' 2>/dev/null) || quarry_state=unreadable
    case "$quarry_state" in
      set) pass "quarry-edge: the service credential variable is set (value not read)" ;;
      unset) fail "quarry-edge: the service credential variable is unset" ;;
      *) fail "quarry-edge: the service credential variable could not be read (unknown, not unset)" ;;
    esac
  fi

  # Every migration file is recorded. Down files are not applied and are not counted.
  local disk recorded missing disk_count recorded_count missing_count
  disk=$(for file in "$migrations"/*.sql; do
    [ -e "$file" ] || continue
    base=$(basename "$file")
    case "$base" in *.down.sql) continue ;; esac
    printf '%s\n' "${base%%_*}"
  done | sort)
  recorded=$(compose exec -T postgres psql -U "$db_user" -d "$db_name" -tA \
    -c 'SELECT version::text FROM schema_migrations' 2>/dev/null | sort || true)
  missing=$(comm -23 <(printf '%s\n' "$disk") <(printf '%s\n' "$recorded") | sed '/^$/d' || true)
  disk_count=$(printf '%s\n' "$disk" | sed '/^$/d' | wc -l | tr -d ' ')
  recorded_count=$(printf '%s\n' "$recorded" | sed '/^$/d' | wc -l | tr -d ' ')
  missing_count=$(printf '%s\n' "$missing" | sed '/^$/d' | wc -l | tr -d ' ')
  info "migrations: on disk $disk_count, recorded $recorded_count, not recorded $missing_count"
  if [ "$missing_count" -eq 0 ] && [ "$disk_count" -gt 0 ]; then
    pass "migrations: every migration file is recorded"
  else
    fail "migrations: $missing_count migration version(s) are not recorded: $(printf '%s' "$missing" | tr '\n' ' ')"
  fi

  # Baseline counters, recorded before the proof so the proof's deltas can be read.
  local metrics name total
  metrics=$(compose exec -T retrieval-engine curl -s --max-time 5 http://127.0.0.1:8004/metrics 2>/dev/null || true)
  if [ -z "$metrics" ]; then
    fail "metrics: the retrieval-engine metrics endpoint did not answer"
  else
    for name in dpv2_authz_denials_total dpv2_web_rerank_total dpv2_web_rerank_cost_event_failures_total; do
      total=$(printf '%s\n' "$metrics" | awk -v n="$name" '
        index($1, n) == 1 && (substr($1, length(n) + 1, 1) == "{" || length($1) == length(n)) { sum += $NF; seen = 1 }
        END { if (seen) print sum; else print 0 }')
      info "baseline $name=$total"
    done
    pass "metrics: baseline counters read"
  fi

  local costs
  costs=$(compose exec -T postgres psql -U "$db_user" -d "$db_name" -tA \
    -c "SELECT count(*) FILTER (WHERE event_type = 'rerank'), count(*) FROM cost_events" 2>/dev/null || true)
  if [ -n "$costs" ]; then
    info "baseline cost_events rerank rows and all rows: ${costs}"
    pass "cost events: baseline counted (read-only)"
  else
    fail "cost events: the baseline count did not run"
  fi
}

# ---------------------------------------------------------------- QB-T09 ship gate (read-only)
# Owner ruling 2026-10-12: quarry-edge's rerank client (QB-T09) must not ship before the Data Plane gate
# and the Control Plane grant are live. Until they are, every non-ZDR search skips or degrades the rerank,
# the outcome is never cached (skipped and degraded results are not cached by design), and each search
# pays the search providers again. This gate reads switch values and registry fields; it prints labels.
ship_gate_checks() {
  # Each switch arrives as read (1 | true | a value, or unknown when it could not be read). An unknown value
  # fails the gate: the gate passes only when every part is read and open.
  local enabled_read="$1" approved_read="$2" allowed_read="$3" allowed="$4" services hold
  hold="QB-T09 must not ship yet: until this is live, every non-ZDR search skips or degrades the rerank, is never cached, and pays the search providers again"
  printf 'QB-T09 ship gate: the Data Plane gate and the Control Plane grant are live\n'

  case "$enabled_read" in
    1 | true) pass "gate: WEB_RERANK_ENABLED is on" ;;
    unknown) fail "gate: WEB_RERANK_ENABLED could not be read, so the gate cannot be shown open. $hold" ;;
    *) fail "gate: WEB_RERANK_ENABLED is off. $hold" ;;
  esac

  if [ "$allowed_read" != yes ]; then
    fail "gate: WEB_RERANK_ALLOWED_SERVICES could not be read, so the allowlist is unknown. $hold"
  else
    services=$(printf '%s' "$allowed" | tr -d '[:space:]' | tr ',' '\n')
    if grep -Fxq 'service:quarry-edge' <<<"$services"; then
      pass "gate: WEB_RERANK_ALLOWED_SERVICES names service:quarry-edge"
    else
      fail "gate: WEB_RERANK_ALLOWED_SERVICES does not name service:quarry-edge. $hold"
    fi
  fi

  # The processor gate may be on only under counsel's Article 28 sign-off (OD-LEGAL), owner ruling 2026-10-12.
  # Off, every call is skipped.
  case "$approved_read" in
    true | 1)
      if [ "${DPV2_PREFLIGHT_ART28_CONFIRMED:-}" = "yes" ]; then
        pass "gate: WEB_RERANK_PROCESSOR_APPROVED is on under a recorded basis"
      else
        fail "gate: WEB_RERANK_PROCESSOR_APPROVED is on with no recorded basis: set DPV2_PREFLIGHT_ART28_CONFIRMED=yes after counsel's Article 28 sign-off (OD-LEGAL)"
      fi
      ;;
    unknown) fail "gate: WEB_RERANK_PROCESSOR_APPROVED could not be read, so the gate is unknown. $hold" ;;
    *) fail "gate: WEB_RERANK_PROCESSOR_APPROVED is off. $hold" ;;
  esac

  grant_gate_check "$hold"
}

# The command that reads auth-service's principal registry inside the container. Production mounts the
# registry as a file (PLANE_SERVICE_PRINCIPALS_FILE, docker-compose.production.yml) and unsets the JSON
# variable; the development stack sets the JSON variable. Either is read, in place, and nothing is printed
# except by the node filter below.
registry_read='if [ -n "${PLANE_SERVICE_PRINCIPALS_FILE:-}" ]; then cat -- "$PLANE_SERVICE_PRINCIPALS_FILE"; else printenv PLANE_SERVICE_PRINCIPALS_JSON; fi'

# The live registry: auth-service's principal registry, projected in place by a node filter that prints
# one of four words. The registry text is never printed. A value that does not parse is reported as
# unreadable and is not echoed, because a parser error can quote the input.
grant_gate_check() {
  local hold="$1" container verdict
  container=$("$docker_bin" ps --filter name=auth-service --format '{{.Names}}' 2>/dev/null |
    awk '$0 == "auth-service" { print; exit }' || true)
  if [ -z "$container" ]; then
    fail "grant: no running auth-service container was found, so the live registry cannot be read. $hold"
    return 0
  fi
  verdict=$("$docker_bin" exec "$container" sh -c "$registry_read" 2>/dev/null |
    node -e '
      let raw = "";
      process.stdin.on("data", (chunk) => { raw += chunk; });
      process.stdin.on("end", () => {
        let policy;
        try { policy = JSON.parse(raw); } catch (error) { console.log("unreadable"); return; }
        const row = policy && typeof policy === "object" ? policy["quarry-edge"] : undefined;
        if (!row || typeof row !== "object") { console.log("absent"); return; }
        const onPlane = Array.isArray(row.audiences) && row.audiences.includes("data-plane");
        const scopes = row.scopesByAudience && Array.isArray(row.scopesByAudience["data-plane"])
          ? row.scopesByAudience["data-plane"] : [];
        console.log(onPlane && scopes.includes("data:rerank") ? "held" : "not-held");
      });
    ' 2>/dev/null || true)
  case "$verdict" in
    held) pass "grant: the live registry gives quarry-edge data:rerank on data-plane" ;;
    not-held) fail "grant: the live registry does not give quarry-edge data:rerank on data-plane (principal bundle P1 is not live). $hold" ;;
    absent) fail "grant: the live registry has no quarry-edge row. $hold" ;;
    *) fail "grant: the live registry could not be read (its value is not printed). $hold" ;;
  esac
}

# ---------------------------------------------------------------- self-test
# The offline checks must pass on the real files and fail on a registry without data:rerank. The
# live checks must pass against a healthy fake stack, fail on a route that admits an anonymous call
# and on a missing migration, and must never issue a mutating docker or SQL command.
self_test() {
  local tmp
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  mkdir -p "$tmp/bin" "$tmp/migrations"

  # 1. The real files pass the offline checks. (The script is run through bash: its file mode is not
  #    executable on every checkout.)
  if ! bash "$script" --offline >"$tmp/offline.out" 2>&1; then
    printf 'SELF-TEST FAIL: the real files fail the offline checks\n' >&2
    cat "$tmp/offline.out" >&2
    return 1
  fi

  # 2. A registry without data:rerank fails them.
  node -e '
    const fs = require("node:fs");
    const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    policy["quarry-edge"].scopes = policy["quarry-edge"].scopes.filter((scope) => scope !== "data:rerank");
    fs.writeFileSync(process.argv[2], JSON.stringify(policy));
  ' "$policy" "$tmp/policy-without-rerank.json"
  if QBP_POLICY="$tmp/policy-without-rerank.json" bash "$script" --offline >"$tmp/no-scope.out" 2>&1; then
    printf 'SELF-TEST FAIL: the offline checks accepted a registry without data:rerank\n' >&2
    return 1
  fi

  # 3. The fake docker: every call is logged, and each probe gets the answer of a healthy stack.
  # Any other call exits non-zero, so an unexpected command is a failure, not a silent pass.
  cat >"$tmp/bin/docker" <<'FAKE'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$FAKE_DOCKER_LOG"
case "$*" in
  *" exec -T retrieval-engine curl "*"/v1/rerank"*) printf '%s' "${FAKE_ROUTE_STATUS:-401}" ;;
  *" exec -T retrieval-engine curl "*"/metrics"*)
    printf 'dpv2_authz_denials_total{org_id="fake-org"} 2\n'
    printf 'dpv2_web_rerank_total{outcome="skipped",reason="disabled"} 4\n'
    printf 'dpv2_web_rerank_cost_event_failures_total 0\n' ;;
  *" exec -T retrieval-engine printenv WEB_RERANK_ENABLED"*)
    # A compose exec that cannot run (the compose file needs SOURCE_REVISION and BUILD_DATE to interpolate).
    if [ "${FAKE_ENABLED_READ_FAILS:-0}" = 1 ]; then printf 'required variable SOURCE_REVISION is missing\n' >&2; exit 1; fi
    printf '%s' "${FAKE_ENABLED:-0}" ;;
  *" exec -T retrieval-engine printenv WEB_RERANK_PROCESSOR_APPROVED"*)
    if [ "${FAKE_APPROVED_READ_FAILS:-0}" = 1 ]; then printf 'required variable BUILD_DATE is missing\n' >&2; exit 1; fi
    printf '%s' "${FAKE_APPROVED:-false}" ;;
  *" exec -T retrieval-engine printenv WEB_RERANK_ALLOWED_SERVICES"*) printf '%s' "${FAKE_ALLOWED:-service:quarry-edge}" ;;
  *" exec -T retrieval-engine printenv RERANK_PROVIDER_MAX_INFLIGHT"*) printf '%s' "${FAKE_INFLIGHT:-0}" ;;
  *" exec -T retrieval-engine printenv RERANK_PROVIDER_RPS"*) printf '%s' "${FAKE_RPS:-0}" ;;
  *" -c SELECT version::text FROM schema_migrations"*) cat "$FAKE_RECORDED" ;;
  *" -c SELECT count(*) FILTER"*) printf '0|7\n' ;;
  "ps --filter name=quarry-edge "*) printf 'quarry-edge-1\n' ;;
  "ps --filter name=auth-service "*) printf '%s\n' "${FAKE_AUTH_CONTAINER-auth-service}" ;;
  "exec auth-service sh -c "*)
    # The container's own shell runs the command the script sent, in the environment production (a file
    # mounted at PLANE_SERVICE_PRINCIPALS_FILE, the JSON variable unset) or development (the JSON variable
    # set) gives auth-service.
    joined="$*"
    cmd="${joined#"exec auth-service sh -c "}"
    if [ -n "${FAKE_REGISTRY_FILE:-}" ]; then
      env -i PATH="$PATH" PLANE_SERVICE_PRINCIPALS_FILE="$FAKE_REGISTRY_FILE" sh -c "$cmd"
    else
      env -i PATH="$PATH" PLANE_SERVICE_PRINCIPALS_JSON="$(cat "${FAKE_REGISTRY:-/dev/null}")" sh -c "$cmd"
    fi
    ;;
  "exec quarry-edge-1 "*)
    # A credential probe that cannot run (the container is not reachable): its exec exits non-zero.
    if [ "${FAKE_QUARRY_READ_FAILS:-0}" = 1 ]; then printf 'container is not running\n' >&2; exit 1; fi
    printf '%s' "${FAKE_QUARRY_STATE:-set}" ;;
  *) exit 97 ;;
esac
FAKE
  chmod +x "$tmp/bin/docker"

  # The recorded versions: every migration file's version, as the database would list them.
  local file base
  for file in "$root"/infra/postgres/migrations/*.sql; do
    base=$(basename "$file")
    case "$base" in *.down.sql) continue ;; esac
    printf '%s\n' "${base%%_*}"
  done >"$tmp/recorded-all.txt"
  cp "$tmp/recorded-all.txt" "$tmp/recorded.txt"
  export FAKE_DOCKER_LOG="$tmp/docker.log" FAKE_RECORDED="$tmp/recorded.txt"

  live_run() {
    QBP_DOCKER="$tmp/bin/docker" QBP_MIGRATIONS="$root/infra/postgres/migrations" \
      bash "$script" --live
  }

  : >"$tmp/docker.log"
  if ! live_run >"$tmp/live.out" 2>&1; then
    printf 'SELF-TEST FAIL: the live checks fail on a healthy fake stack\n' >&2
    cat "$tmp/live.out" >&2
    return 1
  fi
  if grep -Eq '^  FAIL' "$tmp/live.out"; then
    printf 'SELF-TEST FAIL: the healthy live run reported a failed check\n' >&2
    return 1
  fi
  # Every live check must have spoken: a check that silently did not run cannot pass.
  local expected
  for expected in \
    'PASS  route: /v1/rerank answers 401' \
    'PASS  switches: the route is off' \
    'PASS  quarry-edge: the service credential variable is set' \
    'PASS  migrations: every migration file is recorded' \
    'PASS  metrics: baseline counters read' \
    'PASS  cost events: baseline counted'; do
    if ! grep -Fq "  $expected" "$tmp/live.out"; then
      printf 'SELF-TEST FAIL: the healthy live run did not report: %s\n' "$expected" >&2
      return 1
    fi
  done

  # 3b. A switch written in another case is read as the process reads it: "True" is on, so the pre-flight
  #     must not report the route as off. The calls join the log above, so section 4 reads them too.
  export FAKE_ENABLED=True
  live_run >"$tmp/case.out" 2>&1 || true
  export FAKE_ENABLED=0
  if grep -Fq '  PASS  switches: the route is off' "$tmp/case.out"; then
    printf 'SELF-TEST FAIL: a switch written as True was read as off\n' >&2
    return 1
  fi
  if ! grep -Fq 'WEB_RERANK_ENABLED is on but the processor gate is not approved' "$tmp/case.out"; then
    printf 'SELF-TEST FAIL: a switch written as True was not read as on\n' >&2
    return 1
  fi

  # 3c. A failed compose read is UNKNOWN, never off. The fake's exec of printenv exits 1, as docker compose
  #     does when the file cannot interpolate (SOURCE_REVISION unset). The live run fails, names the switch as
  #     unknown, and never reports the route off.
  local read_failed_rc=0
  export FAKE_ENABLED_READ_FAILS=1
  live_run >"$tmp/enabled-unreadable.out" 2>&1 || read_failed_rc=$?
  export FAKE_ENABLED_READ_FAILS=0
  if [ "$read_failed_rc" -eq 0 ]; then
    printf 'SELF-TEST FAIL: the live checks passed with WEB_RERANK_ENABLED unreadable\n' >&2
    return 1
  fi
  if grep -Fq '  PASS  switches: the route is off' "$tmp/enabled-unreadable.out"; then
    printf 'SELF-TEST FAIL: an unreadable WEB_RERANK_ENABLED was reported as the route being off\n' >&2
    return 1
  fi
  if ! grep -Fq '  FAIL  switches: WEB_RERANK_ENABLED could not be read' "$tmp/enabled-unreadable.out"; then
    printf 'SELF-TEST FAIL: an unreadable WEB_RERANK_ENABLED was not reported as unknown\n' >&2
    return 1
  fi

  # 3d. The processor gate unreadable while the route is on fails, and is named as unreadable.
  read_failed_rc=0
  export FAKE_ENABLED=1 FAKE_APPROVED_READ_FAILS=1 FAKE_INFLIGHT=4 FAKE_RPS=5
  live_run >"$tmp/approved-unreadable.out" 2>&1 || read_failed_rc=$?
  export FAKE_ENABLED=0 FAKE_APPROVED_READ_FAILS=0
  unset FAKE_INFLIGHT FAKE_RPS
  if [ "$read_failed_rc" -eq 0 ] || ! grep -Fq '  FAIL  switches: WEB_RERANK_PROCESSOR_APPROVED could not be read' "$tmp/approved-unreadable.out"; then
    printf 'SELF-TEST FAIL: an unreadable processor gate with the route on was not refused\n' >&2
    return 1
  fi

  # 4. Read-only: every docker call is one of the allowed shapes. The docker `ps` for quarry-edge and for
  #    auth-service, one `exec` into the quarry-edge container (a set or unset probe), the one fixed read of
  #    the auth-service principal registry (the registry_read command, a cat or a printenv), and `compose exec`
  #    into retrieval-engine (curl and printenv) or postgres (a SELECT). Any other call, any compose verb other
  #    than exec, and any write statement fails the self-test.
  scan_read_only_log() {
    local line
    while IFS= read -r line; do
      if [ "$line" != "exec auth-service sh -c $registry_read" ] && ! printf '%s\n' "$line" | grep -Eq \
        '^(ps --filter name=(quarry-edge|auth-service) --format \{\{\.Names\}\}|exec quarry-edge-1 sh -c |compose (--project-name [^ ]+ )?exec -T (retrieval-engine (curl|printenv) |postgres psql ))'; then
        printf 'SELF-TEST FAIL: a docker call outside the read-only shapes ran: %s\n' "$line" >&2
        return 1
      fi
      if printf '%s\n' "$line" | grep -q ' psql ' && ! printf '%s\n' "$line" | grep -q -- ' -c SELECT '; then
        printf 'SELF-TEST FAIL: a SQL statement that is not a SELECT ran: %s\n' "$line" >&2
        return 1
      fi
      if printf '%s\n' "$line" | grep -Eq '(INSERT|UPDATE|DELETE|DROP|ALTER|TRUNCATE|CREATE) '; then
        printf 'SELF-TEST FAIL: a write statement ran: %s\n' "$line" >&2
        return 1
      fi
    done <"$tmp/docker.log"
    return 0
  }
  scan_read_only_log || return 1

  # 5. A route that admits an anonymous call fails the live checks.
  : >"$tmp/docker.log"
  export FAKE_ROUTE_STATUS=200
  if live_run >"$tmp/open-route.out" 2>&1; then
    printf 'SELF-TEST FAIL: the live checks accepted a route that answers 200 without a token\n' >&2
    return 1
  fi
  export FAKE_ROUTE_STATUS=401

  # 6. A migration that is not recorded fails the live checks.
  grep -v '^20261003140000$' "$tmp/recorded-all.txt" >"$tmp/recorded.txt" || true
  if live_run >"$tmp/missing-migration.out" 2>&1; then
    printf 'SELF-TEST FAIL: the live checks accepted an unrecorded migration\n' >&2
    return 1
  fi
  cp "$tmp/recorded-all.txt" "$tmp/recorded.txt"

  # 7. A credential probe that returns a value is never echoed: the value is compared to "set" and then
  #    dropped, so a run that sees a secret-shaped answer reports the probe as unread and prints nothing of it.
  export FAKE_QUARRY_STATE='SECRET-probe-value'
  if live_run >"$tmp/secret.out" 2>&1; then
    printf 'SELF-TEST FAIL: a credential probe that did not answer set was accepted\n' >&2
    return 1
  fi
  export FAKE_QUARRY_STATE=set
  if grep -Fq 'SECRET-probe-value' "$tmp/secret.out"; then
    printf 'SELF-TEST FAIL: the pre-flight echoed a credential probe value\n' >&2
    return 1
  fi

  # 7b. A credential probe that cannot run is unknown, never unset: the run fails and says it could not be read.
  local quarry_rc=0
  export FAKE_QUARRY_READ_FAILS=1
  live_run >"$tmp/quarry-unreadable.out" 2>&1 || quarry_rc=$?
  export FAKE_QUARRY_READ_FAILS=0
  if [ "$quarry_rc" -eq 0 ] || ! grep -Fq '  FAIL  quarry-edge: the service credential variable could not be read' "$tmp/quarry-unreadable.out"; then
    printf 'SELF-TEST FAIL: an unreadable credential probe was not reported as unknown\n' >&2
    return 1
  fi
  if grep -Fq 'the service credential variable is unset' "$tmp/quarry-unreadable.out"; then
    printf 'SELF-TEST FAIL: an unreadable credential probe was reported as unset\n' >&2
    return 1
  fi

  # 8. The QB-T09 ship gate. With the gate open and the grant live it passes; each closed part fails and
  #    names its reason; an unreadable registry fails without its text being printed; a mode the gate
  #    cannot run with is a command-line error (exit 2). Its docker calls are read-only (section 4).
  unset DPV2_PREFLIGHT_ART28_CONFIRMED DPV2_PREFLIGHT_PROOF_ENVIRONMENT
  export FAKE_ENABLED=1 FAKE_APPROVED=true FAKE_INFLIGHT=4 FAKE_RPS=5
  unset FAKE_AUTH_CONTAINER FAKE_ALLOWED
  export FAKE_REGISTRY="$tmp/registry-with-grant.json"
  cp "$policy" "$tmp/registry-with-grant.json"
  node -e '
    const fs = require("node:fs");
    const policy = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const row = policy["quarry-edge"];
    row.scopes = row.scopes.filter((scope) => scope !== "data:rerank");
    if (row.scopesByAudience) {
      row.scopesByAudience["data-plane"] = (row.scopesByAudience["data-plane"] || []).filter((scope) => scope !== "data:rerank");
    }
    fs.writeFileSync(process.argv[2], JSON.stringify(policy));
  ' "$policy" "$tmp/registry-without-grant.json"
  printf 'SECRET-NOT-A-REGISTRY' >"$tmp/registry-unreadable.json"
  ship_live() {
    env "$@" QBP_DOCKER="$tmp/bin/docker" QBP_MIGRATIONS="$root/infra/postgres/migrations" \
      bash "$script" --live --ship-qb-t09
  }
  : >"$tmp/docker.log"

  # 8a. Open and granted, under counsel's Article 28 sign-off: the gate passes and names each part it checked.
  if ! ship_live DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-open.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate refused an open, granted stack under the Article 28 sign-off\n' >&2
    cat "$tmp/ship-open.out" >&2
    return 1
  fi
  for expected in \
    'PASS  gate: WEB_RERANK_ENABLED is on' \
    'PASS  gate: WEB_RERANK_ALLOWED_SERVICES names service:quarry-edge' \
    'PASS  gate: WEB_RERANK_PROCESSOR_APPROVED is on under a recorded basis' \
    'PASS  grant: the live registry gives quarry-edge data:rerank on data-plane'; do
    if ! grep -Fq "  $expected" "$tmp/ship-open.out"; then
      printf 'SELF-TEST FAIL: the open ship run did not report: %s\n' "$expected" >&2
      return 1
    fi
  done

  # 8b. The same stack under counsel's Article 28 sign-off passes too.
  if ! ship_live DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-art28.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate refused a processor gate under counsel sign-off\n' >&2
    return 1
  fi

  # 8c. A processor gate on with no recorded basis fails, and says what would record it.
  if ship_live >"$tmp/ship-no-basis.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted a processor gate with no recorded basis\n' >&2
    return 1
  fi
  if ! grep -Fq 'with no recorded basis' "$tmp/ship-no-basis.out"; then
    printf 'SELF-TEST FAIL: the no-basis run did not name the missing sign-off\n' >&2
    return 1
  fi

  # 8c2. The live-proof marker is no basis for shipping: only counsel's Article 28 sign-off opens the gate for it.
  if ship_live DPV2_PREFLIGHT_PROOF_ENVIRONMENT=yes >"$tmp/ship-proof-only.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted the live-proof marker in place of counsel sign-off\n' >&2
    return 1
  fi
  if ! grep -Fq 'with no recorded basis' "$tmp/ship-proof-only.out"; then
    printf 'SELF-TEST FAIL: the proof-marker-only run did not name the missing sign-off\n' >&2
    return 1
  fi

  # 8d. A closed Data Plane gate fails and says the rerank must not ship yet.
  if ship_live FAKE_ENABLED=0 DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-closed.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted a closed Data Plane gate\n' >&2
    return 1
  fi
  if ! grep -Fq 'WEB_RERANK_ENABLED is off' "$tmp/ship-closed.out" || ! grep -Fq 'must not ship yet' "$tmp/ship-closed.out"; then
    printf 'SELF-TEST FAIL: the closed-gate run did not name the reason to hold QB-T09\n' >&2
    return 1
  fi

  # 8e. The processor gate off fails: every call would be skipped as processor_unapproved.
  if ship_live FAKE_APPROVED=false DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-unapproved.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted an unapproved processor gate\n' >&2
    return 1
  fi
  if ! grep -Fq 'WEB_RERANK_PROCESSOR_APPROVED is off' "$tmp/ship-unapproved.out"; then
    printf 'SELF-TEST FAIL: the unapproved run did not name the processor gate\n' >&2
    return 1
  fi

  # 8f. A route allowlist that does not name quarry-edge fails.
  if ship_live FAKE_ALLOWED=service:other DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-allowlist.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted an allowlist without quarry-edge\n' >&2
    return 1
  fi

  # 8g. A live registry without the data:rerank grant for quarry-edge fails: the principal bundle is not live.
  if ship_live FAKE_REGISTRY="$tmp/registry-without-grant.json" DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-grant.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted a registry without the data:rerank grant\n' >&2
    return 1
  fi
  if ! grep -Fq 'does not give quarry-edge data:rerank on data-plane' "$tmp/ship-grant.out"; then
    printf 'SELF-TEST FAIL: the no-grant run did not name the grant\n' >&2
    return 1
  fi

  # 8h. A registry that does not parse fails, and its text is never printed.
  if ship_live FAKE_REGISTRY="$tmp/registry-unreadable.json" DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-unreadable.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted an unreadable registry\n' >&2
    return 1
  fi
  if ! grep -Fq 'could not be read' "$tmp/ship-unreadable.out"; then
    printf 'SELF-TEST FAIL: the unreadable run did not say the registry could not be read\n' >&2
    return 1
  fi
  if grep -Fq 'SECRET-NOT-A-REGISTRY' "$tmp/ship-unreadable.out"; then
    printf 'SELF-TEST FAIL: the pre-flight echoed an unreadable registry value\n' >&2
    return 1
  fi

  # 8i. No running auth-service container: the registry cannot be read, so the gate fails.
  if ship_live FAKE_AUTH_CONTAINER= DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-no-auth.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate passed with no auth-service container\n' >&2
    return 1
  fi
  if ! grep -Fq 'no running auth-service container' "$tmp/ship-no-auth.out"; then
    printf 'SELF-TEST FAIL: the no-auth run did not name the missing container\n' >&2
    return 1
  fi

  # 8l. Production mounts the registry as a file (PLANE_SERVICE_PRINCIPALS_FILE, the JSON variable unset).
  #     The same grant read from that file passes.
  if ! ship_live FAKE_REGISTRY_FILE="$tmp/registry-with-grant.json" DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-file.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate refused a grant read from the mounted registry file\n' >&2
    cat "$tmp/ship-file.out" >&2
    return 1
  fi
  if ! grep -Fq '  PASS  grant: the live registry gives quarry-edge data:rerank on data-plane' "$tmp/ship-file.out"; then
    printf 'SELF-TEST FAIL: the mounted-file run did not report the grant\n' >&2
    return 1
  fi

  # 8m. A mounted registry without the grant fails, the same as the variable form does.
  if ship_live FAKE_REGISTRY_FILE="$tmp/registry-without-grant.json" DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-file-grant.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted a mounted registry without the grant\n' >&2
    return 1
  fi
  if ! grep -Fq 'does not give quarry-edge data:rerank on data-plane' "$tmp/ship-file-grant.out"; then
    printf 'SELF-TEST FAIL: the mounted no-grant run did not name the grant\n' >&2
    return 1
  fi

  # 8n. An unreadable mounted registry fails, and its text is not printed.
  if ship_live FAKE_REGISTRY_FILE="$tmp/registry-unreadable.json" DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-file-bad.out" 2>&1; then
    printf 'SELF-TEST FAIL: the ship gate accepted an unreadable mounted registry\n' >&2
    return 1
  fi
  if ! grep -Fq 'could not be read' "$tmp/ship-file-bad.out" || grep -Fq 'SECRET-NOT-A-REGISTRY' "$tmp/ship-file-bad.out"; then
    printf 'SELF-TEST FAIL: the unreadable mounted registry was not reported without its text\n' >&2
    return 1
  fi

  # 8j. The ship gate with a mode that cannot carry it is a command-line error.
  local rc=0
  bash "$script" --offline --ship-qb-t09 >/dev/null 2>&1 || rc=$?
  if [ "$rc" -ne 2 ]; then
    printf 'SELF-TEST FAIL: --offline with --ship-qb-t09 was not refused as a command-line error (exit %s)\n' "$rc" >&2
    return 1
  fi

  # 8h. A Data Plane gate that cannot be read fails the ship gate closed, and is named as unknown, never as open.
  export FAKE_ENABLED_READ_FAILS=1
  local unreadable_rc=0
  ship_live DPV2_PREFLIGHT_ART28_CONFIRMED=yes >"$tmp/ship-unreadable.out" 2>&1 || unreadable_rc=$?
  export FAKE_ENABLED_READ_FAILS=0
  if [ "$unreadable_rc" -eq 0 ]; then
    printf 'SELF-TEST FAIL: the ship gate passed with WEB_RERANK_ENABLED unreadable\n' >&2
    return 1
  fi
  if ! grep -Fq '  FAIL  gate: WEB_RERANK_ENABLED could not be read' "$tmp/ship-unreadable.out" ||
    grep -Fq '  PASS  gate: WEB_RERANK_ENABLED is on' "$tmp/ship-unreadable.out"; then
    printf 'SELF-TEST FAIL: the unreadable-gate run did not name the switch as unknown\n' >&2
    return 1
  fi

  # 8k. Every docker call the ship runs made is read-only.
  scan_read_only_log || return 1

  unset FAKE_ENABLED FAKE_APPROVED FAKE_INFLIGHT FAKE_RPS FAKE_REGISTRY FAKE_ENABLED_READ_FAILS FAKE_APPROVED_READ_FAILS
  printf 'self-test: offline, live and ship-gate checks hold on the real files and fail on the defects they name\n'
  return 0
}

usage() {
  echo "usage: $0 --offline | --live [--ship-qb-t09] | --self-test | --ship-qb-t09" >&2
}

mode=""
for arg in "$@"; do
  case "$arg" in
    --offline | --live | --self-test)
      if [ -n "$mode" ] && [ "$mode" != "$arg" ]; then
        usage
        exit 2
      fi
      mode="$arg"
      ;;
    --ship-qb-t09)
      ship=1
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done
if [ -z "$mode" ] && [ "$ship" -eq 1 ]; then
  mode="--live"
fi

case "$mode" in
  --offline)
    if [ "$ship" -eq 1 ]; then
      usage
      exit 2
    fi
    offline_checks
    ;;
  --live)
    live_checks
    ;;
  --self-test)
    if [ "$ship" -eq 1 ]; then
      usage
      exit 2
    fi
    if self_test; then
      echo "PASS: qb-preflight self-test"
      exit 0
    fi
    exit 1
    ;;
  *)
    usage
    exit 2
    ;;
esac

label="$mode"
if [ "$ship" -eq 1 ]; then
  label="$mode --ship-qb-t09"
fi
if [ "$failures" -eq 0 ]; then
  echo "PASS: qb-preflight ($label)"
  exit 0
fi
echo "FAIL: qb-preflight ($label): $failures check(s) failed" >&2
exit 1
