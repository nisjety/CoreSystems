#!/usr/bin/env bash
set -Eeuo pipefail

# Velion service builder for the core server planes.
#
# Subcommands:
#   ./build-velion-services.sh                Build/start every plane (default).
#   ./build-velion-services.sh --no-cache     Build all buildable images without
#                                              Docker layer cache, then start.
#   ./build-velion-services.sh --dry-run      Validate compose files only; no build/start.
#   ./build-velion-services.sh --from ingestion
#                                              Resume at a plane and continue downstream.
#   ./build-velion-services.sh --resume        Skip planes that are already fully ready.
#   ./build-velion-services.sh --skip-build    Start existing images without rebuilding.
#   ./build-velion-services.sh --production    Layer available production Compose overrides
#                                              and enable strict verification defaults.
#   ./build-velion-services.sh --compose-bootstrap
#                                              Called by velionv3 docker-compose.
#                                              Builds core planes only when they
#                                              are not already ready.
#   CONFIRM_DESTRUCTIVE_PRUNE=1 ./build-velion-services.sh --prune
#                                              Stop + remove every plane's containers,
#                                              volumes, and networks. This is destructive.
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
#   COMPOSE_PARALLEL_LIMIT=2   Max concurrent Docker Compose engine calls. Lower
#                              this (1–2) on a memory-constrained Docker VM: the
#                              heavy Rust planes (Quarry-v2, Model Plane) each
#                              spawn rustc+LLVM per core, and several building in
#                              parallel can OOM the VM (build dies with exit 101).
#   COMPOSE_BAKE=false         Keep full-stack local builds predictable. Set true
#                              to opt into buildx bake parallel builds.
#   COMPOSE_PROGRESS=plain     Keep logs readable for long plane-by-plane builds.
#   BUILDKIT_PROGRESS=plain    Keep BuildKit logs readable.
#   PRUNE_BUILD_CACHE=true     Also clear Docker's build cache during a confirmed
#                              prune. Defaults to false so useful build layers survive.
#   SEED_DEV_ACCOUNT=1        Opt in to local Better Auth account seeding. Requires
#                              SEED_DEV_PASSWORD; disabled by default.
#   DOCKER_CONNECT_TIMEOUT_SECONDS=20
#                              Fail fast when the Docker daemon is stuck/unavailable.
#   MIN_DOCKER_FREE_GB=15      Refuse builds when Docker's data volume is nearly full.
#   DOCKER_DISK_IMAGE_PATH=... Explicit Docker.raw path for the disk-space preflight.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE_ROOT="${CORE_ROOT_OVERRIDE:-$(cd "$SCRIPT_DIR/../../.." && pwd)}"
cd "$CORE_ROOT"

# Compose loads each plane's `.env` itself. Never source a dotenv file as shell:
# besides treating data as executable code, exporting the root file would make
# its values override every plane-local `.env` through Compose precedence.
dotenv_value() {
  local file="$1"
  local key="$2"
  local value

  [[ -f "$file" ]] || return 1
  value="$(awk -v wanted="$key" '
    /^[[:space:]]*#/ { next }
    {
      line=$0
      sub(/^[[:space:]]*export[[:space:]]+/, "", line)
      if (index(line, wanted "=") == 1) {
        print substr(line, length(wanted) + 2)
        exit
      }
    }
  ' "$file")"
  [[ -n "$value" ]] || return 1
  if [[ "$value" == \"*\" && ${#value} -ge 2 ]]; then
    value="${value#\"}"
    value="${value%\"}"
  elif [[ "$value" == \'*\' && ${#value} -ge 2 ]]; then
    value="${value#\'}"
    value="${value%\'}"
  fi
  printf '%s' "$value"
}

# Quarry v2 expects BRAVE_SEARCH_KEY; the repo-local env currently stores the
# same credential as BRAVE_API_KEY. Preserve an explicit BRAVE_SEARCH_KEY if
# the caller already set one.
if [[ -z "${BRAVE_SEARCH_KEY:-}" ]]; then
  brave_key="${BRAVE_API_KEY:-}"
  if [[ -z "$brave_key" ]]; then
    brave_key="$(dotenv_value "$CORE_ROOT/.env" BRAVE_SEARCH_KEY 2>/dev/null || true)"
  fi
  if [[ -z "$brave_key" ]]; then
    brave_key="$(dotenv_value "$CORE_ROOT/.env" BRAVE_API_KEY 2>/dev/null || true)"
  fi
  if [[ -n "$brave_key" ]]; then
    export BRAVE_SEARCH_KEY="$brave_key"
  fi
fi
unset brave_key

export COMPOSE_PARALLEL_LIMIT="${COMPOSE_PARALLEL_LIMIT:-2}"
export COMPOSE_PROGRESS="${COMPOSE_PROGRESS:-plain}"
export BUILDKIT_PROGRESS="${BUILDKIT_PROGRESS:-plain}"
export SOURCE_REVISION="${SOURCE_REVISION:-$(git -C "$CORE_ROOT" rev-parse HEAD 2>/dev/null || printf unverified)}"
export BUILD_DATE="${BUILD_DATE:-$(date -u '+%Y-%m-%dT%H:%M:%SZ')}"

# ── Build-speed env (safe defaults; all caller-overridable) ─────────────────
# BuildKit is required for the `RUN --mount=type=cache` steps in the per-plane
# Dockerfiles to actually persist their caches between builds. Enable it for
# both the `docker build` path and the Compose-driven build path. These are
# honored only if the caller has not already set them.
export DOCKER_BUILDKIT="${DOCKER_BUILDKIT:-1}"
export COMPOSE_DOCKER_CLI_BUILD="${COMPOSE_DOCKER_CLI_BUILD:-1}"

# COMPOSE_BAKE=true delegates multi-service builds to `docker buildx bake`.
# That can be faster on large machines, but it builds many services at once and
# repeatedly sends large Rust contexts during a full local stack bring-up. Keep
# local full-stack builds serial-friendly by default; callers can opt in with
# COMPOSE_BAKE=true.
if [[ -z "${COMPOSE_BAKE:-}" ]]; then
  export COMPOSE_BAKE=false
else
  export COMPOSE_BAKE
fi

MODE="build"
MODE_FLAGS=0
DRY_RUN=false
NO_CACHE=false
SKIP_BUILD=false
RESUME_READY_STACKS=false
PRODUCTION=false
CLEANUP_LEGACY_PROJECTS=false
VALIDATE_SOURCE_PLACEHOLDERS="${VERIFY_SOURCE_PLACEHOLDERS:-false}"
FROM_STACK=""
usage() {
  printf 'Usage: %s [options]\n' "$0"
  printf '%s\n' \
    '  --dry-run             Render and validate selected Compose files; no daemon calls' \
    '  --from PLANE          Start at data|control|ingestion|model|application|frontend' \
    '  --resume              Skip planes whose runtime services are already ready' \
    '  --skip-build          Start existing images without rebuilding' \
    '  --no-cache            Rebuild all selected images without Docker layer cache' \
    '  --production          Layer available production overrides and enable strict checks' \
    '  --validate-source     Deep-scan source defaults for known insecure placeholders' \
    '  --cleanup-legacy      Explicitly stop known legacy Compose projects before startup' \
    '  --compose-bootstrap   Start prerequisite planes for Compose-managed frontend startup' \
    '  --status              Print a read-only per-plane health roll-up' \
    '  --prune               Destructive reset; also requires CONFIRM_DESTRUCTIVE_PRUNE=1' \
    '  --help                Show this help'
}
while (( $# > 0 )); do
  case "$1" in
    --dry-run) DRY_RUN=true; shift ;;
    --no-cache) NO_CACHE=true; shift ;;
    --skip-build) SKIP_BUILD=true; shift ;;
    --resume) RESUME_READY_STACKS=true; shift ;;
    --production) PRODUCTION=true; shift ;;
    --validate-source) VALIDATE_SOURCE_PLACEHOLDERS=true; shift ;;
    --cleanup-legacy) CLEANUP_LEGACY_PROJECTS=true; shift ;;
    --from)
      if (( $# < 2 )) || [[ -z "$2" || "$2" == --* ]]; then
        printf '%s requires a plane name\n' "$1" >&2
        exit 2
      fi
      FROM_STACK="$2"
      shift 2
      ;;
    --compose-bootstrap) MODE="compose-bootstrap"; MODE_FLAGS=$((MODE_FLAGS + 1)); shift ;;
    --prune)   MODE="prune"; MODE_FLAGS=$((MODE_FLAGS + 1)); shift ;;
    --status)  MODE="status"; MODE_FLAGS=$((MODE_FLAGS + 1)); shift ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

