#!/usr/bin/env bash
set -Eeuo pipefail

# Velion service builder for the core server planes.
#
# Subcommands:
#   ./build-velion-services.sh                Build/start every plane (default).
#   ./build-velion-services.sh --dry-run      Validate compose files only; no build/start.
#   ./build-velion-services.sh --compose-bootstrap
#                                              Called by velionv2 docker-compose.
#                                              Builds core planes only when they
#                                              are not already ready.
#   ./build-velion-services.sh --prune        Stop + remove every plane's containers,
#                                              volumes, and networks. Use before a
#                                              clean rebuild. Idempotent.
#   ./build-velion-services.sh --status       Per-plane health roll-up. Read-only.
#
# Environment knobs:
#   CORE_ROOT_OVERRIDE=/path/to/CoreSystem
#   WAIT_TIMEOUT_SECONDS=900   Max seconds to wait for one-shot services.
#   WAIT_INTERVAL_SECONDS=3    Poll interval while waiting for one-shot services.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE_ROOT="${CORE_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/../../.." && pwd)}"
cd "$CORE_ROOT"

MODE="build"
DRY_RUN=false
while (( $# > 0 )); do
  case "$1" in
    --dry-run) DRY_RUN=true; shift ;;
    --compose-bootstrap) MODE="compose-bootstrap"; shift ;;
    --prune)   MODE="prune"; shift ;;
    --status)  MODE="status"; shift ;;
    --help|-h)
      sed -n '3,18p' "$0"
      exit 0
      ;;
    *)
      printf 'Usage: %s [--dry-run] [--compose-bootstrap|--prune|--status]\n' "$0" >&2
      exit 2
      ;;
  esac
done

if [[ "$MODE" != "build" && "$MODE" != "compose-bootstrap" && "$DRY_RUN" == "true" ]]; then
  printf 'Usage: %s [--dry-run] [--compose-bootstrap]\n' "$0" >&2
  exit 2
fi

# These are intentionally ordered by dependency flow:
# Data and Ingestion first, then Model, Control, Application (Convex + Novu +
# Affine), and finally Frontend Velion (which subscribes to Convex).
COMPOSE_FILES=(
  "apps/Data Plane v2/docker-compose.yml"
  "apps/Ingestion Plane/docker-compose.yml"
  "apps/Model Plane/deploy/docker-compose.yml"
  "apps/Control Plane/docker-compose.yml"
  "apps/Application Plane/docker-compose.yml"
  "apps/Frontend Plane/velionv2/docker-compose.yml"
)

STACK_NAMES=(
  "Data Plane v2"
  "Ingestion Plane"
  "Model Plane"
  "Control Plane"
  "Application Plane"
  "Frontend Plane Velion v2"
)

# One-shot services are removed after they exit successfully so `docker ps -a`
# stays focused on long-running servers.
BOOTSTRAP_SERVICES=(
  "minio-init"
  "nango-seed"
  ""
  "lago-migrate"
  "affine-runtime-migration"
  ""
)
# Model Plane (index 2) historically had three one-shots
# (capability-migrations, minio-bootstrap, temporal-bootstrap) — they were
# removed from `apps/Model Plane/deploy/docker-compose.yml` when the
# corresponding setup moved into the runtime containers themselves
# (temporal uses temporalio/auto-setup; capability-core/minio bootstrap
# inline on first start). Leave the slot empty so `docker compose rm` does
# not error on names that no longer exist.

# Model Plane used to be derived from the `deploy` folder name. Stop that old
# project before starting the explicit `model-plane` project to avoid port and
# container-name collisions.
#
# The Frontend slot now stops the legacy `frontend-plane-velion` project
# (velion v1) before standing up `frontend-plane-velionv2` so the two cannot
# fight over host port 3000 or the `velion-nats` container name.
OLD_PROJECTS=(
  ""
  ""
  "deploy"
  ""
  ""
  "frontend-plane-velion"
)

# Per-stack post-build hooks (run after one-shots clear, before moving to the
# next stack). Keep entries short — long hooks belong in their own function.
# Format: comma-separated function names; empty string skips.
POST_BUILD_HOOKS=(
  ""
  "ensure_finspo_database"
  ""
  ""
  "deploy_convex_functions"
  ""
)

FRONTEND_STACK_INDEX=5
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-900}"
WAIT_INTERVAL_SECONDS="${WAIT_INTERVAL_SECONDS:-3}"

log() {
  printf '\n[%s] %s\n' "$(date '+%H:%M:%S')" "$*"
}

