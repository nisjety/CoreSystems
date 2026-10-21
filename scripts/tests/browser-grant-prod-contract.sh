#!/usr/bin/env bash
# SC1 L2 wrapper: the production-mode browser grant contract on a live stack.
# Plan 6.4 A (WS1.A2). It runs the Playwright spec
#   apps/Frontend Plane/verevonv3/tests/e2e/browser-grants-prod-contract.spec.ts
# in three phases around a per-hop log scan, and prints a transcript that can be
# pasted into a PR. It never prints a token, a secret or a log line: scan results
# are counts per hop and pattern.
#
#   bash scripts/tests/browser-grant-prod-contract.sh                  preflight + legs + scan
#   bash scripts/tests/browser-grant-prod-contract.sh --preflight-only  checks only, changes nothing
#   bash scripts/tests/browser-grant-prod-contract.sh --boot --yes-rebuild-stack
#                                                                       rebuild the stack first (see below)
#   bash scripts/tests/browser-grant-prod-contract.sh --self-test      the scanner and the parser, no stack
#
# The stack it expects (the exit-gate stack, never the user's live one without the
# owner's say-so): quarry-edge on docker-compose.yml + docker-compose.production.yml
# + docker-compose.grant-contract.yml with ENVIRONMENT=production; the broker with
# BROWSER_BROKER_GRANT_TTL_SECONDS=45; auth-core serving the registry row of
# quarry-edge on audience browser-broker; the gateway with
# BROWSER_GRANT_ENFORCEMENT=strict. A seeded local@verevon.dev account (and
# e2e@verevon.dev for leg A6) signs in through the same-origin gateway.
#
# Phases (the plan puts the log scan after the happy legs and before the negatives):
#   1. @happy   legs A1 to A4                 then the five-hop log scan (fatal)
#   2. @legBC   legs B and C (test.fail())    scanned, fatal only with CONTRACT_BC_GREEN=1
#   3. @negative legs A5 and A6
#
# --boot runs the repo's own recipes in order and REBUILDS containers of the running
# stack (auth-core, browser-broker, model-gateway, quarry-edge, the gateway image),
# so it needs --yes-rebuild-stack. Without --boot nothing is changed except one
# throwaway container, grant-contract-tunnel, that gives the host a loopback
# route to quarry-edge (production publishes no port); it is removed on exit.
#
# Environment (all optional):
#   ENV_ROOT           where the plane env files live (default: this repo). The
#                      exit gate points it at the checkout that owns the stack.
#   QUARRY_CONTAINER, BROKER_CONTAINER, MODEL_GATEWAY_CONTAINER, EXECUTION_CORE_CONTAINER,
#   GATEWAY_CONTAINER, AUTH_CONTAINER   container names of each hop
#   AUTH_CORE_URL, MODEL_GATEWAY_URL, LOCAL_E2E_BASE_URL   as the spec reads them
#   TUNNEL_IMAGE       image for the loopback tunnel (default alpine/socat)
#   QUARRY_EDGE_URL    set to skip the tunnel and use this address
#   DOCKER_LOCK        a prefix that serialises container-starting docker commands,
#                      e.g. "bash /path/with-lock.sh docker 2700 --"
#   CONTRACT_BC_GREEN=1   legs B and C are expected to pass (after WS1.B): the
#                      scan of phase 2 becomes fatal and test.fail() must be gone.
#   GATEWAY_SWAP_TOOL  the host tool that swaps the gateway image (--boot only)

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENV_ROOT="${ENV_ROOT:-$REPO}"
PLANE="$REPO/apps/Ingestion Plane"
SPA="$REPO/apps/Frontend Plane/verevonv3"