if (( MODE_FLAGS > 1 )); then
  printf '%s\n' '--compose-bootstrap, --prune, and --status are mutually exclusive' >&2
  exit 2
fi

if [[ "$MODE" != "build" && "$MODE" != "compose-bootstrap" && "$DRY_RUN" == "true" ]]; then
  printf 'Usage: %s [--dry-run] [--compose-bootstrap]\n' "$0" >&2
  exit 2
fi

if [[ "$MODE" != "build" && "$MODE" != "compose-bootstrap" && "$NO_CACHE" == "true" ]]; then
  printf 'Usage: %s [--no-cache] [--compose-bootstrap]\n' "$0" >&2
  exit 2
fi

if [[ "$NO_CACHE" == "true" && "$SKIP_BUILD" == "true" ]]; then
  printf '%s\n' '--no-cache and --skip-build cannot be used together' >&2
  exit 2
fi

if [[ "$MODE" != "build" && ( -n "$FROM_STACK" || "$RESUME_READY_STACKS" == "true" || "$SKIP_BUILD" == "true" || "$PRODUCTION" == "true" || "$CLEANUP_LEGACY_PROJECTS" == "true" ) ]]; then
  printf '%s\n' '--from, --resume, --skip-build, --production, and --cleanup-legacy are only valid for the default build mode' >&2
  exit 2
fi

if [[ "$DRY_RUN" == "true" && "$RESUME_READY_STACKS" == "true" ]]; then
  printf '%s\n' '--resume needs Docker runtime state and cannot be combined with --dry-run' >&2
  exit 2
fi

if [[ "$PRODUCTION" == "true" && "$RESUME_READY_STACKS" == "true" ]]; then
  printf '%s\n' '--production and --resume cannot be combined; production must reconcile every selected container' >&2
  exit 2
fi

if [[ "$PRODUCTION" == "true" && "$SKIP_BUILD" == "true" ]]; then
  printf '%s\n' '--production and --skip-build cannot be combined; production images must be rebuilt from the production targets' >&2
  exit 2
fi

if [[ "$PRODUCTION" == "true" ]]; then
  export STRICT_INTERNAL_KEY_CHECK="${STRICT_INTERNAL_KEY_CHECK:-1}"
  export STRICT_DB_AUTH="${STRICT_DB_AUTH:-1}"
  export STRICT_OWNERSHIP_CHECK="${STRICT_OWNERSHIP_CHECK:-1}"
  export SEED_DEV_ACCOUNT=0
fi

# These are intentionally ordered by dependency flow. Data's strict JWT
# services fetch Auth Core JWKS during startup, so Control must be ready before
# Data. Ingestion follows both, then Model, Application, and Frontend.
COMPOSE_FILES=(
  "apps/Control Plane/docker-compose.yml"
  "apps/Data Plane v2/docker-compose.yml"
  "apps/Ingestion Plane/docker-compose.yml"
  "apps/Model Plane/deploy/docker-compose.yml"
  "apps/Application Plane/docker-compose.yml"
  "apps/Frontend Plane/velionv3/docker-compose.yml"
)

STACK_NAMES=(
  "Control Plane"
  "Data Plane v2"
  "Ingestion Plane"
  "Model Plane"
  "Application Plane"
  "Frontend Plane Velion v3"
)

# One-shot services are removed after they exit successfully so `docker ps -a`
# stays focused on long-running servers.
BOOTSTRAP_SERVICES=(
  "lago-migrate"
  "minio-init migrate"
  ""
  ""
  "affine-runtime-migration"
  ""
)
# Model Plane (index 3) historically had three one-shots
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
  ""
  "deploy"
  ""
  "frontend-plane-velion"
)

# Per-stack post-build hooks (run after one-shots clear, before moving to the
# next stack). Keep entries short — long hooks belong in their own function.
# Format: comma-separated function names; empty string skips.
POST_BUILD_HOOKS=(
  "seed_dev_account,verify_controlplane_db_auth"
  "apply_dataplane_migrations,verify_ownership_phase"
  "ensure_finspo_database"
  ""
  "wait_for_convex_gateway_ready"
  ""
)

FRONTEND_STACK_INDEX=5
START_STACK_INDEX=0
WAIT_TIMEOUT_SECONDS="${WAIT_TIMEOUT_SECONDS:-900}"
WAIT_INTERVAL_SECONDS="${WAIT_INTERVAL_SECONDS:-3}"
REMOVE_TIMEOUT_SECONDS="${REMOVE_TIMEOUT_SECONDS:-60}"
WAIT_FOR_STACK_READY="${WAIT_FOR_STACK_READY:-true}"
DOCKER_CONNECT_TIMEOUT_SECONDS="${DOCKER_CONNECT_TIMEOUT_SECONDS:-20}"
MIN_DOCKER_FREE_GB="${MIN_DOCKER_FREE_GB:-15}"

validate_launcher_settings() {
  local name value
  for name in WAIT_TIMEOUT_SECONDS WAIT_INTERVAL_SECONDS REMOVE_TIMEOUT_SECONDS \
      DOCKER_CONNECT_TIMEOUT_SECONDS COMPOSE_PARALLEL_LIMIT; do
    value="${!name}"
    if [[ ! "$value" =~ ^[1-9][0-9]*$ ]]; then
      printf '%s must be a positive integer, got: %s\n' "$name" "$value" >&2
      return 2
    fi
  done
  if [[ ! "$MIN_DOCKER_FREE_GB" =~ ^[0-9]+$ ]]; then
    printf 'MIN_DOCKER_FREE_GB must be a non-negative integer, got: %s\n' "$MIN_DOCKER_FREE_GB" >&2
    return 2
  fi
  if [[ "$WAIT_FOR_STACK_READY" != "true" && "$WAIT_FOR_STACK_READY" != "false" ]]; then
    printf 'WAIT_FOR_STACK_READY must be true or false, got: %s\n' "$WAIT_FOR_STACK_READY" >&2
    return 2
  fi
  if [[ "$VALIDATE_SOURCE_PLACEHOLDERS" != "true" && "$VALIDATE_SOURCE_PLACEHOLDERS" != "false" ]]; then
    printf 'VERIFY_SOURCE_PLACEHOLDERS must be true or false, got: %s\n' "$VALIDATE_SOURCE_PLACEHOLDERS" >&2
    return 2
  fi
}

validate_launcher_settings

resolve_stack_index() {
  local requested
  requested="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  requested="${requested// /-}"
  requested="${requested//_/-}"

  case "$requested" in
    control|control-plane|controlplane) printf '0' ;;
    data|data-plane|data-plane-v2|dataplane) printf '1' ;;
    ingestion|ingestion-plane) printf '2' ;;
    model|model-plane) printf '3' ;;
    application|application-plane|app) printf '4' ;;
    frontend|frontend-plane|velion|velionv3) printf '5' ;;
    *)
      printf 'Unknown plane for --from: %s (use data, control, ingestion, model, application, or frontend)\n' "$1" >&2
      return 2
      ;;
  esac
}