run() {
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] %q' "$1"
    shift
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  "$@"
}

ensure_velion_network() {
  # Compose files across all planes declare `inter-plane-bus` as an external
  # network. Older versions of this script created `velion-net`, which caused
  # `docker compose up` to fail with "network inter-plane-bus declared as
  # external, but could not be found". The shared bus is `inter-plane-bus`.
  if docker network inspect inter-plane-bus >/dev/null 2>&1; then
    return 0
  fi

  log "Creating shared Docker network: inter-plane-bus"
  run docker network create inter-plane-bus >/dev/null
}

validate_compose() {
  local compose_file="$1"
  docker compose -f "$compose_file" config --quiet
}

validate_ingestion_plane_targets_quarry_v2() {
  local compose_file="$1"
  local services

  services="$(docker compose -f "$compose_file" config --services)"

  for service in quarry-edge quarry-control quarry-orchestrator; do
    if ! grep -qx "$service" <<<"$services"; then
      printf 'Ingestion Plane must target Quarry-v2; missing service: %s\n' "$service" >&2
      return 1
    fi
  done

  if grep -qx "quarry-api" <<<"$services"; then
    printf 'Ingestion Plane points at deferred legacy Quarry service: quarry-api\n' >&2
    return 1
  fi
}

compose_services() {
  local compose_file="$1"
  docker compose -f "$compose_file" config --services
}

service_in_list() {
  local needle="$1"
  local list="$2"

  [[ " $list " == *" $needle "* ]]
}

runtime_services() {
  local compose_file="$1"
  local bootstrap_services="$2"
  local service

  while IFS= read -r service; do
    [[ -z "$service" ]] && continue
    if service_in_list "$service" "$bootstrap_services"; then
      continue
    fi
    printf '%s\n' "$service"
  done < <(compose_services "$compose_file")
}

service_ready() {
  local compose_file="$1"
  local service="$2"
  local container_id status health

  container_id="$(docker compose -f "$compose_file" ps -q "$service" 2>/dev/null || true)"
  [[ -n "$container_id" ]] || return 1

  status="$(docker inspect -f '{{.State.Status}}' "$container_id" 2>/dev/null || true)"
  [[ "$status" == "running" ]] || return 1

  health="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{end}}' "$container_id" 2>/dev/null || true)"
  [[ -z "$health" || "$health" == "healthy" ]]
}

stack_runtime_ready() {
  local index="$1"
  local compose_file="${COMPOSE_FILES[$index]}"
  local bootstrap_services="${BOOTSTRAP_SERVICES[$index]}"
  local service
  local total=0

  while IFS= read -r service; do
    [[ -z "$service" ]] && continue
    total=$((total + 1))
    if ! service_ready "$compose_file" "$service"; then
      return 1
    fi
  done < <(runtime_services "$compose_file" "$bootstrap_services")

  (( total > 0 ))
}

core_stacks_ready() {
  local index

  for ((index = 0; index < FRONTEND_STACK_INDEX; index++)); do
    if ! stack_runtime_ready "$index"; then
      return 1
    fi
  done
}

ensure_frontend_bus() {
  local compose_file="${COMPOSE_FILES[$FRONTEND_STACK_INDEX]}"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  log "Ensuring Frontend Plane NATS bus is running"
  validate_compose "$compose_file"
  run docker compose -f "$compose_file" up -d --build --remove-orphans nats

  if [[ "$DRY_RUN" == "true" ]]; then
    return 0
  fi

  while (( SECONDS < deadline )); do
    if service_ready "$compose_file" "nats"; then
      return 0
    fi
    sleep "$WAIT_INTERVAL_SECONDS"
  done

  docker compose -f "$compose_file" logs --tail=120 nats || true
  printf 'Timed out waiting for frontend NATS bus\n' >&2
  return 1
}

stop_old_project() {
  local compose_file="$1"
  local old_project="$2"

  if [[ -z "$old_project" ]]; then
    return 0
  fi

  if docker compose ls --all --format json | grep -q "\"Name\":\"$old_project\""; then
    log "Stopping old Compose project '$old_project' for $compose_file"
    run docker compose -p "$old_project" -f "$compose_file" down --remove-orphans
  fi
}