QUARRY_CONTAINER="${QUARRY_CONTAINER:-quarry-edge}"
BROKER_CONTAINER="${BROKER_CONTAINER:-model-plane-browser-broker-1}"
MODEL_GATEWAY_CONTAINER="${MODEL_GATEWAY_CONTAINER:-model-plane-model-gateway-1}"
EXECUTION_CORE_CONTAINER="${EXECUTION_CORE_CONTAINER:-model-plane-execution-core-1}"
GATEWAY_CONTAINER="${GATEWAY_CONTAINER:-frontend-plane-verevonv3-gateway-1}"
AUTH_CONTAINER="${AUTH_CONTAINER:-auth-service}"
TUNNEL_NAME="grant-contract-tunnel"
TUNNEL_PORT="${TUNNEL_PORT:-18082}"
TUNNEL_IMAGE="${TUNNEL_IMAGE:-alpine/socat}"
GRANT_TTL_SECONDS=45
BROKER_GRPC="http://browser-broker:9095"
DOCKER_LOCK="${DOCKER_LOCK:-}"

# The scan of one hop's log: a hit is a refusal somewhere on the happy path.
FAILURE_PATTERN='\b40[13]\b|PermissionDenied|Unauthenticated|browser grant is not authorized|browser grant_id is required'
# Host names the legs use; they must not appear in the three hops that the plan keeps domain-free.
DOMAIN_PATTERN='example\.com|example\.org|evil\.example'

