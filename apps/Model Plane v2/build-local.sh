#!/usr/bin/env bash
# build-local.sh — Build all Model Plane v2 images without pulling from Docker Hub.
# Uses 'docker commit' strategy on top of agent-core-v2:test (Python 3.12 base).
# Run from the Model Plane v2 directory.

set -euo pipefail

BASE_IMAGE="agent-core-v2:test"
SHARED_PKG="./shared/reasoning_runtime"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

log()  { echo -e "${BLUE}[build]${NC} $*"; }
ok()   { echo -e "${GREEN}[✓]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
fail() { echo -e "${RED}[✗]${NC} $*"; exit 1; }

cd "$SCRIPT_DIR"

[[ -d $SHARED_PKG ]] || fail "shared/reasoning_runtime not found — run from Model Plane v2 dir"
docker image inspect "$BASE_IMAGE" &>/dev/null || fail "Base image $BASE_IMAGE not found"

# ------------------------------------------------------------------
# Helper: build an image via docker-commit
#   build_image <target_image_name> <service_dir> <extra_pip_args...>
# ------------------------------------------------------------------
build_service() {
  local IMAGE_NAME="$1"
  local SERVICE_DIR="$2"
  local INSTALL_RUNTIME="${3:-no}"    # "yes" = install reasoning_runtime
  local PORT="${4:-8001}"
  local ENTRY="${5:-./docker-entrypoint.sh}"
  shift 5
  local EXTRA_PKGS=("$@")            # any additional pip packages

  local CTR="build-${IMAGE_NAME//[:\/ ]/-}-$$"
  log "Building $IMAGE_NAME from $SERVICE_DIR (port $PORT)..."

  # Start root-accessible builder container
  docker run -d --user root --name "$CTR" --entrypoint sleep "$BASE_IMAGE" 3600 >/dev/null

  # ---- trap to always clean up ----
  cleanup() { docker rm -f "$CTR" &>/dev/null || true; }
  trap cleanup EXIT

  # Clear old app code from base image
  docker exec --user root "$CTR" rm -rf /app/*

  # Install reasoning_runtime from local source
  if [[ $INSTALL_RUNTIME == "yes" ]]; then
    log "  Installing reasoning_runtime..."
    docker cp "$SHARED_PKG/." "$CTR":/tmp/reasoning_runtime/
    docker exec --user root "$CTR" pip install --no-cache-dir /tmp/reasoning_runtime
  fi

  # Install service-specific requirements
  if [[ -f "$SERVICE_DIR/requirements.txt" ]]; then
    log "  Installing service requirements..."
    docker cp "$SERVICE_DIR/requirements.txt" "$CTR":/tmp/requirements.txt
    docker exec --user root "$CTR" pip install --no-cache-dir -r /tmp/requirements.txt
  fi

  # Install any extra pip packages
  if [[ ${#EXTRA_PKGS[@]} -gt 0 ]]; then
    log "  Installing extra packages: ${EXTRA_PKGS[*]}"
    docker exec --user root "$CTR" pip install --no-cache-dir "${EXTRA_PKGS[@]}"
  fi

  # Copy the app code
  docker cp "$SERVICE_DIR/app/." "$CTR":/app/app/

  # Copy and fix entrypoint
  if [[ -f "$SERVICE_DIR/docker-entrypoint.sh" ]]; then
    docker cp "$SERVICE_DIR/docker-entrypoint.sh" "$CTR":/app/docker-entrypoint.sh
    docker exec --user root "$CTR" chmod +x /app/docker-entrypoint.sh
  fi

  # Commit with proper metadata
  docker commit \
    --change="WORKDIR /app" \
    --change="USER appuser" \
    --change="ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 PORT=$PORT" \
    --change="EXPOSE $PORT" \
    --change="ENTRYPOINT [\"$ENTRY\"]" \
    "$CTR" "$IMAGE_NAME" >/dev/null

  trap - EXIT
  cleanup
  ok "Built $IMAGE_NAME"
}

# ------------------------------------------------------------------
# Build all 5 services
# ------------------------------------------------------------------

log "=== Model Plane v2 — Local Build ==="
echo ""

# 1. ai-core (port 8001) — needs reasoning_runtime + azure + deepgram
log "--- ai-core ---"
build_service \
  "model-plane-v2-ai-core:latest" \
  "./ai-core" \
  "yes" \
  "8001" \
  "./docker-entrypoint.sh" \
  "azure-ai-contentsafety>=1.0.0" \
  "deepgram-sdk>=3.7" \
  "azure-cognitiveservices-speech>=1.41.0" \
  "structlog>=24.0,<25"

echo ""

# 2. llm-worker (port 8005) — needs reasoning_runtime only
log "--- llm-worker ---"
build_service \
  "model-plane-v2-llm-worker:latest" \
  "./llm-worker" \
  "yes" \
  "8005" \
  "./docker-entrypoint.sh"

echo ""

# 3. agent-core-v2 (port 8002) — needs reasoning_runtime + heavy deps
log "--- agent-core-v2 ---"
build_service \
  "model-plane-v2-agent-core-v2:latest" \
  "./agent-core" \
  "yes" \
  "8002" \
  "./docker-entrypoint.sh"

echo ""

# 4. execution-core-v2 (port 8003) — no reasoning_runtime
log "--- execution-core-v2 ---"
build_service \
  "model-plane-v2-execution-core-v2:latest" \
  "./execution-core" \
  "no" \
  "8003" \
  "./docker-entrypoint.sh"

echo ""

# 5. capability-core-v2 (port 8004) — no reasoning_runtime
log "--- capability-core-v2 ---"
build_service \
  "model-plane-v2-capability-core-v2:latest" \
  "./capability-core" \
  "no" \
  "8004" \
  "./docker-entrypoint.sh"

echo ""
ok "All images built successfully!"
echo ""
log "Available images:"
docker image ls | grep -E "model-plane-v2|agent-core-v2|IMAGE ID" | head -20
echo ""
log "Next: docker compose up -d (infrastructure first, then services)"
