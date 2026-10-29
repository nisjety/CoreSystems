#!/usr/bin/env bash
# start-stack.sh — bring up the full Quarry-v2 dev stack and verify the
# end-to-end live-crawl path that verevon's onboarding wizard depends on.
#
# What this does:
#   1. Sanity-check Docker is running.
#   2. `docker compose up --build -d` for postgres, redis, temporal,
#      quarry-control, quarry-edge, quarry-orchestrator.
#   3. Wait for /health on quarry-control (:8081) and quarry-edge (:8080).
#   4. Smoke-probe: POST a real `crawl` job for `https://example.com`,
#      poll /v1/jobs/{id}/events for a `page_fetched` (and ideally a
#      `branding_extracted`) event arriving within the deadline.
#   5. Report go/no-go with concrete next steps.
#
# Why it exists: the chain (verevon → control → orchestrator dispatcher
# → Temporal → edge runtime → control events → verevon) has three places
# where it can quietly drop work — control's auth, edge's auth, and the
# jobs dispatcher's polling cadence. The smoke probe at the end is what
# tells you whether all three are wired correctly.
#
# Usage:
#   ./scripts/start-stack.sh             # full bring-up + smoke
#   ./scripts/start-stack.sh --no-smoke  # bring-up only
#   ./scripts/start-stack.sh --tail      # bring-up + tail combined logs
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$REPO_ROOT/deploy/compose/docker-compose.yml"

CONTROL_URL="${CONTROL_URL:-http://localhost:8081}"
EDGE_URL="${EDGE_URL:-http://localhost:8080}"
CONTROL_TOKEN="${CONTROL_AUTH_TOKEN:-devsecret}"
SMOKE_URL="${SMOKE_URL:-https://example.com}"
SMOKE_TIMEOUT_S="${SMOKE_TIMEOUT_S:-60}"

DO_SMOKE=1
DO_TAIL=0

for arg in "$@"; do
    case "$arg" in
        --no-smoke) DO_SMOKE=0 ;;
        --tail)     DO_TAIL=1 ;;
        -h|--help)
            sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown arg: $arg" >&2
            exit 2
            ;;
    esac
done

c_red()    { printf '\033[31m%s\033[0m\n' "$1"; }
c_green()  { printf '\033[32m%s\033[0m\n' "$1"; }
c_yellow() { printf '\033[33m%s\033[0m\n' "$1"; }
c_dim()    { printf '\033[2m%s\033[0m\n' "$1"; }

step() { c_yellow "▶ $1"; }
ok()   { c_green  "  ✓ $1"; }
fail() { c_red    "  ✗ $1"; }

# --- 1. Sanity checks ------------------------------------------------------

step "Checking prerequisites"
if ! command -v docker >/dev/null 2>&1; then
    fail "docker not on PATH"; exit 1
fi
if ! docker info >/dev/null 2>&1; then
    fail "docker daemon not running (start Docker Desktop?)"; exit 1
fi
ok "docker daemon ready"

if [ ! -f "$COMPOSE_FILE" ]; then
    fail "compose file not found: $COMPOSE_FILE"; exit 1
fi
ok "compose file: $COMPOSE_FILE"

# --- 2. Bring up the stack -------------------------------------------------

step "Building images and starting services (this can take 10–15 min on first build)"
docker compose -f "$COMPOSE_FILE" up --build -d
ok "compose up -d issued"

# --- 3. Wait for health ----------------------------------------------------

wait_for_url() {
    local name="$1" url="$2" budget="$3"
    local started elapsed
    started=$(date +%s)
    while :; do
        if curl -fsS -o /dev/null --max-time 2 "$url" >/dev/null 2>&1; then
            ok "$name reachable ($url)"
            return 0
        fi
        elapsed=$(( $(date +%s) - started ))
        if [ "$elapsed" -ge "$budget" ]; then
            fail "$name not reachable after ${budget}s ($url)"
            c_dim   "    last logs (50 lines):"
            docker compose -f "$COMPOSE_FILE" logs --tail=50 "$name" 2>/dev/null | sed 's/^/    /'
            return 1
        fi
        sleep 2
    done
}

step "Waiting for service health (90s budget each)"
wait_for_url quarry-control "$CONTROL_URL/health" 90 || exit 1
wait_for_url quarry-edge    "$EDGE_URL/health"    90 || exit 1

# --- 4. Smoke probe --------------------------------------------------------

if [ "$DO_SMOKE" -eq 0 ]; then
    ok "skipping smoke (--no-smoke)"
    exit 0
fi