log() { printf '%s\n' "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; FAILED=1; }
FAILED=0

# ---- pure helpers (also driven by --self-test) -------------------------------------

# count_hits <pattern>: how many lines on stdin match; never echoes a line.
count_hits() { grep -Ec -- "$1" || true; }

# env_value <name>: the value of NAME in `KEY=VALUE` lines on stdin ('' when absent).
env_value() { local name="$1"; sed -n "s/^${name}=//p" | head -n 1; }

# registry_ok: reads the plane-service-principal registry JSON on stdin and prints
# "ok" only when quarry-edge holds browser:read on audience browser-broker and no
# principal holds browser:write anywhere. It prints nothing else of the file.
registry_ok() {
  node -e '
    let s = ""; process.stdin.on("data", (c) => (s += c)).on("end", () => {
      try {
        const reg = JSON.parse(s);
        const edge = reg["quarry-edge"] || {};
        const read = (edge.scopesByAudience || {})["browser-broker"] || [];
        const write = Object.values(reg).some((p) =>
          (p.scopes || []).includes("browser:write") ||
          Object.values(p.scopesByAudience || {}).some((l) => l.includes("browser:write")));
        console.log(read.length === 1 && read[0] === "browser:read" && !write ? "ok" : "bad");
      } catch { console.log("unreadable"); }
    });'
}

self_test() {
  local bad=0
  check() { # <name> <expected> <actual>
    if [ "$2" = "$3" ]; then log "  ok   $1"; else log "  FAIL $1 (want $2, got $3)"; bad=1; fi
  }
  log "self-test: the scanner, the env reader and the registry check (no stack involved)"
  check "a clean log has no hit" 0 "$(printf 'GET /health 200\nrun started\n' | count_hits "$FAILURE_PATTERN")"
  check "a 403 is a hit" 1 "$(printf 'status 403 returned\n' | count_hits "$FAILURE_PATTERN")"
  check "a 401 is a hit" 1 "$(printf 'upstream said 401\n' | count_hits "$FAILURE_PATTERN")"
  check "PermissionDenied is a hit" 1 "$(printf 'grpc PermissionDenied: no\n' | count_hits "$FAILURE_PATTERN")"
  check "Unauthenticated is a hit" 1 "$(printf 'code Unauthenticated\n' | count_hits "$FAILURE_PATTERN")"
  check "the missing-grant refusal is a hit" 1 "$(printf 'browser grant_id is required\n' | count_hits "$FAILURE_PATTERN")"
  check "the grant refusal is a hit" 1 "$(printf 'browser grant is not authorized\n' | count_hits "$FAILURE_PATTERN")"
  check "4030 and port 14031 are not a 403 or 401" 0 "$(printf 'port 4030 and id 14031\n' | count_hits "$FAILURE_PATTERN")"
  check "two hits count two" 2 "$(printf '403\nok\n401\n' | count_hits "$FAILURE_PATTERN")"
  check "a domain string is a hit" 1 "$(printf 'navigating to example.com\n' | count_hits "$DOMAIN_PATTERN")"
  check "a log without domains is clean" 0 "$(printf 'grant issued zdr=false\n' | count_hits "$DOMAIN_PATTERN")"
  check "env_value reads a name" "production" "$(printf 'A=1\nENVIRONMENT=production\n' | env_value ENVIRONMENT)"
  check "env_value of an absent name is empty" "" "$(printf 'A=1\n' | env_value ENVIRONMENT)"
  check "env_value does not match a longer name" "" "$(printf 'XENVIRONMENT=prod\n' | env_value ENVIRONMENT)"
  check "the registry check passes the real registry" ok "$(registry_ok < "$REPO/apps/Control Plane/config/plane-service-principals.json")"
  check "the registry check refuses a registry without the row" bad \
    "$(printf '{"quarry-edge":{"scopesByAudience":{"data-plane":["data:read"]}}}' | registry_ok)"
  check "the registry check refuses browser:write" bad \
    "$(printf '{"quarry-edge":{"scopesByAudience":{"browser-broker":["browser:read"]}},"x":{"scopes":["browser:write"]}}' | registry_ok)"
  check "the registry check says unreadable for garbage" unreadable "$(printf 'not json' | registry_ok)"
  [ "$bad" -eq 0 ] && log "self-test passed" || { log "self-test FAILED"; return 1; }
}

# ---- preflight (read-only) ---------------------------------------------------------

container_env() { # <container>: the environment, one KEY=VALUE per line
  docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$1" 2>/dev/null
}

expect_env() { # <label> <container> <name> <wanted>
  local got
  got="$(container_env "$2" | env_value "$3")"
  if [ "$got" = "$4" ]; then log "  ok   $1: $3=$4"; else fail "$1: $3 is '${got:-<unset>}', want '$4' (container $2)"; fi
}

preflight() {
  log "preflight (read-only)"
  command -v docker >/dev/null || { fail "docker is not on PATH"; return; }
  local c
  for c in "$QUARRY_CONTAINER" "$BROKER_CONTAINER" "$MODEL_GATEWAY_CONTAINER" "$GATEWAY_CONTAINER" "$AUTH_CONTAINER"; do
    if [ "$(docker inspect -f '{{.State.Running}}' "$c" 2>/dev/null || echo false)" = true ]; then
      log "  ok   running: $c"
    else
      fail "container $c is not running (boot the stack, or set the container-name variables)"
    fi
  done
  [ "$FAILED" -eq 0 ] || return 0

  # quarry-edge is in production posture, proved from its configuration.
  expect_env "quarry-edge" "$QUARRY_CONTAINER" ENVIRONMENT production
  expect_env "quarry-edge" "$QUARRY_CONTAINER" QUARRY_EDGE_AUTH_DEV_BYPASS 0
  expect_env "quarry-edge" "$QUARRY_CONTAINER" QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS 0
  expect_env "quarry-edge" "$QUARRY_CONTAINER" QUARRY_BROWSER_NO_SANDBOX 0
  expect_env "quarry-edge" "$QUARRY_CONTAINER" QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL "$BROKER_GRPC"
  expect_env "quarry-edge" "$QUARRY_CONTAINER" QUARRY_EDGE__DURABLE_EVENT_HISTORY 1
  # The broker's grant TTL is the one the expiry leg waits for.
  expect_env "broker" "$BROKER_CONTAINER" BROWSER_BROKER_GRANT_TTL_SECONDS "$GRANT_TTL_SECONDS"
  # The gateway's grant enforcement is strict on this stack.
  expect_env "gateway" "$GATEWAY_CONTAINER" BROWSER_GRANT_ENFORCEMENT strict

  # The hardening of the production overlay survived the contract overlay: the
  # running container, not the files (the files are tested by
  # scripts/tests/grant-contract-overlay.test.mjs).
  local ro cap
  ro="$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$QUARRY_CONTAINER" 2>/dev/null || true)"
  cap="$(docker inspect -f '{{join .HostConfig.CapDrop ","}}' "$QUARRY_CONTAINER" 2>/dev/null || true)"
  [ "$ro" = true ] && log "  ok   quarry-edge root filesystem is read-only" || fail "quarry-edge root filesystem is not read-only"
  case "$cap" in *ALL*) log "  ok   quarry-edge drops all capabilities";; *) fail "quarry-edge cap_drop is '$cap', want ALL";; esac
  local ports
  ports="$(docker inspect -f '{{len .HostConfig.PortBindings}}' "$QUARRY_CONTAINER" 2>/dev/null || echo '?')"
  [ "$ports" = 0 ] && log "  ok   quarry-edge publishes no host port" || fail "quarry-edge publishes $ports port binding(s)"

  # WS3.A: no Chromium runs with --no-sandbox.
  local nosandbox
  nosandbox="$(docker exec "$QUARRY_CONTAINER" sh -c 'ps -eo args | grep -c -- --no-sandbox' 2>/dev/null || true)"
  # grep counts its own sh -c line; zero running browsers shows 0 or 1 (the probe itself).
  case "${nosandbox:-0}" in 0|1) log "  ok   no Chromium started with --no-sandbox (count $nosandbox, the probe itself included)";;
    *) fail "Chromium runs with --no-sandbox ($nosandbox processes)";; esac

  # auth-core serves the registry row, from the file it actually loaded.
  local verdict
  verdict="$(docker exec "$AUTH_CONTAINER" sh -c 'if [ -n "$PLANE_SERVICE_PRINCIPALS_FILE" ]; then cat "$PLANE_SERVICE_PRINCIPALS_FILE"; else printf "%s" "$PLANE_SERVICE_PRINCIPALS_JSON"; fi' 2>/dev/null | registry_ok || true)"
  [ "$verdict" = ok ] && log "  ok   auth-core registry: quarry-edge holds browser:read on browser-broker, no browser:write anywhere" \
    || fail "auth-core registry check says '${verdict:-none}': rebuild auth-core with the registry row (--boot)"
}