wait_for_one_shot() {
  local compose_file="$1"
  local service="$2"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  while (( SECONDS < deadline )); do
    local container_id
    container_id="$(docker compose -f "$compose_file" ps -q "$service" 2>/dev/null || true)"

    if [[ -z "$container_id" ]]; then
      return 0
    fi

    local state
    state="$(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' "$container_id" 2>/dev/null || true)"

    case "$state" in
      "exited 0")
        return 0
        ;;
      exited\ *)
        docker compose -f "$compose_file" logs --tail=120 "$service" || true
        printf 'One-shot service failed: %s (%s)\n' "$service" "$state" >&2
        return 1
        ;;
      "")
        return 0
        ;;
      *)
        sleep "$WAIT_INTERVAL_SECONDS"
        ;;
    esac
  done

  docker compose -f "$compose_file" logs --tail=120 "$service" || true
  printf 'Timed out waiting for one-shot service: %s\n' "$service" >&2
  return 1
}

remove_one_shot_containers() {
  local compose_file="$1"
  local services="$2"
  local service
  local existing=()

  if [[ -z "$services" ]]; then
    return 0
  fi

  for service in $services; do
    if ! compose_services "$compose_file" | grep -qx "$service"; then
      log "Skipping missing one-shot service: $service"
      continue
    fi
    wait_for_one_shot "$compose_file" "$service"
    existing+=("$service")
  done

  if (( ${#existing[@]} == 0 )); then
    return 0
  fi

  log "Removing completed one-shot services: ${existing[*]}"
  if [[ "$DRY_RUN" == "true" ]]; then
    run docker compose -f "$compose_file" rm -f -s -v "${existing[@]}"
  else
    docker compose -f "$compose_file" rm -f -s -v "${existing[@]}" >/dev/null
  fi
}

build_stack() {
  local index="$1"
  local name="${STACK_NAMES[$index]}"
  local compose_file="${COMPOSE_FILES[$index]}"
  local bootstrap_services="${BOOTSTRAP_SERVICES[$index]}"
  local old_project="${OLD_PROJECTS[$index]}"
  local post_hook="${POST_BUILD_HOOKS[$index]}"

  log "Validating $name"
  validate_compose "$compose_file"
  if [[ "$name" == "Ingestion Plane" ]]; then
    validate_ingestion_plane_targets_quarry_v2 "$compose_file"
  fi

  stop_old_project "$compose_file" "$old_project"

  log "Building and starting $name"
  if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
    run env VELION_SKIP_BOOTSTRAP=1 docker compose -f "$compose_file" up -d --build --remove-orphans frontend nats
  else
    run docker compose -f "$compose_file" up -d --build --remove-orphans
  fi

  if [[ "$DRY_RUN" == "false" ]]; then
    remove_one_shot_containers "$compose_file" "$bootstrap_services"

    log "$name status"
    docker compose -f "$compose_file" ps

    if [[ -n "$post_hook" ]]; then
      IFS=',' read -r -a hooks <<<"$post_hook"
      for hook in "${hooks[@]}"; do
        hook="$(printf '%s' "$hook" | xargs)"   # trim
        [[ -z "$hook" ]] && continue
        log "Running post-build hook for $name: $hook"
        "$hook"
      done
    fi
  fi
}

# ─────────────────────────────────────────────────────────────────────────
# Post-build hooks
# ─────────────────────────────────────────────────────────────────────────

read_env_value() {
  local file="$1"
  local key="$2"

  [[ -f "$file" ]] || return 0

  awk -F= -v key="$key" '
    $1 == key {
      value = substr($0, length(key) + 2)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
      gsub(/^"|"$/, "", value)
      gsub(/^'\''|'\''$/, "", value)
      print value
      exit
    }
  ' "$file"
}

# deploy_convex_functions — force-deploys the convex-core function registry
# against the running convex-backend container. Idempotent: when the
# backend already has the same code, `npx convex deploy` is a no-op.
#
# Rationale (velion ui-ux-velion-gap.md §10):
# Convex backend's SQLite registry can drift from the on-disk source when
# the container's volume is wiped or first brought up. Without this hook
# the dashboard hits "Could not find public function for 'controlSessions:
# byUser'" until something inside the convex/ directory mutates (which is
# the only thing `convex dev`'s file-watcher triggers on).
#
# The convex-gateway container's startup.sh ALSO runs `convex deploy` on
# every cold start — this build-script step belongs-and-suspenders the
# same outcome for the case where a CI / fresh-clone run invokes this
# script directly.
deploy_convex_functions() {
  local convex_dir="$CORE_ROOT/apps/Application Plane/convex-core"
  local backend_url="${CONVEX_SELF_HOSTED_URL:-http://localhost:3210}"

  if [[ ! -d "$convex_dir/convex" ]]; then
    printf '[convex-deploy] Skipping — %s/convex not found\n' "$convex_dir" >&2
    return 0
  fi

  local admin_key="${CONVEX_ADMIN_KEY:-${CONVEX_SELF_HOSTED_ADMIN_KEY:-}}"
  if [[ -z "$admin_key" ]] && [[ -f "$convex_dir/.env.local" ]]; then
    admin_key="$(read_env_value "$convex_dir/.env.local" "CONVEX_ADMIN_KEY")"
  fi
  if [[ -z "$admin_key" ]]; then
    printf '[convex-deploy] WARN: no CONVEX_ADMIN_KEY available; skipping deploy\n' >&2
    return 0
  fi

  # Wait for the backend to become reachable. 60 attempts × 2s = 2 min cap,
  # well inside the convex-backend healthcheck start_period window.
  local attempts=0
  while ! curl -sf "$backend_url/version" >/dev/null 2>&1; do
    attempts=$((attempts + 1))
    if (( attempts >= 60 )); then
      printf '[convex-deploy] WARN: backend %s did not respond after %d attempts; skipping deploy\n' "$backend_url" "$attempts" >&2
      return 0
    fi
    sleep 2
  done

  log "Deploying Convex functions to $backend_url"
  (
    cd "$convex_dir"
    mkdir -p .convex-tmp
    CONVEX_SELF_HOSTED_URL="$backend_url" \
    CONVEX_SELF_HOSTED_ADMIN_KEY="$admin_key" \
    CONVEX_AUTH_ISSUER="${CONVEX_AUTH_ISSUER:-$(read_env_value "$convex_dir/.env.local" "CONVEX_AUTH_ISSUER")}" \
    CONVEX_AUTH_JWKS_URL="${CONVEX_AUTH_JWKS_URL:-$(read_env_value "$convex_dir/.env.local" "CONVEX_AUTH_JWKS_URL")}" \
    CONVEX_AUTH_AUDIENCE="${CONVEX_AUTH_AUDIENCE:-$(read_env_value "$convex_dir/.env.local" "CONVEX_AUTH_AUDIENCE")}" \
    TMPDIR="$convex_dir/.convex-tmp" \
    npx convex deploy --yes
  ) || {
    printf '[convex-deploy] WARN: deploy returned non-zero; verify convex-gateway logs for the auto-deploy fallback\n' >&2
    return 0
  }
}

# ensure_finspo_database — idempotently CREATE DATABASE finspo on the
# running ingestion-postgres container.
#
# Why this hook exists:
#   `init-databases.sql` only fires on a fresh `ingestion-postgres-data`
#   volume. Hosts whose volume predates the finspo-core rollout never get
#   the database, and finspo-api crashes at startup with "database 'finspo'
#   does not exist". The embedded Go migration runner (internal/db.ApplyMigrations)
#   creates tables once the database exists — but it can't create the
#   database itself. This hook fills that gap on every build.
#
# Safe to re-run: the `WHERE NOT EXISTS` gate skips the CREATE when the
# database is already present.
ensure_finspo_database() {
  local container="ingestion-postgres"
  local user="${INGESTION_PG_USER:-ingestion_user}"
  local maintenance_db="${INGESTION_PG_DB:-ingestion_plane_db}"

  if ! docker ps --format '{{.Names}}' | grep -qx "$container"; then
    printf '[finspo-db] WARN: %s not running; skipping CREATE DATABASE finspo\n' "$container" >&2
    return 0
  fi

  # Wait for postgres to accept connections — the Ingestion Plane healthcheck
  # has already gated up, but the role might still be locking out new
  # connections during boot.
  local attempts=0
  while ! docker exec "$container" pg_isready -U "$user" -d "$maintenance_db" >/dev/null 2>&1; do
    attempts=$((attempts + 1))
    if (( attempts >= 30 )); then
      printf '[finspo-db] WARN: pg_isready never returned ok after %d attempts; skipping\n' "$attempts" >&2
      return 0
    fi
    sleep 2
  done

  log "Ensuring finspo database exists on $container"
  # \gexec runs the SELECT's output as the next SQL statement when (and only
  # when) the database is missing — making this a true no-op for fresh
  # volumes that already got finspo from init-databases.sql.
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] docker exec %s psql ... CREATE DATABASE finspo\n' "$container"
    return 0
  fi

  docker exec -i "$container" psql -v ON_ERROR_STOP=1 -U "$user" -d "$maintenance_db" <<'SQL' >/dev/null
SELECT 'CREATE DATABASE finspo
            WITH OWNER = ingestion_user
                 ENCODING = ''UTF8''
                 LC_COLLATE = ''en_US.utf8''
                 LC_CTYPE = ''en_US.utf8''
                 TEMPLATE = template0'
 WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'finspo')\gexec
SQL
}

# prune_all — tear down every plane's compose project, prune any
# straggling containers/volumes, and remove `inter-plane-bus`. Idempotent:
# safe to run when nothing is up.
prune_all() {
  log "Pruning all velion planes (containers, volumes, network)…"
  for index in "${!COMPOSE_FILES[@]}"; do
    local compose_file="${COMPOSE_FILES[$index]}"
    local stack_name="${STACK_NAMES[$index]}"
    local old_project="${OLD_PROJECTS[$index]}"

    if [[ ! -f "$compose_file" ]]; then
      printf '[prune] skipping %s — compose file not found\n' "$stack_name" >&2
      continue
    fi

    log "Stopping $stack_name ($compose_file)"
    run docker compose -f "$compose_file" down --volumes --remove-orphans --rmi local || true

    if [[ -n "$old_project" ]]; then
      if docker compose ls --all --format json 2>/dev/null | grep -q "\"Name\":\"$old_project\""; then
        log "Stopping legacy Compose project '$old_project'"
        run docker compose -p "$old_project" -f "$compose_file" down --volumes --remove-orphans || true
      fi
    fi
  done

  # Remove the shared external network last.
  if docker network inspect inter-plane-bus >/dev/null 2>&1; then
    log "Removing inter-plane-bus"
    run docker network rm inter-plane-bus || true
  fi

  # Final sweep — any dangling images from the rebuilds.
  log "Sweeping dangling images"
  run docker image prune -f || true

  log "Prune complete."
}

# status_all — read-only health roll-up. Prints, per plane:
#   - the compose project name
#   - count of containers up vs total
#   - each container's name + state
status_all() {
  printf '\n%-22s %-12s %s\n' "PLANE" "STATE" "DETAIL"
  printf '%s\n' "---------------------------------------------------------------"
  for index in "${!COMPOSE_FILES[@]}"; do
    local compose_file="${COMPOSE_FILES[$index]}"
    local stack_name="${STACK_NAMES[$index]}"
    local bootstrap_services="${BOOTSTRAP_SERVICES[$index]}"

    if [[ ! -f "$compose_file" ]]; then
      printf '%-22s %-12s missing\n' "$stack_name" "MISSING"
      continue
    fi

    local total=0
    local healthy=0
    local service

    while IFS= read -r service; do
      [[ -z "$service" ]] && continue
      total=$((total + 1))
      if service_ready "$compose_file" "$service"; then
        healthy=$((healthy + 1))
      fi
    done < <(runtime_services "$compose_file" "$bootstrap_services")

    if [[ "$total" == "0" ]]; then
      printf '%-22s %-12s 0 runtime services configured\n' "$stack_name" "DOWN"
      continue
    fi

    local label="UP"
    if [[ "$healthy" != "$total" ]]; then
      label="DEGRADED"
    fi
    printf '%-22s %-12s %s of %s runtime services ready\n' "$stack_name" "$label" "$healthy" "$total"
  done
  printf '\n'
  log "Detailed container roster:"
  docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
}

compose_bootstrap() {
  local index

  ensure_velion_network
  ensure_frontend_bus

  for index in "${!COMPOSE_FILES[@]}"; do
    validate_compose "${COMPOSE_FILES[$index]}"
    if [[ "${STACK_NAMES[$index]}" == "Ingestion Plane" ]]; then
      validate_ingestion_plane_targets_quarry_v2 "${COMPOSE_FILES[$index]}"
    fi
  done

  if core_stacks_ready; then
    log "Core planes already running; compose bootstrap skips full rebuild."
    return 0
  fi

  log "Core planes are not fully ready; compose bootstrap will build prerequisite planes."
  for ((index = 0; index < FRONTEND_STACK_INDEX; index++)); do
    build_stack "$index"
  done
}

main() {
  case "$MODE" in
    compose-bootstrap)
      compose_bootstrap
      return 0
      ;;
    prune)
      prune_all
      return 0
      ;;
    status)
      status_all
      return 0
      ;;
  esac

  ensure_velion_network
  ensure_frontend_bus

  for index in "${!COMPOSE_FILES[@]}"; do
    build_stack "$index"
  done

  if [[ "$DRY_RUN" == "false" ]]; then
    log "All stacks processed. Running containers:"
    docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
  else
    log "Dry run complete. Compose files validated; no containers were built or started."
  fi
}

main "$@"