step "Smoke probe — POST a real crawl job and watch for events"
SMOKE_BODY=$(cat <<JSON
{"kind":"crawl","params":{"url":"$SMOKE_URL","max_pages":4,"max_depth":1,"auto_commit":true}}
JSON
)
JOB_RESP=$(curl -fsS -X POST "$CONTROL_URL/v1/jobs/" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "$SMOKE_BODY") || { fail "POST /v1/jobs/ failed"; exit 1; }

JOB_ID=$(printf '%s' "$JOB_RESP" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
if [ -z "$JOB_ID" ]; then
    fail "could not parse job id from response: $JOB_RESP"; exit 1
fi
ok "job created: $JOB_ID"
c_dim "    (orchestrator's jobs dispatcher polls every 2s; expect dispatch within ~3s)"

START_S=$(date +%s)
SAW_PAGE=0
SAW_BRANDING=0
AFTER_SEQ=0
while :; do
    ELAPSED=$(( $(date +%s) - START_S ))
    if [ "$ELAPSED" -ge "$SMOKE_TIMEOUT_S" ]; then
        break
    fi
    EVENTS=$(curl -fsS --max-time 3 \
        "$CONTROL_URL/v1/jobs/$JOB_ID/events?after_seq=$AFTER_SEQ&limit=100" 2>/dev/null || echo "[]")
    # Cheap parsing — grep instead of jq because we don't want a hard
    # jq dependency on the dev box. The seq is monotonic so we can pick
    # the largest one with sort -n.
    NEW_SEQ=$(printf '%s' "$EVENTS" | grep -oE '"seq":[0-9]+' | sed 's/"seq"://' | sort -n | tail -1)
    if [ -n "$NEW_SEQ" ] && [ "$NEW_SEQ" -gt "$AFTER_SEQ" ]; then
        AFTER_SEQ="$NEW_SEQ"
    fi
    if printf '%s' "$EVENTS" | grep -q '"type":"page_fetched"'; then
        SAW_PAGE=1
    fi
    if printf '%s' "$EVENTS" | grep -q '"type":"branding_extracted"'; then
        SAW_BRANDING=1
    fi
    if [ "$SAW_PAGE" -eq 1 ] && [ "$SAW_BRANDING" -eq 1 ]; then
        break
    fi
    sleep 1
done

# --- 5. Report -------------------------------------------------------------

step "Smoke result"
if [ "$SAW_PAGE" -eq 1 ]; then
    ok "page_fetched event observed"
else
    fail "no page_fetched event within ${SMOKE_TIMEOUT_S}s"
fi
if [ "$SAW_BRANDING" -eq 1 ]; then
    ok "branding_extracted event observed"
else
    fail "no branding_extracted event within ${SMOKE_TIMEOUT_S}s (page_fetched alone is still OK for the snippet drop)"
fi

if [ "$SAW_PAGE" -eq 1 ]; then
    c_green ""
    c_green "✅ live crawl works. Hit POST $CONTROL_URL/v1/jobs/ from verevon to drive the onboarding."
    c_green "   Verevon's QUARRY_API_URL is already set to http://localhost:8081 in .env.local."
    c_dim "   logs:    docker compose -f $COMPOSE_FILE logs -f"
    c_dim "   stop:    docker compose -f $COMPOSE_FILE down -v"
    if [ "$DO_TAIL" -eq 1 ]; then
        echo
        c_yellow "▶ Tailing combined logs (Ctrl-C to detach; stack keeps running)"
        docker compose -f "$COMPOSE_FILE" logs -f
    fi
    exit 0
fi

# Failure path — best-effort diagnostics.
c_red ""
c_red "❌ live crawl did NOT produce a page_fetched event."
c_dim ""
c_dim "Recent orchestrator + edge logs (likely culprits):"
docker compose -f "$COMPOSE_FILE" logs --tail=30 quarry-orchestrator 2>/dev/null | sed 's/^/  /'
echo
docker compose -f "$COMPOSE_FILE" logs --tail=30 quarry-edge 2>/dev/null | sed 's/^/  /'
echo
c_dim "Common causes:"
c_dim "  - Temporal still booting (it has its own postgres init dance)"
c_dim "  - Edge auth rejecting orchestrator's bearer (check QUARRY_EDGE_AUTH_DEV_BYPASS=1)"
c_dim "  - Control rejecting event POSTs (check QUARRY_CONTROL_API_KEY matches CONTROL_AUTH_TOKEN)"
c_dim "  - Outbound network blocked from inside the container (try with --network host)"
exit 1