# ---- the tunnel and the spec -------------------------------------------------------

start_tunnel() {
  if [ -n "${QUARRY_EDGE_URL:-}" ]; then log "using QUARRY_EDGE_URL as given; no tunnel"; return 0; fi
  local network
  network="$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' "$QUARRY_CONTAINER" | tr ' ' '\n' | grep -m1 '^inter-plane-bus$' || true)"
  [ -n "$network" ] || { fail "quarry-edge is not on inter-plane-bus; cannot open a tunnel"; return 1; }
  docker rm -f "$TUNNEL_NAME" >/dev/null 2>&1 || true
  # shellcheck disable=SC2086
  MSYS_NO_PATHCONV=1 $DOCKER_LOCK docker run -d --name "$TUNNEL_NAME" --network "$network" \
    -p "127.0.0.1:${TUNNEL_PORT}:${TUNNEL_PORT}" "$TUNNEL_IMAGE" \
    "TCP-LISTEN:${TUNNEL_PORT},fork,reuseaddr" "TCP:${QUARRY_CONTAINER}:8082" >/dev/null
  export QUARRY_EDGE_URL="http://127.0.0.1:${TUNNEL_PORT}"
  local i
  for i in $(seq 1 20); do
    if [ "$(curl -s -o /dev/null -w '%{http_code}' "$QUARRY_EDGE_URL/health" || true)" = 200 ]; then
      log "tunnel to quarry-edge is up on $QUARRY_EDGE_URL"; return 0
    fi
    sleep 1
  done
  fail "the tunnel to quarry-edge did not answer /health"; return 1
}

stop_tunnel() { docker rm -f "$TUNNEL_NAME" >/dev/null 2>&1 || true; }

