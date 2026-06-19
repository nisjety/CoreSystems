#!/usr/bin/env bash
set -Eeuo pipefail

# Velion service builder for the core server planes.
#
# Subcommands:
#   ./build-velion-services.sh                Build/start every plane (default).
#   ./build-velion-services.sh --no-cache     Build all buildable images without
#                                              Docker layer cache, then start.
#   ./build-velion-services.sh --dry-run      Validate compose files only; no build/start.
#   ./build-velion-services.sh --compose-bootstrap
#                                              Called by velionv3 docker-compose.
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
#   REMOVE_TIMEOUT_SECONDS=60  Max seconds to wait for one-shot cleanup.
#   WAIT_FOR_STACK_READY=true  After each plane starts, wait until every default
#                              runtime service is running/healthy before moving
#                              to the next plane.
#   COMPOSE_PARALLEL_LIMIT=4   Max concurrent Docker Compose engine calls. Lower
#                              this (1–2) on a memory-constrained Docker VM: the
#                              heavy Rust planes (Quarry-v2, Model Plane) each
#                              spawn rustc+LLVM per core, and several building in
#                              parallel can OOM the VM (build dies with exit 101).
#   PRUNE_BUILD_CACHE=true     When pruning, also clear the Docker build cache
#                              (it balloons past 25 GB across full rebuilds and
#                              slows/stalls later builds). Set false to keep it.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE_ROOT="${CORE_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/../../.." && pwd)}"
cd "$CORE_ROOT"

# Compose resolves `.env` relative to the compose file path, not this
# monorepo root. Preload the root env here so every per-plane compose file
# sees the same local secrets/config, including BRAVE_API_KEY.
if [[ -f "$CORE_ROOT/.env" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$CORE_ROOT/.env"
  set +a
fi

# Quarry v2 expects BRAVE_SEARCH_KEY; the repo-local env currently stores the
# same credential as BRAVE_API_KEY. Preserve an explicit BRAVE_SEARCH_KEY if
# the caller already set one.
if [[ -z "${BRAVE_SEARCH_KEY:-}" && -n "${BRAVE_API_KEY:-}" ]]; then
  export BRAVE_SEARCH_KEY="$BRAVE_API_KEY"
fi

export COMPOSE_PARALLEL_LIMIT="${COMPOSE_PARALLEL_LIMIT:-4}"

MODE="build"
DRY_RUN=false
NO_CACHE=false
while (( $# > 0 )); do
  case "$1" in
    --dry-run) DRY_RUN=true; shift ;;
    --no-cache) NO_CACHE=true; shift ;;
    --compose-bootstrap) MODE="compose-bootstrap"; shift ;;
    --prune)   MODE="prune"; shift ;;
    --status)  MODE="status"; shift ;;
    --help|-h)
      sed -n '3,18p' "$0"
      exit 0
      ;;
    *)
      printf 'Usage: %s [--dry-run] [--no-cache] [--compose-bootstrap|--prune|--status]\n' "$0" >&2
      exit 2
      ;;
  esac
done

if [[ "$MODE" != "build" && "$MODE" != "compose-bootstrap" && "$DRY_RUN" == "true" ]]; then
  printf 'Usage: %s [--dry-run] [--compose-bootstrap]\n' "$0" >&2
  exit 2
fi

if [[ "$MODE" != "build" && "$MODE" != "compose-bootstrap" && "$NO_CACHE" == "true" ]]; then
  printf 'Usage: %s [--no-cache] [--compose-bootstrap]\n' "$0" >&2
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
  "apps/Frontend Plane/velionv3/docker-compose.yml"
)

STACK_NAMES=(
  "Data Plane v2"
  "Ingestion Plane"
  "Model Plane"
  "Control Plane"
  "Application Plane"
  "Frontend Plane Velion v3"
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
# (velion v1) before standing up `frontend-plane-velionv3` so the two cannot
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
  "seed_dev_account,verify_controlplane_db_auth"
  "wait_for_convex_gateway_ready"
  ""
)

FRONTEND_STACK_INDEX=5
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-900}"
WAIT_INTERVAL_SECONDS="${WAIT_INTERVAL_SECONDS:-3}"
REMOVE_TIMEOUT_SECONDS="${REMOVE_TIMEOUT_SECONDS:-60}"
WAIT_FOR_STACK_READY="${WAIT_FOR_STACK_READY:-true}"

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

