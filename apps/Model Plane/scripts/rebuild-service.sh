#!/usr/bin/env bash
set -euo pipefail

# Rebuild and recreate ONE Model Plane service on the development stack, leaving a
# rollback tag behind.
#
#   scripts/rebuild-service.sh [--dry-run] [--require-clean] [--wait-seconds N]
#                              [--env-file PATH]... <service>
#
# 1. Tags the image the service is running now as model-plane-<service>:pre-<yyyymmdd>-<sha>
#    (<sha> is the revision the running image was built from). Without that tag a rebuild
#    overwrites the only copy of the previous image and a bad deploy has nothing to go back to.
# 2. Runs `scripts/compose.sh ... up -d --no-deps --build <service>`: only that service is
#    rebuilt and recreated, its dependencies and its siblings are left alone.
# 3. Waits until the new container is healthy (or has stayed up, when the image declares no
#    healthcheck) and prints the revision it was built from and the rollback command.
#
# Development stack only. Production traffic enters through a signed artifact
# (`scripts/release-artifact.sh compose <artifact> ...`); this script refuses
# MODEL_PLANE_PRODUCTION=1 and never touches an artifact tree.
#
# --env-file is passed to Compose in front of `up`, for a worktree build that must reuse the live
# stack's environment chain. Only the path is handled here: no variable is read, echoed or logged.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="model-plane"

die() {
  echo "rebuild-service: $*" >&2
  exit 1
}

usage() {
  sed -n '4,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

dry_run=0
require_clean=0
wait_seconds=180
service=""
env_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) dry_run=1 ;;
    --require-clean) require_clean=1 ;;
    --wait-seconds)
      [[ $# -ge 2 && "$2" =~ ^[0-9]+$ && "$2" -ge 1 ]] || die "--wait-seconds needs a positive integer"
      wait_seconds="$2"
      shift
      ;;
    --env-file)
      [[ $# -ge 2 && -n "$2" ]] || die "--env-file needs a path"
      env_args+=(--env-file "$2")
      shift
      ;;
    -h | --help) usage ;;
    -*) die "unknown option '$1'" ;;
    *)
      [[ -z "$service" ]] || die "exactly one service is rebuilt per run"
      service="$1"
      ;;
  esac
  shift
done

[[ -n "$service" ]] || usage
[[ "$service" =~ ^[a-z][a-z0-9-]*$ ]] || die "'$service' is not a service name"

# Refuse before anything else, including a dry run: the answer must not depend on a flag.
[[ "${MODEL_PLANE_PRODUCTION:-0}" != "1" ]] ||
  die "refusing MODEL_PLANE_PRODUCTION=1; production deploys go through scripts/release-artifact.sh compose <artifact>"
[[ ! -f "$ROOT_DIR/artifact-metadata.env" ]] ||
  die "this tree is a release artifact; rebuilds are for the development workspace only"

dirty="unknown"
if state="$(git -C "$ROOT_DIR" status --porcelain -- . 2>/dev/null)"; then
  if [[ -z "$state" ]]; then dirty="clean"; else dirty="dirty"; fi
fi
if [[ "$dirty" == "dirty" ]]; then
  [[ "$require_clean" == "0" ]] ||
    die "the working tree has uncommitted changes under apps/Model Plane; build from a clean detached worktree"
  echo "rebuild-service: WARNING the working tree is dirty, so the new image is not reproducible from its revision label" >&2
fi

docker_cmd() { docker "$@"; }

running_container() {
  docker_cmd ps -q \
    --filter "label=com.docker.compose.project=$PROJECT" \
    --filter "label=com.docker.compose.service=$service" | head -n 1
}

today="$(date -u +%Y%m%d)"

if [[ "$dry_run" == "1" ]]; then
  echo "rebuild-service: dry run for '$service' (tree: $dirty)"
  echo "  1. tag the running image as model-plane-$service:pre-$today-<running revision>"
  echo "  2. bash scripts/compose.sh ${env_args[*]:-} up -d --no-deps --build $service"
  echo "  3. wait up to ${wait_seconds}s for the new container, then print its revision"
  exit 0