JSON_OUT=""
# run_phase <tag>: the spec's tests carrying the tag; returns playwright's exit code.
run_phase() {
  local tag="$1" rc=0
  JSON_OUT="$(mktemp)"
  log "---- playwright phase $tag"
  (cd "$SPA" && BROWSER_GRANT_PROD_CONTRACT=1 PLAYWRIGHT_JSON_OUTPUT_NAME="$JSON_OUT" \
    CONTRACT_GRANT_TTL_SECONDS="$GRANT_TTL_SECONDS" \
    pnpm exec playwright test --project=local tests/e2e/browser-grants-prod-contract.spec.ts \
    --grep "$tag" --reporter=list,json) || rc=$?
  # What the expected-failure legs observed (status and message only; the spec writes no body).
  node -e '
    const fs = require("fs");
    try {
      const report = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
      const walk = (suite) => { for (const s of suite.suites || []) walk(s);
        for (const spec of suite.specs || []) for (const t of spec.tests || [])
          for (const a of t.annotations || []) if (a.type === "observed") console.log("  observed: " + a.description); };
      for (const s of report.suites || []) walk(s);
    } catch { /* no json report */ }' "$JSON_OUT" || true
  rm -f "$JSON_OUT"
  return "$rc"
}

# scan_hops <since> <fatal 1|0>: failure patterns on all five hops, domain strings on three.
scan_hops() {
  local since="$1" fatal="$2" hop container hits any=0
  log "---- per-hop log scan since $since (counts only)"
  for hop in gateway:"$GATEWAY_CONTAINER" model-gateway:"$MODEL_GATEWAY_CONTAINER" \
             execution-core:"$EXECUTION_CORE_CONTAINER" quarry-edge:"$QUARRY_CONTAINER" browser-broker:"$BROKER_CONTAINER"; do
    container="${hop#*:}"
    if ! docker inspect "$container" >/dev/null 2>&1; then log "  skip ${hop%%:*}: no container $container"; continue; fi
    hits="$(docker logs --since "$since" "$container" 2>&1 | count_hits "$FAILURE_PATTERN")"
    if [ "$hits" -eq 0 ]; then log "  ok   ${hop%%:*}: 0 refusals"; else any=1; log "  HIT  ${hop%%:*}: $hits line(s) match the refusal patterns"; fi
  done
  for hop in browser-broker:"$BROKER_CONTAINER" model-gateway:"$MODEL_GATEWAY_CONTAINER" gateway:"$GATEWAY_CONTAINER"; do
    container="${hop#*:}"
    docker inspect "$container" >/dev/null 2>&1 || continue
    hits="$(docker logs --since "$since" "$container" 2>&1 | count_hits "$DOMAIN_PATTERN")"
    if [ "$hits" -eq 0 ]; then log "  ok   ${hop%%:*}: 0 host names in the log"; else any=1; log "  HIT  ${hop%%:*}: $hits line(s) carry a host name"; fi
  done
  if [ "$any" -ne 0 ] && [ "$fatal" = 1 ]; then fail "the log scan found refusals or host names (see the hops above)"; fi
  return 0
}

# ---- boot (the repo's own recipes, in order) ---------------------------------------

