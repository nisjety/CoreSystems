#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() {
  echo "model-plane compose error: $*" >&2
  exit 1
}

canonical_runtime_file() {
  local path="$1" directory basename canonical
  [[ "$path" == /* ]] ||
    die "MODEL_PLANE_RUNTIME_ENV_FILE must be an absolute canonical path"
  [[ -r "$path" && -f "$path" && ! -L "$path" ]] ||
    die "runtime environment file is not a readable regular file"
  directory="$(dirname -- "$path")"
  basename="$(basename -- "$path")"
  [[ -d "$directory" ]] || die "runtime environment directory does not exist"
  canonical="$(cd "$directory" && pwd -P)/$basename"
  printf '%s\n' "$canonical"
}

artifact_mode=0
if [[ -f "$ROOT_DIR/artifact-metadata.env" ]]; then
  artifact_mode=1
fi

if [[ "$artifact_mode" == "1" ]]; then
  # This script was copied into an immutable release artifact. Never select a
  # workspace .env or a caller-provided image lock in this mode: both would
  # make rollback depend on mutable local state.
  [[ "${MODEL_PLANE_PRODUCTION:-0}" == "1" ]] ||
    die "artifact-contained Compose requires MODEL_PLANE_PRODUCTION=1"
  [[ "${MODEL_PLANE_BRIDGES:-0}" != "1" ]] ||
    die "artifact-contained Compose does not permit optional bridge overlays"
  [[ -z "${MODEL_PLANE_RELEASE_LOCK:-}" ]] ||
    die "artifact-contained Compose selects its own immutable image lock"
  [[ -n "${MODEL_PLANE_RUNTIME_ENV_FILE:-}" ]] ||
    die "artifact-contained Compose requires MODEL_PLANE_RUNTIME_ENV_FILE"
  runtime_env_source="$(canonical_runtime_file "$MODEL_PLANE_RUNTIME_ENV_FILE")"

  # The artifact is signed, but arbitrary Compose argv are not. Accept only the
  # two commands constructed by release-artifact.sh. In particular, reject
  # trailing -f/--file, --env-file, profile, run, entrypoint, command, and
  # service overrides that could supersede the signed release layers.
  if [[ "$#" == 2 && "$1" == "config" && "$2" == "--quiet" ]]; then
    :
  elif [[ "$#" == 4 && "$1" == "up" && "$2" == "-d" && "$3" == "--wait" && "$4" == "--no-build" ]]; then
    :
  else
    die "artifact-contained Compose accepts only fixed config validation or fixed deployment"
  fi

  # The copied verifier covers the copied Compose files, migration snapshot,
  # release gates and signature. It runs before Docker is invoked.
  MODEL_PLANE_RELEASE_MODE=1 "$ROOT_DIR/scripts/release-artifact.sh" verify "$ROOT_DIR"
  runtime_snapshot_dir="$(mktemp -d "${TMPDIR:-/tmp}/model-plane-runtime-env.XXXXXX")"
  chmod 700 "$runtime_snapshot_dir"
  runtime_snapshot_dir="$(cd "$runtime_snapshot_dir" && pwd -P)"
  trap 'rm -rf "$runtime_snapshot_dir"' EXIT
  MODEL_PLANE_RUNTIME_ENV_FILE="$runtime_snapshot_dir/runtime.env"
  export MODEL_PLANE_RUNTIME_ENV_FILE
  "$ROOT_DIR/scripts/release-artifact.sh" snapshot-runtime-config \
    "$runtime_env_source" "$ROOT_DIR/config-policy.tsv" \
    "$ROOT_DIR/runtime-public-policy.env" "$ROOT_DIR/compatibility-gates.env" \
    "$MODEL_PLANE_RUNTIME_ENV_FILE"

  compose_files=(
    -f "$ROOT_DIR/deploy/docker-compose.yml"
    -f "$ROOT_DIR/deploy/docker-compose.production.yml"
    -f "$ROOT_DIR/deploy/docker-compose.release.yml"
  )
  env_files=(
    --env-file "$ROOT_DIR/runtime-public-policy.env"
    --env-file "$MODEL_PLANE_RUNTIME_ENV_FILE"
    --env-file "$ROOT_DIR/images.lock.env"
  )

  # Docker Compose normally lets its parent environment override --env-file.
  # Use a narrow client environment so neither shell secrets nor caller-supplied
  # image variables can mutate an artifact deployment. Docker connection and
  # credential-location variables are retained for an explicitly chosen daemon.
  docker_env=(env -i "PATH=$PATH")
  [[ -n "${HOME:-}" ]] && docker_env+=("HOME=$HOME")
  for variable in DOCKER_HOST DOCKER_CONTEXT DOCKER_CONFIG DOCKER_TLS DOCKER_TLS_VERIFY DOCKER_CERT_PATH DOCKER_API_VERSION DOCKER_DEFAULT_PLATFORM; do
    value="${!variable:-}"
    [[ -n "$value" ]] && docker_env+=("$variable=$value")
  done
  # Keep this shell alive until Compose has consumed the validated snapshot so
  # the EXIT trap can remove the temporary credential-bearing file.
  "${docker_env[@]}" docker compose "${env_files[@]}" "${compose_files[@]}" "$@"
  exit $?
fi

ENV_FILE="$ROOT_DIR/deploy/.env"
if [[ ! -r "$ENV_FILE" ]]; then
  echo "canonical environment file is not readable: $ENV_FILE" >&2
  exit 1
fi

export SOURCE_REVISION="${SOURCE_REVISION:-$(git -C "$ROOT_DIR" rev-parse HEAD)}"
export BUILD_DATE="${BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

compose_files=(
  -f "$ROOT_DIR/deploy/docker-compose.yml"
)

production="${MODEL_PLANE_PRODUCTION:-0}"

# A workspace tree and its env/lock files are mutable. Production or rollback
# traffic must enter through `release-artifact.sh compose <artifact> ...`,
# whose copied runner verifies the signed artifact and uses only its snapshot.
if [[ "$production" == "1" ]]; then
  die "production Compose must use scripts/release-artifact.sh compose <artifact> <arguments...>"
fi

if [[ "$production" != "1" && "${MODEL_PLANE_DEV:-1}" != "0" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.override.yml")
fi

if [[ "${MODEL_PLANE_BRIDGES:-0}" == "1" ]]; then
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.bridges.yml")
fi

env_files=(--env-file "$ENV_FILE")
release_lock="${MODEL_PLANE_RELEASE_LOCK:-}"
if [[ -n "$release_lock" ]]; then
  if [[ "$release_lock" != /* ]]; then
    release_lock="$ROOT_DIR/$release_lock"
  fi
  "$ROOT_DIR/scripts/release-artifact.sh" validate-lock "$release_lock"
  env_files+=(--env-file "$release_lock")
  # The release layer replaces every image with an artifact-contained content
  # ID and disables pulls. Keep it last so Compose cannot fall back to a
  # mutable local build or tag resolution.
  compose_files+=(-f "$ROOT_DIR/deploy/docker-compose.release.yml")
fi

exec docker compose "${env_files[@]}" "${compose_files[@]}" "$@"