run_with_timeout() {
  local timeout_seconds="$1"
  shift

  "$@" &
  local command_pid=$!
  local deadline=$((SECONDS + timeout_seconds))

  while kill -0 "$command_pid" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      pkill -TERM -P "$command_pid" >/dev/null 2>&1 || true
      kill -TERM "$command_pid" >/dev/null 2>&1 || true
      sleep 1
      pkill -KILL -P "$command_pid" >/dev/null 2>&1 || true
      kill -KILL "$command_pid" >/dev/null 2>&1 || true
      wait "$command_pid" >/dev/null 2>&1 || true
      return 124
    fi
    sleep 1
  done

  wait "$command_pid"
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
  local services rendered_config settings_file

  services="$(docker compose -f "$compose_file" config --services)"

  for service in quarry-edge quarry-control quarry-orchestrator searxng; do
    if ! grep -qx "$service" <<<"$services"; then
      printf 'Ingestion Plane must target Quarry-v2; missing service: %s\n' "$service" >&2
      return 1
    fi
  done

  if grep -qx "quarry-api" <<<"$services"; then
    printf 'Ingestion Plane points at deferred legacy Quarry service: quarry-api\n' >&2
    return 1
  fi

  rendered_config="$(docker compose -f "$compose_file" config)"
  if ! grep -q 'QUARRY_EDGE__SEARXNG_URL: http://searxng:8080' <<<"$rendered_config"; then
    printf 'Ingestion Plane must wire Quarry-v2 to its local searxng service (http://searxng:8080)\n' >&2
    return 1
  fi

  settings_file="$(dirname "$compose_file")/config/searxng/settings.yml"
  if [[ ! -f "$settings_file" ]]; then
    printf 'Ingestion Plane searxng settings file is missing: %s\n' "$settings_file" >&2
    return 1
  fi
  if ! grep -Eq '^[[:space:]]*-[[:space:]]*json[[:space:]]*$' "$settings_file"; then
    printf 'Ingestion Plane searxng settings must enable search.formats json: %s\n' "$settings_file" >&2
    return 1
  fi
}

compose_services() {
  local compose_file="$1"
  docker compose -f "$compose_file" config --services
}