boot() {
  log "boot: rebuilding auth-core, broker, model-gateway, quarry-edge and the gateway image"
  local revision date
  revision="$(git -C "$REPO" rev-parse HEAD)"; date="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  # 1. auth-core with the registry row.
  bash "$REPO/apps/Control Plane/scripts/run-control-plane.sh" up -d --no-deps --build auth-core
  # 2. broker (45 s grants) and model-gateway.
  BROWSER_BROKER_GRANT_TTL_SECONDS="$GRANT_TTL_SECONDS" \
    bash "$REPO/apps/Model Plane/scripts/compose.sh" up -d --no-deps --build browser-broker model-gateway
  # 3. quarry-edge on the base, the production overlay and the contract overlay, with the
  #    repo's env-file chain (the order is the ingestion-plane deploy recipe's).
  local files=() f
  for f in "$ENV_ROOT/.env" \
    "$ENV_ROOT/apps/Control Plane/.env.generated-secrets" "$ENV_ROOT/apps/Application Plane/.env.generated-secrets" \
    "$ENV_ROOT/apps/Ingestion Plane/Quarry-v2/.env" "$ENV_ROOT/apps/Ingestion Plane/autocomplete-core/.env" \
    "$ENV_ROOT/apps/Ingestion Plane/finspo-core/.env" "$ENV_ROOT/apps/Ingestion Plane/imports-core/.env" \
    "$ENV_ROOT/apps/Ingestion Plane/integration-corev2/.env" "$ENV_ROOT/apps/Ingestion Plane/shipping-core/.env" \
    "$ENV_ROOT/apps/Ingestion Plane/.env.generated-secrets"; do
    if [ -f "$f" ]; then files+=(--env-file "$f"); else log "  note: no env file $f"; fi
  done
  # shellcheck disable=SC2086
  SOURCE_REVISION="$revision" BUILD_DATE="$date" ENVIRONMENT=production \
    QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL="$BROKER_GRPC" \
    $DOCKER_LOCK docker compose -p ingestion-plane "${files[@]}" \
      -f "$PLANE/docker-compose.yml" -f "$PLANE/docker-compose.production.yml" -f "$PLANE/docker-compose.grant-contract.yml" \
      up -d --no-deps --build quarry-edge
  # 4. the gateway image, with strict enforcement.
  local swap="${GATEWAY_SWAP_TOOL:-}"
  if [ -n "$swap" ]; then
    BROWSER_GRANT_ENFORCEMENT=strict python3 "$swap"
  else
    log "  note: GATEWAY_SWAP_TOOL is not set; swap the gateway image yourself with BROWSER_GRANT_ENFORCEMENT=strict (the preflight checks it)"
  fi
  log "boot: waiting for quarry-edge to be healthy"
  local i
  for i in $(seq 1 60); do
    [ "$(docker inspect -f '{{.State.Health.Status}}' "$QUARRY_CONTAINER" 2>/dev/null || true)" = healthy ] && return 0
    sleep 5
  done
  fail "quarry-edge did not become healthy after the boot"
}

# ---- main ---------------------------------------------------------------------------

main() {
  local do_boot=0 yes=0 preflight_only=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --self-test) self_test; return $? ;;
      --boot) do_boot=1 ;;
      --yes-rebuild-stack) yes=1 ;;
      --preflight-only) preflight_only=1 ;;
      -h|--help) sed -n '2,45p' "${BASH_SOURCE[0]}"; return 0 ;;
      *) printf 'unknown argument: %s\n' "$1" >&2; return 2 ;;
    esac
    shift
  done
  if [ "$do_boot" = 1 ]; then
    [ "$yes" = 1 ] || { printf -- '--boot rebuilds live containers; add --yes-rebuild-stack to confirm\n' >&2; return 2; }
    boot
  fi
  preflight
  [ "$FAILED" -eq 0 ] || { log "preflight FAILED: nothing was run"; return 1; }
  [ "$preflight_only" = 0 ] || { log "preflight passed"; return 0; }

  trap stop_tunnel EXIT
  start_tunnel || return 1

  local t0 t1 rc=0
  t0="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run_phase '@happy' || { fail "phase @happy failed (legs A1 to A4)"; }
  scan_hops "$t0" 1

  t1="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run_phase '@legBC' || { fail "phase @legBC failed (a test.fail() leg passed unexpectedly, or a green leg failed)"; }
  scan_hops "$t1" "${CONTRACT_BC_GREEN:-0}"

  run_phase '@negative' || { fail "phase @negative failed (legs A5 and A6)"; }

  if [ "$FAILED" -eq 0 ]; then log "RESULT: PASS"; else log "RESULT: FAIL"; rc=1; fi
  return "$rc"
}

main "$@"