if [[ -n "$FROM_STACK" ]]; then
  START_STACK_INDEX="$(resolve_stack_index "$FROM_STACK")"
fi

log() {
  printf '\n[%s] %s\n' "$(date '+%H:%M:%S')" "$*"
}

# _strip_env_quotes — strip one matching pair of surrounding double or single
# quotes from a raw `.env` value (KEY="value" / KEY='value' both parse to
# `value` under normal dotenv semantics; a bare grep-and-cut extraction does
# not do this, which caused a false "drift" positive between two files holding
# the identical secret in different quoting styles). Values with no matching
# surrounding quotes pass through unchanged.
_strip_env_quotes() {
  local v="$1"
  if [[ "$v" == \"*\" && ${#v} -ge 2 ]]; then
    v="${v#\"}"; v="${v%\"}"
  elif [[ "$v" == \'*\' && ${#v} -ge 2 ]]; then
    v="${v#\'}"; v="${v%\'}"
  fi
  printf '%s' "$v"
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

compose() {
  local original_args=("$@")
  local compose_args=()
  local env_args=()
  local compose_file=""
  local plane_env=""
  local production_override=""
  local index=0

  # Compose's implicit dotenv location depends on invocation/project context.
  # Pass it explicitly so the root file is a fallback and the plane file wins.
  while (( index < ${#original_args[@]} )); do
    if [[ "${original_args[$index]}" == "-f" && $((index + 1)) -lt ${#original_args[@]} ]]; then
      compose_file="${original_args[$((index + 1))]}"
      compose_args+=("-f" "$compose_file")

      if [[ "$PRODUCTION" == "true" && "$(basename "$compose_file")" == "docker-compose.yml" ]]; then
        production_override="$(dirname "$compose_file")/docker-compose.production.yml"
        if [[ -f "$production_override" ]]; then
          compose_args+=("-f" "$production_override")
        else
          case "$compose_file" in
            *"apps/Control Plane/docker-compose.yml"|*"apps/Ingestion Plane/docker-compose.yml"|*"apps/Model Plane/deploy/docker-compose.yml"|*"apps/Frontend Plane/velionv3/docker-compose.yml")
              printf 'Required production Compose override is missing: %s\n' "$production_override" >&2
              return 1
              ;;
          esac
        fi
      fi
      index=$((index + 2))
      continue
    fi
    compose_args+=("${original_args[$index]}")
    index=$((index + 1))
  done

  if [[ -f "$CORE_ROOT/.env" ]]; then
    env_args+=("--env-file" "$CORE_ROOT/.env")
  fi
  if [[ -n "$compose_file" ]]; then
    if [[ "$compose_file" == /* ]]; then
      plane_env="$(dirname "$compose_file")/.env"
    else
      plane_env="$CORE_ROOT/$(dirname "$compose_file")/.env"
    fi
    if [[ -f "$plane_env" && "$plane_env" != "$CORE_ROOT/.env" ]]; then
      env_args+=("--env-file" "$plane_env")
    fi
  fi

  if (( ${#env_args[@]} > 0 )); then
    docker compose --parallel "$COMPOSE_PARALLEL_LIMIT" --progress "$COMPOSE_PROGRESS" \
      "${env_args[@]}" "${compose_args[@]}"
  else
    # Bash 3.2 + `set -u` treats an empty-array expansion as unbound.
    docker compose --parallel "$COMPOSE_PARALLEL_LIMIT" --progress "$COMPOSE_PROGRESS" \
      "${compose_args[@]}"
  fi
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

docker_disk_image_path() {
  local candidate

  if [[ -n "${DOCKER_DISK_IMAGE_PATH:-}" ]]; then
    printf '%s' "$DOCKER_DISK_IMAGE_PATH"
    return 0
  fi

  # Docker Desktop's default path plus this workstation's configured external
  # data path. Avoid searching whole volumes: a full/stalled Docker volume is
  # precisely the condition this preflight is meant to diagnose quickly.
  for candidate in \
    "$HOME/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw" \
    "/Volumes/Applikasjon/DockerDesktop/DockerDesktop/Docker.raw"; do
    if [[ -e "$candidate" ]]; then
      printf '%s' "$candidate"
      return 0
    fi
  done

  return 1
}

check_docker_disk_space() {
  local minimum_gb="$MIN_DOCKER_FREE_GB"
  local disk_image anchor available_kb required_kb available_gb

  [[ "$minimum_gb" =~ ^[0-9]+$ ]] || {
    printf 'MIN_DOCKER_FREE_GB must be a non-negative integer, got: %s\n' "$minimum_gb" >&2
    return 2
  }
  (( minimum_gb > 0 )) || return 0

  disk_image="$(docker_disk_image_path 2>/dev/null || true)"
  if [[ -n "$disk_image" ]]; then
    anchor="$(dirname "$disk_image")"
  else
    anchor="$CORE_ROOT"
  fi

  available_kb="$(df -Pk "$anchor" 2>/dev/null | awk 'NR == 2 { print $4 }')"
  [[ "$available_kb" =~ ^[0-9]+$ ]] || {
    printf 'Unable to determine free disk space for Docker build path: %s\n' "$anchor" >&2
    return 1
  }

  required_kb=$((minimum_gb * 1024 * 1024))
  if (( available_kb < required_kb )); then
    available_gb=$((available_kb / 1024 / 1024))
    printf 'Insufficient free space for a full Docker build: %s GiB available at %s; %s GiB required.\n' \
      "$available_gb" "$anchor" "$minimum_gb" >&2
    printf 'Free space first, or override deliberately with MIN_DOCKER_FREE_GB=0. No Docker state was changed.\n' >&2
    return 1
  fi

  log "Docker build disk preflight: $((available_kb / 1024 / 1024)) GiB free at $anchor"
}

preflight_compose_cli() {
  if ! command -v docker >/dev/null 2>&1; then
    printf 'Docker CLI is not installed or not on PATH.\n' >&2
    return 1
  fi

  if ! docker compose version >/dev/null 2>&1; then
    printf 'Docker Compose v2 is unavailable; install/enable the docker compose plugin.\n' >&2
    return 1
  fi
}

preflight_docker() {
  preflight_compose_cli

  if [[ "$MODE" == "build" && "$SKIP_BUILD" != "true" ]]; then
    check_docker_disk_space
  fi

  if ! run_with_timeout "$DOCKER_CONNECT_TIMEOUT_SECONDS" \
      docker info --format '{{.ServerVersion}}' >/dev/null 2>&1; then
    printf 'Docker daemon is unavailable or did not respond within %s seconds. Start/recover Docker Desktop, then rerun this command.\n' \
      "$DOCKER_CONNECT_TIMEOUT_SECONDS" >&2
    return 1
  fi
}

LOCK_DIR="${VELION_LOCK_DIR:-$CORE_ROOT/.velion-stack.lock}"
LOCK_HELD=false
ACTIVE_STACK_INDEX=-1

release_orchestration_lock() {
  if [[ "$LOCK_HELD" == "true" ]]; then
    rm -f "$LOCK_DIR/pid"
    rmdir "$LOCK_DIR" 2>/dev/null || true
    LOCK_HELD=false
  fi
}

report_launcher_error() {
  local exit_code=$?
  local line="$1"
  trap - ERR

  printf '\n[launcher] ERROR: command failed at line %s (exit %s).\n' "$line" "$exit_code" >&2
  if (( ACTIVE_STACK_INDEX >= 0 )); then
    local resume_aliases=(control data ingestion model application frontend)
    printf '[launcher] Active plane: %s. Resume with: %q --from %q' \
      "${STACK_NAMES[$ACTIVE_STACK_INDEX]}" "$SCRIPT_DIR/build-velion-services.sh" \
      "${resume_aliases[$ACTIVE_STACK_INDEX]}" >&2
    [[ "$PRODUCTION" == "true" ]] && printf ' --production' >&2
    [[ "$SKIP_BUILD" == "true" ]] && printf ' --skip-build' >&2
    printf '\n' >&2
  fi
  return "$exit_code"
}

acquire_orchestration_lock() {
  local existing_pid=""

  if mkdir "$LOCK_DIR" 2>/dev/null; then
    printf '%s\n' "$$" > "$LOCK_DIR/pid"
    LOCK_HELD=true
  else
    if [[ -f "$LOCK_DIR/pid" ]]; then
      IFS= read -r existing_pid < "$LOCK_DIR/pid" || true
    fi
    if [[ "$existing_pid" =~ ^[0-9]+$ ]] && kill -0 "$existing_pid" 2>/dev/null; then
      printf 'Another Velion stack operation is already running (pid %s).\n' "$existing_pid" >&2
      return 1
    fi

    # Recover only the exact lock artifacts this script owns.
    rm -f "$LOCK_DIR/pid"
    if ! rmdir "$LOCK_DIR" 2>/dev/null || ! mkdir "$LOCK_DIR" 2>/dev/null; then
      printf 'Unable to recover stale launcher lock: %s\n' "$LOCK_DIR" >&2
      return 1
    fi
    printf '%s\n' "$$" > "$LOCK_DIR/pid"
    LOCK_HELD=true
  fi

  trap release_orchestration_lock EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  trap 'report_launcher_error "$LINENO"' ERR
}

ensure_velion_network() {
  # Compose files across all planes declare `inter-plane-bus` as an external
  # network. Older versions of this script created `velion-net`, which caused
  # `docker compose up` to fail with "network inter-plane-bus declared as
  # external, but could not be found". The shared bus is `inter-plane-bus`.
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] ensure Docker network inter-plane-bus exists\n'
    return 0
  fi

  if docker network inspect inter-plane-bus >/dev/null 2>&1; then
    return 0
  fi

  log "Creating shared Docker network: inter-plane-bus"
  run docker network create inter-plane-bus >/dev/null
}

validate_compose() {
  local compose_file="$1"
  compose -f "$compose_file" config --quiet
}

validate_ingestion_plane_targets_quarry_v2() {
  local compose_file="$1"
  local services rendered_config settings_file

  services="$(compose -f "$compose_file" config --services)"

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

  rendered_config="$(compose -f "$compose_file" config)"
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
  compose -f "$compose_file" config --services
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

  container_id="$(compose -f "$compose_file" ps -q "$service" 2>/dev/null || true)"
  [[ -n "$container_id" ]] || return 1

  status="$(docker inspect -f '{{.State.Status}}' "$container_id" 2>/dev/null || true)"
  [[ "$status" == "running" ]] || return 1

  health="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{end}}' "$container_id" 2>/dev/null || true)"
  [[ -z "$health" || "$health" == "healthy" ]]
}

services_ready_snapshot() {
  local compose_file="$1"
  local services="$2"
  local snapshot running_services healthy_services health_required_services service

  if ! command -v jq >/dev/null 2>&1; then
    for service in $services; do
      service_ready "$compose_file" "$service" || return 1
    done
    return 0
  fi

  snapshot="$(compose -f "$compose_file" ps --all --format json 2>/dev/null || true)"
  [[ -n "$snapshot" ]] || return 1
  health_required_services="$(compose -f "$compose_file" config --format json 2>/dev/null \
    | jq -r '.services | to_entries[] | select(.value.healthcheck != null and .value.healthcheck.disable != true) | .key' \
    | xargs || true)"
  running_services="$(jq -r '
    (if type == "array" then .[] else . end)
    | select((.State // "") == "running")
    | .Service
  ' <<<"$snapshot" 2>/dev/null | xargs || true)"
  healthy_services="$(jq -r '
    (if type == "array" then .[] else . end)
    | select((.State // "") == "running" and (.Health // "") == "healthy")
    | .Service
  ' <<<"$snapshot" 2>/dev/null | xargs || true)"

  for service in $services; do
    service_in_list "$service" "$running_services" || return 1
    if service_in_list "$service" "$health_required_services"; then
      service_in_list "$service" "$healthy_services" || return 1
    fi
  done
}

stack_runtime_ready() {
  local index="$1"
  local compose_file="${COMPOSE_FILES[$index]}"
  local bootstrap_services="${BOOTSTRAP_SERVICES[$index]}"
  local services

  services="$(runtime_services "$compose_file" "$bootstrap_services" | xargs)"
  [[ -n "$services" ]] || return 1
  services_ready_snapshot "$compose_file" "$services"
}

wait_for_services_ready() {
  local compose_file="$1"
  local services="$2"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))
  local service

  [[ -n "$services" ]] || return 0

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] wait for runtime services: %s\n' "$services"
    return 0
  fi

  while (( SECONDS < deadline )); do
    if services_ready_snapshot "$compose_file" "$services"; then
      return 0
    fi

    sleep "$WAIT_INTERVAL_SECONDS"
  done

  printf 'Timed out waiting for runtime services to become ready: %s\n' "$services" >&2
  compose -f "$compose_file" ps || true
  for service in $services; do
    if ! service_ready "$compose_file" "$service"; then
      compose -f "$compose_file" logs --tail=80 "$service" || true
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
  run compose -f "$compose_file" up -d --remove-orphans nats

  if [[ "$DRY_RUN" == "true" ]]; then
    return 0
  fi

  while (( SECONDS < deadline )); do
    if service_ready "$compose_file" "nats"; then
      return 0
    fi
    sleep "$WAIT_INTERVAL_SECONDS"
  done

  compose -f "$compose_file" logs --tail=120 nats || true
  printf 'Timed out waiting for frontend NATS bus\n' >&2
  return 1
}

stop_old_project() {
  local compose_file="$1"
  local old_project="$2"

  if [[ -z "$old_project" ]]; then
    return 0
  fi

  if [[ "$CLEANUP_LEGACY_PROJECTS" != "true" ]]; then
    return 0
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] would stop legacy Compose project if present: %s\n' "$old_project"
    return 0
  fi

  if compose ls --all --format json | grep -q "\"Name\":\"$old_project\""; then
    log "Stopping old Compose project '$old_project' for $compose_file"
    run compose -p "$old_project" -f "$compose_file" down --remove-orphans
  fi
}

wait_for_one_shot() {
  local compose_file="$1"
  local service="$2"
  local deadline=$((SECONDS + WAIT_TIMEOUT_SECONDS))

  while (( SECONDS < deadline )); do
    local container_id
    container_id="$(compose -f "$compose_file" ps --all -q "$service" 2>/dev/null || true)"

    if [[ -z "$container_id" ]]; then
      printf 'One-shot service was never created or is no longer inspectable: %s\n' "$service" >&2
      return 1
    fi

    local state
    state="$(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' "$container_id" 2>/dev/null || true)"

    case "$state" in
      "exited 0")
        return 0
        ;;
      exited\ *)
        compose -f "$compose_file" logs --tail=120 "$service" || true
        printf 'One-shot service failed: %s (%s)\n' "$service" "$state" >&2
        return 1
        ;;
      "")
        printf 'Unable to inspect one-shot service state: %s (%s)\n' "$service" "$container_id" >&2
        return 1
        ;;
      *)
        sleep "$WAIT_INTERVAL_SECONDS"
        ;;
    esac
  done

  compose -f "$compose_file" logs --tail=120 "$service" || true
  printf 'Timed out waiting for one-shot service: %s\n' "$service" >&2
  return 1
}

remove_one_shot_containers() {
  local compose_file="$1"
  local services="$2"
  local service
  local configured_services
  local existing=()

  if [[ -z "$services" ]]; then
    return 0
  fi

  configured_services="$(compose_services "$compose_file")"
  for service in $services; do
    if ! grep -qx "$service" <<<"$configured_services"; then
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
    run compose -f "$compose_file" rm -f -s -v "${existing[@]}"
  else
    for service in "${existing[@]}"; do
      local container_id
      container_id="$(compose -f "$compose_file" ps --all -q "$service" 2>/dev/null || true)"
      [[ -z "$container_id" ]] && continue

      if ! run_with_timeout "$REMOVE_TIMEOUT_SECONDS" docker rm -f -v "$container_id" >/dev/null; then
        printf '[cleanup] WARN: timed out removing one-shot service %s (%s); continuing\n' "$service" "$container_id" >&2
      fi
    done
  fi
}

run_post_build_hooks() {
  local index="$1"
  local name="${STACK_NAMES[$index]}"
  local post_hook="${POST_BUILD_HOOKS[$index]}"
  local hook
  local hooks=()

  [[ -n "$post_hook" ]] || return 0
  IFS=',' read -r -a hooks <<<"$post_hook"
  for hook in "${hooks[@]}"; do
    hook="$(printf '%s' "$hook" | xargs)"
    [[ -z "$hook" ]] && continue
    log "Running post-build hook for $name: $hook"
    "$hook"
  done
}

preflight_skip_build_images() {
  [[ "$SKIP_BUILD" == "true" && "$DRY_RUN" == "false" ]] || return 0

  local index compose_file config_json image
  local missing=()
  for ((index = START_STACK_INDEX; index <= FRONTEND_STACK_INDEX; index++)); do
    compose_file="${COMPOSE_FILES[$index]}"
    config_json="$(compose -f "$compose_file" config --format json)"
    while IFS= read -r image; do
      [[ -n "$image" ]] || continue
      if ! docker image inspect "$image" >/dev/null 2>&1; then
        missing+=("${STACK_NAMES[$index]}:$image")
      fi
    done < <(jq -r '
      .name as $project
      | .services
      | to_entries[]
      | select(.value.build != null)
      | (.value.image // (($project // "") + "-" + .key))
    ' <<<"$config_json")
  done

  if (( ${#missing[@]} > 0 )); then
    printf '[images] ERROR: --skip-build requires these local images before any plane is changed:\n' >&2
    printf '  %s\n' "${missing[@]}" >&2
    return 1
  fi
  printf '[images] OK — every selected build service has a local image\n'
}

build_stack() {
  local index="$1"
  local name="${STACK_NAMES[$index]}"
  local compose_file="${COMPOSE_FILES[$index]}"
  local bootstrap_services="${BOOTSTRAP_SERVICES[$index]}"
  local old_project="${OLD_PROJECTS[$index]}"

  ACTIVE_STACK_INDEX="$index"

  stop_old_project "$compose_file" "$old_project"

  log "Building and starting $name"
  if [[ "$SKIP_BUILD" == "true" ]]; then
    if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
      run compose -f "$compose_file" up -d --remove-orphans --no-build nats gateway frontend
    else
      run compose -f "$compose_file" up -d --remove-orphans --no-build
    fi
  elif [[ "$NO_CACHE" == "true" ]]; then
    if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
      run compose -f "$compose_file" build --no-cache gateway frontend
      run compose -f "$compose_file" up -d --no-build --remove-orphans nats gateway frontend
    else
      run compose -f "$compose_file" build --no-cache
      run compose -f "$compose_file" up -d --no-build --remove-orphans
    fi
  else
    if [[ "$index" == "$FRONTEND_STACK_INDEX" ]]; then
      run compose -f "$compose_file" up -d --build --remove-orphans nats gateway frontend
    else
      run compose -f "$compose_file" up -d --build --remove-orphans
    fi
  fi

  if [[ "$DRY_RUN" == "false" ]]; then
    remove_one_shot_containers "$compose_file" "$bootstrap_services"
    wait_for_stack_ready "$name" "$compose_file" "$bootstrap_services"

    log "$name status"
    compose -f "$compose_file" ps

    run_post_build_hooks "$index"
  fi
  ACTIVE_STACK_INDEX=-1
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

# apply_dataplane_migrations — idempotently apply the Per-User Data Ownership
# schema migrations to the running dataplane DB.
#
# Why this hook exists (same class as ensure_finspo_database):
#   `init.sql` only creates the schema on a FRESH `dpv2-postgres-data` volume.
#   On an existing volume the ownership columns (documents.owner_id/visibility)
#   and the document_acl drop never land, so the freshly-built documents-api /
#   retrieval-engine would run against a stale schema and every ownership query
#   would error. The standalone migrator can't help here: this DB is bootstrapped
#   by init.sql (not the migrator), so `migrator up` replays from zero and fails
#   on pre-init.sql migrations (e.g. retrieval_traces). We therefore apply the
#   ownership migration files DIRECTLY — they are idempotent (ADD COLUMN IF NOT
#   EXISTS / DROP TABLE IF EXISTS / DROP-then-ADD constraint), so this is safe on
#   fresh volumes (init.sql already added them → no-ops) and re-runs.
#
# Skip with APPLY_DPV2_OWNERSHIP=0.
apply_dataplane_migrations() {
  if [[ "${APPLY_DPV2_OWNERSHIP:-1}" == "0" ]]; then
    log "Data Plane ownership migration apply disabled (APPLY_DPV2_OWNERSHIP=0)"
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dpv2-migrate] dry-run: would apply ownership migrations to dataplane\n'
    return 0
  fi

  local compose_file="$CORE_ROOT/apps/Data Plane v2/docker-compose.yml"
  local container
  container="$(compose -f "$compose_file" ps -q postgres)"
  local user="${DPV2_PG_USER:-dataplane}"
  local db="${DPV2_PG_DB:-dataplane}"
  local mig_dir="$CORE_ROOT/apps/Data Plane v2/infra/postgres/migrations"

  if [[ -z "$container" ]] || ! docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null | grep -qx true; then
    printf '[dpv2-migrate] ERROR: Data Plane postgres is not running; ownership migrations cannot be applied\n' >&2
    return 1
  fi

  log "Applying Data Plane ownership migrations (owner_id/visibility + drop document_acl)"
  local f
  local failures=()
  for f in "20260620130000_add_document_ownership.sql" "20260620120000_drop_document_acl.sql"; do
    if [[ ! -f "$mig_dir/$f" ]]; then
      printf '[dpv2-migrate] ERROR: required migration is missing: %s\n' "$mig_dir/$f" >&2
      failures+=("$f")
    elif docker exec -i "$container" psql -v ON_ERROR_STOP=1 -U "$user" -d "$db" < "$mig_dir/$f" >/dev/null 2>&1; then
      printf '[dpv2-migrate] applied %s\n' "$f"
    else
      printf '[dpv2-migrate] ERROR: applying %s failed; refusing to continue with stale ownership schema\n' "$f" >&2
      failures+=("$f")
    fi
  done

  (( ${#failures[@]} == 0 ))
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
  local compose_file="$CORE_ROOT/apps/Ingestion Plane/docker-compose.yml"
  local container
  container="$(compose -f "$compose_file" ps -q postgres 2>/dev/null || true)"
  local user="${INGESTION_PG_USER:-ingestion_user}"
  local maintenance_db="${INGESTION_PG_DB:-ingestion_plane_db}"

  if [[ -z "$container" ]] || ! docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null | grep -qx true; then
    printf '[finspo-db] WARN: Ingestion postgres service is not running; skipping CREATE DATABASE finspo\n' >&2
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

  log "Ensuring finspo database exists on the Ingestion postgres service"
  # \gexec runs the SELECT's output as the next SQL statement when (and only
  # when) the database is missing — making this a true no-op for fresh
  # volumes that already got finspo from init-databases.sql.
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[dry-run] docker exec %s psql ... CREATE DATABASE finspo\n' "$container"
    return 0
  fi

  docker exec -i "$container" psql -v ON_ERROR_STOP=1 -v "finspo_owner=$user" \
      -U "$user" -d "$maintenance_db" <<'SQL' >/dev/null
SELECT format('CREATE DATABASE finspo WITH OWNER %I ENCODING %L TEMPLATE template0',
              :'finspo_owner', 'UTF8')
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
  if [[ "${SEED_DEV_ACCOUNT:-0}" != "1" ]]; then
    log "Dev account seed disabled (set SEED_DEV_ACCOUNT=1 to opt in)"
    return 0
  fi

  local email="${SEED_DEV_EMAIL:-local@velion.dev}"
  local password="${SEED_DEV_PASSWORD:?SEED_DEV_PASSWORD is required when SEED_DEV_ACCOUNT=1}"
  local name="${SEED_DEV_NAME:-Velion Local}"
  local auth_url="${SEED_AUTH_URL:-http://localhost:3011}"
  local pg="${SEED_PG_CONTAINER:-controlplane-postgres}"
  local signup_payload

  if ! command -v jq >/dev/null 2>&1; then
    printf '[seed] ERROR: jq is required for safe JSON construction when seeding is enabled\n' >&2
    return 1
  fi
  signup_payload="$(jq -nc --arg email "$email" --arg password "$password" --arg name "$name" \
    '{email: $email, password: $password, name: $name}')"

  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[seed] dry-run: would seed %s via %s\n' "$email" "$auth_url"
    return 0
  fi

  log "Seeding dev login $email (idempotent)"

  # Wait until auth-core answers at all (any HTTP status = up). Max ~60s.
  local attempts=0
  while [[ "$(curl --connect-timeout 2 --max-time 5 -s -o /dev/null -w '%{http_code}' \
      "$auth_url/api/auth/ok" 2>/dev/null || true)" != "200" ]]; do
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
  code="$(curl --connect-timeout 2 --max-time 10 -s -o /dev/null -w '%{http_code}' -X POST "$auth_url/api/auth/sign-up/email" \
    -H 'content-type: application/json' \
    --data-binary "$signup_payload" 2>/dev/null || true)"
  [[ -n "$code" ]] || code="000"
  case "$code" in
    2*) printf '[seed] created %s\n' "$email" ;;
    *)  printf '[seed] signup HTTP %s for %s (likely already exists); continuing\n' "$code" "$email" ;;
  esac

  # Promote to superadmin + mark verified so the login is usable immediately.
  if ! docker exec -i "$pg" psql -v ON_ERROR_STOP=1 -v "seed_email=$email" -U aquatiq -d auth_service -c \
      "UPDATE \"user\" SET role='superadmin', email_verified=true WHERE email=:'seed_email';" >/dev/null 2>&1; then
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
  # Each service's container healthcheck calls its DEEP /health endpoint. Using
  # container health avoids assuming a host port (user-core is expose-only).
  local checks=("org-core" "user-core" "billing-core")
  local failed=()
  local svc

  log "Verifying Control Plane cores can authenticate to Postgres (deep /health)"
  for svc in "${checks[@]}"; do
    if service_ready "$compose_file" "$svc"; then
      printf '[db-auth] %-12s OK (deep container healthcheck passed)\n' "$svc"
      continue
    fi

    printf '[db-auth] WARN: %s is unhealthy — force-recreating once to pick up current config…\n' "$svc" >&2
    run compose -f "$compose_file" up -d --force-recreate --no-deps "$svc"
    if wait_for_services_ready "$compose_file" "$svc"; then
      printf '[db-auth] %-12s recovered after recreate\n' "$svc"
    else
      printf '[db-auth] ERROR: %s is still unhealthy after recreate — likely a DB password mismatch.\n' "$svc" >&2
      compose -f "$compose_file" logs --tail 8 "$svc" 2>&1 \
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

# verify_ownership_phase — after Control + Data planes are up, confirm the
# Per-User Data Ownership & Sharing migrations actually applied. Like the finspo
# hook, this nets the "old DB volume predates the schema" case: a core can look
# "up" (port open) while running against a Postgres that never got
# resource_grants / documents.owner_id, which would make every ownership query
# fail at runtime. Checks:
#   - user_service.resource_grants  (Control Plane — user-core migration 012)
#   - documents.owner_id + visibility (Data Plane — migration 20260620130000)
#   - the shared NATS bus is reachable (the grant-revoke evictor + the GDPR
#     ownership-transfer subscriber ride velion-nats; without it revocation
#     falls back to the 5-min TTL and erasure-transfer won't run).
#
# Tolerant by default (warns, never aborts) — set STRICT_OWNERSHIP_CHECK=1 to
# fail the build when the schema is missing. Skip with VERIFY_OWNERSHIP=0.
verify_ownership_phase() {
  if [[ "${VERIFY_OWNERSHIP:-1}" == "0" ]]; then
    log "Ownership-phase verification disabled (VERIFY_OWNERSHIP=0)"
    return 0
  fi
  if [[ "$DRY_RUN" == "true" ]]; then
    printf '[ownership] dry-run: would verify resource_grants + documents.owner_id schema\n'
    return 0
  fi

  local cp_container="${SEED_PG_CONTAINER:-controlplane-postgres}"
  local cp_user="${CONTROLPLANE_PG_USER:-aquatiq}"
  local dp_container
  dp_container="$(compose -f "$CORE_ROOT/apps/Data Plane v2/docker-compose.yml" ps -q postgres)"
  local dp_user="${DPV2_PG_USER:-dataplane}"
  local dp_db="${DPV2_PG_DB:-dataplane}"
  local failed=()

  log "Verifying Per-User Data Ownership schema (resource_grants + documents.owner_id)"

  # 1) resource_grants in user_service (Control Plane).
  local rg
  rg="$(docker exec "$cp_container" psql -U "$cp_user" -d user_service -tAc \
    "SELECT to_regclass('public.resource_grants') IS NOT NULL" 2>/dev/null | tr -d '[:space:]')"
  if [[ "$rg" == "t" ]]; then
    printf '[ownership] user_service.resource_grants OK\n'
  else
    printf '[ownership] WARN: user_service.resource_grants MISSING — user-core migration 012 did not apply (old volume?). Restart user-core or re-run migrations.\n' >&2
    failed+=("resource_grants")
  fi

  # 2) documents.owner_id + visibility in dataplane (Data Plane).
  local cols
  if [[ -n "$dp_container" ]]; then
    cols="$(docker exec "$dp_container" psql -U "$dp_user" -d "$dp_db" -tAc \
      "SELECT count(*) FROM information_schema.columns WHERE table_name='documents' AND column_name IN ('owner_id','visibility')" 2>/dev/null | tr -d '[:space:]')"
  else
    cols=""
  fi
  if [[ "$cols" == "2" ]]; then
    printf '[ownership] documents.owner_id + visibility OK\n'
  else
    printf '[ownership] WARN: documents.owner_id/visibility MISSING (found %s/2) — Data Plane migration 20260620130000 did not apply (old volume?).\n' "${cols:-0}" >&2
    failed+=("documents.owner_id")
  fi

  # 3) shared NATS bus (best-effort signal, never fatal).
  if docker ps --format '{{.Names}}' | grep -qx "velion-nats"; then
    printf '[ownership] shared bus velion-nats present (grant-revoke evictor + GDPR ownership-transfer subscriber can attach)\n'
  else
    printf '[ownership] NOTE: velion-nats not found; revoke-eviction falls back to the 5-min TTL and erasure-transfer will not run until the shared bus is up.\n' >&2
  fi

  if (( ${#failed[@]} > 0 )); then
    printf '[ownership] FAILED: missing schema: %s\n' "${failed[*]}" >&2
    if [[ "${STRICT_OWNERSHIP_CHECK:-0}" == "1" ]]; then
      return 1
    fi
    printf '[ownership] Continuing (set STRICT_OWNERSHIP_CHECK=1 to fail the build here).\n' >&2
  fi
  return 0
}

# verify_internal_api_key_consistency — fleet-wide PRE-flight gate (2026-07-07
# decision: consolidate every plane on ONE Control-Plane-owned INTERNAL_API_KEY
# rather than a per-core key split). Runs before anything is built or started
# (pure file check, no containers required) and catches the two failure modes
# that actually bit this repo:
#   1. A plane .env edited in isolation drifts from the others — the classic
#      "rotated one file, forgot the rest" mistake, which silently breaks that
#      plane's calls to everyone else instead of failing loudly.
#   2. A known-insecure placeholder value (change-me-internal-service-secret,
#      dev-super-secret-internal-api-key) slips back into a .env or a compose
#      default — both were found and removed from source on 2026-07-07; this
#      catches a regression of either.
#
# Control Plane owns identity, so `apps/Control Plane/.env` is authoritative;
# every other plane .env that sets INTERNAL_API_KEY must match it byte-for-byte.
# convex-core's CONVEX_INTERNAL_SERVICE_KEY is intentionally a SEPARATE, distinct
# secret (better isolation) and is never compared — only its own INTERNAL_API_KEY
# fallback line, if present, is checked like any other plane.
#
# Tolerant by default (WARN, continue) — set STRICT_INTERNAL_KEY_CHECK=1 to fail
# the build on drift or a placeholder value (recommended for CI). Skip entirely
# with VERIFY_INTERNAL_KEY=0. Never prints the key value itself, only file paths.
verify_internal_api_key_consistency() {
  if [[ "${VERIFY_INTERNAL_KEY:-1}" == "0" ]]; then
    if [[ "$PRODUCTION" == "true" ]]; then
      printf '[internal-key] ERROR: production mode cannot disable internal-key verification\n' >&2
      return 1
    fi
    log "Internal API key consistency check disabled (VERIFY_INTERNAL_KEY=0)"
    return 0
  fi

  local strict_internal_key_check="${STRICT_INTERNAL_KEY_CHECK:-0}"
  [[ "$PRODUCTION" == "true" ]] && strict_internal_key_check=1

  local canonical_file="$CORE_ROOT/apps/Control Plane/.env"
  if [[ ! -f "$canonical_file" ]]; then
    printf '[internal-key] WARN: %s not found; consistency cannot be verified\n' "$canonical_file" >&2
    [[ "$strict_internal_key_check" == "1" ]] && return 1
    return 0
  fi

  local canonical_line canonical
  canonical_line="$(grep -m1 '^INTERNAL_API_KEY=' "$canonical_file")" || true
  canonical="${canonical_line#INTERNAL_API_KEY=}"
  canonical="$(_strip_env_quotes "$canonical")"

  if [[ -z "$canonical" || "$canonical" == *"change-me"* || "$canonical" == *"CHANGE_ME"* \
        || "$canonical" == *"dev-super-secret"* || ${#canonical} -lt 32 ]]; then
    printf '[internal-key] ERROR: Control Plane INTERNAL_API_KEY is empty, a known placeholder, or too short (<32 chars). Generate a real one: openssl rand -hex 32\n' >&2
    if [[ "$strict_internal_key_check" == "1" ]]; then
      return 1
    fi
    printf '[internal-key] Continuing (set STRICT_INTERNAL_KEY_CHECK=1 to fail the build here).\n' >&2
    return 0
  fi

  log "Verifying INTERNAL_API_KEY consistency across the fleet (Control Plane is the source of truth)"

  local drifted=()
  local scanned=0
  local f
  while IFS= read -r -d '' f; do
    [[ "$f" == "$canonical_file" ]] && continue
    local line value
    line="$(grep -m1 '^INTERNAL_API_KEY=' "$f")" || true
    [[ -z "$line" ]] && continue
    value="${line#INTERNAL_API_KEY=}"
    value="$(_strip_env_quotes "$value")"
    # An unexpanded ${...} reference isn't a literal value to compare — it's a
    # compose-interpolation placeholder, normally shadowed by that service's own
    # `environment:` block in its docker-compose.yml (which reads the real value
    # from this same Control Plane .env). Not drift.
    [[ "$value" == '${'*'}' ]] && continue
    scanned=$((scanned + 1))
    if [[ "$value" != "$canonical" ]]; then
      drifted+=("$f")
    fi
  done < <(rg --files --hidden --no-ignore -0 "$CORE_ROOT/apps" \
      -g '.env' -g '.env.*' -g '*.env' \
      -g '!*.example' -g '!*.sample' -g '!*.bak*' -g '!*.env.production' \
      -g '!**/node_modules/**' -g '!**/.next/**' -g '!**/dist/**' \
      -g '!**/target/**' -g '!**/.git/**' 2>/dev/null)

  # -n (not -l) so we can filter out comment-only mentions before collapsing
  # back to a file list — a source comment EXPLAINING why a placeholder is
  # rejected (e.g. the gateway's own change-me* guard) is the fix, not a
  # regression of the bug; only a real fallback/default usage should trip this.
  local placeholder_files=()
  if [[ "$VALIDATE_SOURCE_PLACEHOLDERS" == "true" ]]; then
    while IFS= read -r f; do
      [[ -z "$f" ]] && continue
      placeholder_files+=("$f")
    done < <(grep -rnE 'change-me-internal-service-secret|dev-super-secret-internal-api-key' \
        "$CORE_ROOT/apps" \
        --include='*.yml' --include='*.yaml' --include='*.ts' --include='*.go' --include='*.rs' \
        --exclude-dir=node_modules --exclude-dir=dist --exclude-dir=target --exclude-dir=.next \
        2>/dev/null \
      | grep -vE '_test\.|\.test\.' \
      | grep -vE ':[[:space:]]*//' \
      | cut -d: -f1 | sort -u || true)
  fi

  if (( ${#drifted[@]} == 0 )) && (( ${#placeholder_files[@]} == 0 )); then
    printf '[internal-key] OK — %d plane .env files agree with Control Plane\n' "$scanned"
    return 0
  fi

  if (( ${#drifted[@]} > 0 )); then
    printf '[internal-key] DRIFT: INTERNAL_API_KEY does NOT match Control Plane in:\n' >&2
    printf '  %s\n' "${drifted[@]}" >&2
  fi
  if (( ${#placeholder_files[@]} > 0 )); then
    printf '[internal-key] INSECURE DEFAULT still present in source:\n' >&2
    printf '  %s\n' "${placeholder_files[@]}" >&2
  fi

  if [[ "$strict_internal_key_check" == "1" ]]; then
    return 1
  fi
  printf '[internal-key] Continuing (set STRICT_INTERNAL_KEY_CHECK=1 to fail the build here).\n' >&2
  return 0
}

# Reject the literal development placeholders that otherwise satisfy Compose's
# non-empty interpolation checks. Only file and variable names are reported;
# secret values never enter logs.
validate_runtime_secrets() {
  local findings=()
  local env_file finding key compose_file production_file index
  local first_index=0
  local selected_compose_files=()
  local runtime_env_files=("$CORE_ROOT/.env")
  local required_vars

  if [[ "$MODE" == "build" ]]; then
    first_index="$START_STACK_INDEX"
  fi
  for ((index = first_index; index <= FRONTEND_STACK_INDEX; index++)); do
    compose_file="${COMPOSE_FILES[$index]}"
    selected_compose_files+=("$compose_file")
    runtime_env_files+=("$CORE_ROOT/$(dirname "$compose_file")/.env")
    if [[ "$PRODUCTION" == "true" ]]; then
      production_file="$(dirname "$compose_file")/docker-compose.production.yml"
      [[ -f "$production_file" ]] && selected_compose_files+=("$production_file")
    fi
  done

  required_vars="$({ rg -o --no-filename '\$\{[A-Z][A-Z0-9_]*:\?' \
      "${selected_compose_files[@]}" 2>/dev/null || true; } \
      | sed -E 's/^\$\{([^:]+):\?$/\1/' | sort -u)"

  for env_file in "${runtime_env_files[@]}"; do
    [[ -f "$env_file" ]] || continue
    while IFS= read -r finding; do
      [[ -z "$finding" ]] && continue
      key="${finding##*:}"
      if grep -qx "$key" <<<"$required_vars"; then
        findings+=("$finding")
      fi
    done < <(awk -F= '
      function is_placeholder(raw, normalized) {
        normalized=tolower(raw)
        return normalized == "change-me" || normalized == "changeme" ||
               normalized == "password" || normalized == "secret" ||
               normalized ~ /^replace-with-/ || normalized ~ /_placeholder$/ ||
               normalized ~ /-secret$/ || normalized ~ /^dev-super-secret/
      }
      /^[[:space:]]*#/ || !/=/ { next }
      {
        key=$1
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", key)
        value=substr($0, index($0, "=") + 1)
        gsub(/^[[:space:]"'"'']+|[[:space:]"'"'']+$/, "", value)
        # An explicitly exported, non-placeholder value has higher Compose
        # precedence and is safe to use without rewriting a local dotenv file.
        if (is_placeholder(value) &&
            (!(key in ENVIRON) || ENVIRON[key] == "" || is_placeholder(ENVIRON[key]))) {
          print FILENAME ":" key
        }
      }
    ' "$env_file")
  done

  if (( ${#findings[@]} == 0 )); then
    printf '[secrets] OK — no known placeholder values in runtime .env files\n'
    return 0
  fi

  printf '[secrets] ERROR: known placeholder values found (values withheld):\n' >&2
  printf '  %s\n' "${findings[@]}" >&2
  return 1
}

# prune_all — tear down every plane's compose project, prune any
# straggling containers/volumes, and remove `inter-plane-bus`. Idempotent:
# safe to run when nothing is up.
prune_all() {
  if [[ "${CONFIRM_DESTRUCTIVE_PRUNE:-0}" != "1" ]]; then
    printf 'Refusing destructive prune: set CONFIRM_DESTRUCTIVE_PRUNE=1 to remove containers, volumes, images, and networks.\n' >&2
    return 2
  fi

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
    run compose -f "$compose_file" down --volumes --remove-orphans --rmi local || true

    if [[ -n "$old_project" ]]; then
      if compose ls --all --format json 2>/dev/null | grep -q "\"Name\":\"$old_project\""; then
        log "Stopping legacy Compose project '$old_project'"
        run compose -p "$old_project" -f "$compose_file" down --volumes --remove-orphans || true
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
  if [[ "${PRUNE_BUILD_CACHE:-false}" == "true" ]]; then
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

validate_stack_configs() {
  local first_index="$1"
  local last_index="$2"
  local index

  for ((index = first_index; index <= last_index; index++)); do
    log "Validating ${STACK_NAMES[$index]}"
    validate_compose "${COMPOSE_FILES[$index]}"
    if [[ "${STACK_NAMES[$index]}" == "Ingestion Plane" ]]; then
      validate_ingestion_plane_targets_quarry_v2 "${COMPOSE_FILES[$index]}"
    fi
  done
}

validate_required_environment() {
  local first_index="$1"
  local last_index="$2"
  local index compose_file production_file plane_env key value
  local source_files=()
  local missing=()

  for ((index = first_index; index <= last_index; index++)); do
    compose_file="${COMPOSE_FILES[$index]}"
    source_files=("$compose_file")
    if [[ "$PRODUCTION" == "true" ]]; then
      production_file="$(dirname "$compose_file")/docker-compose.production.yml"
      [[ -f "$production_file" ]] && source_files+=("$production_file")
    fi
    plane_env="$CORE_ROOT/$(dirname "$compose_file")/.env"

    while IFS= read -r key; do
      [[ -n "$key" ]] || continue
      value="$(printenv "$key" 2>/dev/null || true)"
      if [[ -z "$value" ]]; then
        value="$(dotenv_value "$plane_env" "$key" 2>/dev/null || true)"
      fi
      if [[ -z "$value" ]]; then
        value="$(dotenv_value "$CORE_ROOT/.env" "$key" 2>/dev/null || true)"
      fi
      if [[ -z "$value" ]]; then
        missing+=("${STACK_NAMES[$index]}:$key")
      fi
    done < <(
      { rg -o --no-filename '\$\{[A-Z][A-Z0-9_]*:\?' "${source_files[@]}" 2>/dev/null || true; } \
        | sed -E 's/^\$\{([^:]+):\?$/\1/' | sort -u
    )
  done

  if (( ${#missing[@]} > 0 )); then
    printf '[environment] ERROR: required Compose variables are missing (values are never printed):\n' >&2
    printf '  %s\n' "${missing[@]}" >&2
    return 1
  fi

  printf '[environment] OK — all required variables for selected planes are present\n'
}

compose_bootstrap() {
  local index

  ensure_velion_network
  ensure_frontend_bus

  if [[ "$DRY_RUN" == "true" ]]; then
    log "Compose bootstrap dry-run: core runtime state is intentionally not queried"
    for ((index = 0; index < FRONTEND_STACK_INDEX; index++)); do
      build_stack "$index"
    done
    return 0
  fi

  if core_stacks_ready; then
    log "Core planes already running; compose bootstrap skips full rebuild."
    return 0
  fi

  check_docker_disk_space
  log "Core planes are not fully ready; compose bootstrap will build prerequisite planes."
  for ((index = 0; index < FRONTEND_STACK_INDEX; index++)); do
    build_stack "$index"
  done
}

main() {
  preflight_compose_cli

  if [[ "$MODE" != "prune" && "$MODE" != "status" ]]; then
    verify_internal_api_key_consistency
    validate_runtime_secrets
  fi

  if [[ "$MODE" == "build" ]]; then
    validate_required_environment "$START_STACK_INDEX" "$FRONTEND_STACK_INDEX"
    validate_stack_configs "$START_STACK_INDEX" "$FRONTEND_STACK_INDEX"
  elif [[ "$MODE" == "compose-bootstrap" ]]; then
    validate_required_environment 0 "$FRONTEND_STACK_INDEX"
    validate_stack_configs 0 "$FRONTEND_STACK_INDEX"
  fi

  if [[ "$DRY_RUN" == "false" ]]; then
    preflight_docker
    if [[ "$MODE" != "status" ]]; then
      acquire_orchestration_lock
    fi
    if [[ "$MODE" == "build" ]]; then
      preflight_skip_build_images
    fi
  fi

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

  local index
  for ((index = START_STACK_INDEX; index <= FRONTEND_STACK_INDEX; index++)); do
    if [[ "$RESUME_READY_STACKS" == "true" ]] && stack_runtime_ready "$index"; then
      log "Skipping ${STACK_NAMES[$index]}: every runtime service is already ready"
      ACTIVE_STACK_INDEX="$index"
      run_post_build_hooks "$index"
      ACTIVE_STACK_INDEX=-1
      continue
    fi
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