buildable_services() {
  local compose_file="$1"

  if ! command -v jq >/dev/null 2>&1; then
    printf 'jq is required for --no-cache service targeting\n' >&2
    return 1
  fi

  docker compose -f "$compose_file" config --format json \
    | jq -r '.services | to_entries[] | select(.value.build != null) | .key'
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

no_cache_build_services() {
  local compose_file="$1"

  buildable_services "$compose_file" | xargs
}

no_cache_runtime_start_services() {
  local compose_file="$1"
  local bootstrap_services="$2"
  local services=""
  local service

  # Start image-only services when they are missing or unhealthy, but avoid
  # recreating already-healthy infra containers with fixed host ports.
  while IFS= read -r service; do
    [[ -z "$service" ]] && continue
    if service_ready "$compose_file" "$service"; then
      continue
    fi
    if buildable_services "$compose_file" | grep -qx "$service"; then
      continue
    fi
    services="$services $service"
  done < <(runtime_services "$compose_file" "$bootstrap_services")

  printf '%s\n' "$services" | xargs
}

non_build_runtime_services() {
  local compose_file="$1"
  local bootstrap_services="$2"
  local build_services service

  build_services="$(buildable_services "$compose_file" | xargs)"

  while IFS= read -r service; do
    [[ -z "$service" ]] && continue
    if service_in_list "$service" "$build_services"; then
      continue
    fi
    printf '%s\n' "$service"
  done < <(runtime_services "$compose_file" "$bootstrap_services")
}

wait_for_services_ready() {
  local compose_file="$1"
  local services="$2"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))
  local service pending

  [[ -n "$services" ]] || return 0

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] wait for runtime services: %s\n' "$services"
    return 0
  fi

  while (( SECONDS < deadline )); do
    pending=()
    for service in $services; do
      if ! service_ready "$compose_file" "$service"; then
        pending+=("$service")
      fi
    done

    if (( ${#pending[@]} == 0 )); then
      return 0
    fi

    sleep "$WAIT_INTERVAL_SECONDS"
  done

  printf 'Timed out waiting for runtime services to become ready: %s\n' "$services" >&2
  docker compose -f "$compose_file" ps || true
  for service in $services; do
    if ! service_ready "$compose_file" "$service"; then
      docker compose -f "$compose_file" logs --tail=80 "$service" || true
    fi
  done
  return 1
}

wait_for_stack_ready() {
  local name="$1"
  local compose_file="$2"
  local bootstrap_services="$3"
  local services

  if [[ "$WAIT_FOR_STACK_READY" != "true" ]]; then
    return 0
  fi

  services="$(runtime_services "$compose_file" "$bootstrap_services" | xargs)"
  [[ -n "$services" ]] || return 0

  log "Waiting for $name runtime services"
  wait_for_services_ready "$compose_file" "$services"
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
    for service in "${existing[@]}"; do
      local container_id
      container_id="$(docker compose -f "$compose_file" ps -aq "$service" 2>/dev/null || true)"
      [[ -z "$container_id" ]] && continue

      if ! run_with_timeout "$REMOVE_TIMEOUT_SECONDS" docker rm -f -v "$container_id" >/dev/null; then
        printf '[cleanup] WARN: timed out removing one-shot service %s (%s); continuing\n' "$service" "$container_id" >&2
      fi
    done
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
  if [[ "$NO_CACHE" == "true" ]]; then
    local no_cache_build_service_list no_cache_runtime_service_list no_cache_runtime_ready_service_list
    no_cache_build_service_list="$(no_cache_build_services "$compose_file")"
    no_cache_runtime_service_list="$(no_cache_runtime_start_services "$compose_file" "$bootstrap_services")"
    no_cache_runtime_ready_service_list="$(non_build_runtime_services "$compose_file" "$bootstrap_services" | xargs)"
    if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
      run docker compose -f "$compose_file" build --no-cache gateway frontend
      run env VELION_SKIP_BOOTSTRAP=1 docker compose -f "$compose_file" up -d --remove-orphans nats gateway frontend
    else
      if [[ -z "$no_cache_build_service_list" && -z "$no_cache_runtime_service_list" ]]; then
        run docker compose -f "$compose_file" up -d --remove-orphans
      fi
      if [[ -n "$no_cache_build_service_list" ]]; then
        run docker compose -f "$compose_file" build --no-cache $no_cache_build_service_list
      fi
      if [[ -n "$no_cache_runtime_service_list" ]]; then
        run docker compose -f "$compose_file" up -d --remove-orphans $no_cache_runtime_service_list
      fi
      wait_for_services_ready "$compose_file" "$no_cache_runtime_ready_service_list"
      if [[ -n "$no_cache_build_service_list" ]]; then
        run docker compose -f "$compose_file" up -d --no-deps --remove-orphans $no_cache_build_service_list
      fi
    fi
  else
    if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
      run env VELION_SKIP_BOOTSTRAP=1 docker compose -f "$compose_file" up -d --build --remove-orphans nats gateway frontend
    else
      run docker compose -f "$compose_file" up -d --build --remove-orphans
    fi
  fi

  if [[ "$DRY_RUN" == "false" ]]; then
    remove_one_shot_containers "$compose_file" "$bootstrap_services"
    wait_for_stack_ready "$name" "$compose_file" "$bootstrap_services"

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

wait_for_convex_gateway_ready() {
  local compose_file="$CORE_ROOT/apps/Application Plane/docker-compose.yml"

  # convex-gateway's startup script sets Convex env vars, deploys functions,
  # and only then starts `convex dev`. Its healthcheck depends on that dev
  # process, so readiness here also means the function registry is loaded.
  wait_for_services_ready "$compose_file" "convex-gateway"
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

# seed_dev_account — idempotently create a real Better Auth login (default
# local@velion.dev) so password login works even with dev-auth-bypass off.
#
# Uses Better Auth's own POST /api/auth/sign-up/email (so the password is
# hashed in the correct, version-safe format) against the running auth-core,
# then promotes the user to superadmin and marks the email verified via SQL so
# the login is immediately usable. Tolerant by design — it never fails the
# build (warns + continues). Overridable:
#   SEED_DEV_ACCOUNT=0       skip seeding entirely
#   SEED_DEV_EMAIL / SEED_DEV_PASSWORD / SEED_DEV_NAME
#   SEED_AUTH_URL            (default http://localhost:3011)
#   SEED_PG_CONTAINER        (default controlplane-postgres)
seed_dev_account() {
  if [[ "${SEED_DEV_ACCOUNT:-1}" == "0" ]]; then
    log "Dev account seed disabled (SEED_DEV_ACCOUNT=0)"
    return 0
  fi

  local email="${SEED_DEV_EMAIL:-local@velion.dev}"
  local password="${SEED_DEV_PASSWORD:-VelionLocal!2026}"
  local name="${SEED_DEV_NAME:-Velion Local}"
  local auth_url="${SEED_AUTH_URL:-http://localhost:3011}"
  local pg="${SEED_PG_CONTAINER:-controlplane-postgres}"

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[seed] dry-run: would seed %s via %s\n' "$email" "$auth_url"
    return 0
  fi

  log "Seeding dev login $email (idempotent)"

  # Wait until auth-core answers at all (any HTTP status = up). Max ~60s.
  local attempts=0
  while [[ "$(curl -s -o /dev/null -w '%{http_code}' "$auth_url/api/auth/ok" 2>/dev/null || echo 000)" == "000" ]]; do
    attempts=$((attempts + 1))
    if (( attempts >= 30 )); then
      printf '[seed] WARN: auth-core unreachable at %s after %d tries; skipping dev seed\n' "$auth_url" "$attempts" >&2
      return 0
    fi
    sleep 2
  done

  # Create the account. A non-2xx almost always means it already exists —
  # idempotent, so log and continue either way.
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' -X POST "$auth_url/api/auth/sign-up/email" \
    -H 'content-type: application/json' \
    -d "{\"email\":\"$email\",\"password\":\"$password\",\"name\":\"$name\"}" 2>/dev/null || echo 000)"
  case "$code" in
    2*) printf '[seed] created %s\n' "$email" ;;
    *)  printf '[seed] signup HTTP %s for %s (likely already exists); continuing\n' "$code" "$email" ;;
  esac

  # Promote to superadmin + mark verified so the login is usable immediately.
  if ! docker exec -i "$pg" psql -v ON_ERROR_STOP=1 -U aquatiq -d auth_service -c \
      "UPDATE \"user\" SET role='superadmin', email_verified=true WHERE email='$email';" >/dev/null 2>&1; then
    printf '[seed] WARN: could not promote %s to superadmin (continuing)\n' "$email" >&2
  fi
}

# verify_controlplane_db_auth — after the Control Plane is up, confirm each
# DB-backed core can actually AUTHENTICATE to Postgres by hitting its DEEP
# /health endpoint (which round-trips the DB). This is the net for the
# stale-DB-password drift that silently broke create-org on 2026-06-18: a core
# limping on old pooled connections looks "up" (port open) but fails every new
# write. If a core is unhealthy, force-recreate it once so it picks up the
# current single-sourced ${DB_PASSWORD} config, then re-check.
#
# Tolerant by default (warns, never aborts the build) — set STRICT_DB_AUTH=1 to
# fail the build when a core still cannot reach its DB (recommended for CI).
# Skip entirely with VERIFY_DB_AUTH=0.
verify_controlplane_db_auth() {
  if [[ "${VERIFY_DB_AUTH:-1}" == "0" ]]; then
    log "Control Plane DB-auth verification disabled (VERIFY_DB_AUTH=0)"
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[db-auth] dry-run: would probe Control Plane core /health endpoints\n'
    return 0
  fi

  local compose_file="$CORE_ROOT/apps/Control Plane/docker-compose.yml"
  # compose-service:host-port — /health on these cores is a DEEP check (DB ping).
  local checks=("org-core:18080" "user-core:3012" "billing-core:3014")
  local failed=()
  local entry svc port code attempt

  probe() { # $1=port → echoes final HTTP code after up to ~30s
    local p="$1" c=000 i
    for ((i = 0; i < 15; i++)); do
      c="$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:$p/health" 2>/dev/null)"
      [[ -z "$c" ]] && c="000"
      [[ "$c" == "200" ]] && break
      sleep 2
    done
    printf '%s' "$c"
  }

  log "Verifying Control Plane cores can authenticate to Postgres (deep /health)"
  for entry in "${checks[@]}"; do
    svc="${entry%%:*}"
    port="${entry##*:}"
    code="$(probe "$port")"
    if [[ "$code" == "200" ]]; then
      printf '[db-auth] %-12s OK (/health 200 — DB reachable + authenticated)\n' "$svc"
      continue
    fi

    printf '[db-auth] WARN: %s /health=%s — DB unreachable/unauthenticated; force-recreating to pick up current config…\n' "$svc" "$code" >&2
    run docker compose -f "$compose_file" up -d --force-recreate --no-deps "$svc"
    code="$(probe "$port")"
    if [[ "$code" == "200" ]]; then
      printf '[db-auth] %-12s recovered after recreate (/health 200)\n' "$svc"
    else
      printf '[db-auth] ERROR: %s still /health=%s after recreate — likely a DB password mismatch (compare ${DB_PASSWORD} in .env against the running Postgres).\n' "$svc" "$code" >&2
      docker compose -f "$compose_file" logs --tail 8 "$svc" 2>&1 \
        | grep -iE "password authentication|database ping|connect database" | sed 's/^/[db-auth]   /' || true
      failed+=("$svc")
    fi
  done

  if (( ${#failed[@]} > 0 )); then
    printf '[db-auth] FAILED: %s cannot authenticate to Postgres.\n' "${failed[*]}" >&2
    if [[ "${STRICT_DB_AUTH:-0}" == "1" ]]; then
      return 1
    fi
    printf '[db-auth] Continuing (set STRICT_DB_AUTH=1 to fail the build here).\n' >&2
  fi
  return 0
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

  # The build cache is the big one: the Rust planes (Quarry-v2, Model Plane,
  # gateway) leave tens of GB of intermediate layers behind, and a bloated
  # cache slows — and on a small Docker VM can stall — the next build. Clear
  # it unless the caller opted out via PRUNE_BUILD_CACHE=false.
  if [[ "${PRUNE_BUILD_CACHE:-true}" == "true" ]]; then
    log "Pruning Docker build cache"
    run docker builder prune -af || true
  else
    log "Keeping Docker build cache (PRUNE_BUILD_CACHE=false)"
  fi

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