fi

command -v docker >/dev/null 2>&1 || die "docker is not available"

old_container="$(running_container)"
pre_tag=""
if [[ -n "$old_container" ]]; then
  old_image="$(docker_cmd inspect --format '{{.Image}}' "$old_container")"
  old_revision="$(docker_cmd image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$old_image" 2>/dev/null || true)"
  old_revision="${old_revision:0:8}"
  if [[ -z "$old_revision" || "$old_revision" == "<no value>" ]]; then
    old_revision="${old_image#sha256:}"
    old_revision="${old_revision:0:8}"
  fi
  base_tag="model-plane-$service:pre-$today-$old_revision"
  pre_tag="$base_tag"
  suffix=1
  # A second rebuild on the same day from the same revision must not move the first tag: that
  # tag is the image from before the day's first change.
  while existing="$(docker_cmd image inspect --format '{{.Id}}' "$pre_tag" 2>/dev/null)"; do
    [[ "$existing" != "$old_image" ]] || break
    suffix=$((suffix + 1))
    pre_tag="$base_tag-$suffix"
  done
  docker_cmd tag "$old_image" "$pre_tag"
  echo "rebuild-service: running image of '$service' tagged $pre_tag"
else
  echo "rebuild-service: '$service' is not running, so there is no previous image to tag" >&2
fi

bash "$ROOT_DIR/scripts/compose.sh" "${env_args[@]}" up -d --no-deps --build "$service"

new_container="$(running_container)"
[[ -n "$new_container" ]] || die "'$service' has no running container after the rebuild"

deadline=$((SECONDS + wait_seconds))
stable_since=-1
restarts_at_stable=""
healthy=0
while ((SECONDS < deadline)); do
  state="$(docker_cmd inspect --format '{{.State.Status}}|{{if .State.Health}}{{.State.Health.Status}}{{end}}|{{.RestartCount}}' "$new_container")"
  status="${state%%|*}"
  rest="${state#*|}"
  health="${rest%%|*}"
  restarts="${rest#*|}"
  if [[ "$status" == "exited" || "$status" == "dead" ]]; then
    break
  fi
  if [[ "$health" == "healthy" ]]; then
    healthy=1
    break
  fi
  if [[ -z "$health" && "$status" == "running" ]]; then
    # No healthcheck in the image: running and not restarting for 10 s is the best signal there is.
    if [[ "$stable_since" -lt 0 || "$restarts" != "$restarts_at_stable" ]]; then
      stable_since=$SECONDS
      restarts_at_stable="$restarts"
    elif ((SECONDS - stable_since >= 10)); then
      healthy=1
      break
    fi
  else
    stable_since=-1
  fi
  sleep 2
done

new_image="$(docker_cmd inspect --format '{{.Image}}' "$new_container")"
new_revision="$(docker_cmd image inspect --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' "$new_image" 2>/dev/null || true)"
new_created="$(docker_cmd image inspect --format '{{.Created}}' "$new_image" 2>/dev/null || true)"

echo "rebuild-service: $service container=$new_container image=${new_image:0:19} revision=${new_revision:-unknown} created=${new_created:-unknown}"

if [[ "$healthy" != "1" ]]; then
  echo "rebuild-service: '$service' did not become healthy within ${wait_seconds}s (read: docker logs $new_container)" >&2
  if [[ -n "$pre_tag" ]]; then
    echo "rebuild-service: roll back with: docker tag $pre_tag model-plane-$service:latest && bash scripts/compose.sh up -d --no-deps --no-build $service" >&2
  fi
  exit 1
fi

echo "rebuild-service: '$service' is healthy"
if [[ -n "$pre_tag" ]]; then
  echo "rebuild-service: roll back with: docker tag $pre_tag model-plane-$service:latest && bash scripts/compose.sh up -d --no-deps --no-build $service"
fi
