#!/usr/bin/env bash
# Sourced by run-application-plane.sh (and its test). Defines
# warn_if_convex_image_stale; running it directly does nothing.
#
# Why (finding F10): the convex-gateway / convex-subscriber image bakes
# convex-core/startup.sh and convex-core/nats-subscriber.js at build time; only
# convex-core/convex/ is bind-mounted. After editing either baked file, a stale
# image keeps pushing the OLD startup settings (none of the new Space settings
# reach the backend) and rooms sit at pending_registration with nothing in the
# logs to say why. This is the cheap early signal.
#
# It only ever WARNS: it returns 0 whatever it finds, and is silent when docker,
# the containers, the images or the date/stat tools are unavailable.

# file_mtime FILE -> epoch seconds (GNU stat, then BSD stat); status 1 when unknown.
_convex_file_mtime() {
  local value
  value=$(stat -c %Y "$1" 2>/dev/null) || value=$(stat -f %m "$1" 2>/dev/null) || return 1
  [[ "$value" =~ ^[0-9]+$ ]] || return 1
  printf '%s' "$value"
}

# RFC3339 timestamp (docker's .Created) -> epoch seconds; status 1 when unparseable.
_convex_iso_epoch() {
  local value="$1" trimmed epoch
  epoch=$(date -u -d "$value" +%s 2>/dev/null) && [[ "$epoch" =~ ^[0-9]+$ ]] && { printf '%s' "$epoch"; return 0; }
  # BSD date has no -d: drop the fractional seconds and the zone designator.
  trimmed="${value%%.*}"
  trimmed="${trimmed%Z}"
  epoch=$(date -u -j -f '%Y-%m-%dT%H:%M:%S' "$trimmed" +%s 2>/dev/null) && [[ "$epoch" =~ ^[0-9]+$ ]] && { printf '%s' "$epoch"; return 0; }
  return 1
}

_convex_epoch_text() {
  date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -r "$1" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || printf '%s' "$1"
}

# warn_if_convex_image_stale ROOT [PROJECT]
# ROOT is apps/Application Plane; PROJECT is the Compose project name used to
# find the image of a service whose container does not exist yet.
warn_if_convex_image_stale() {
  local root="$1" project="${2:-application-plane}"
  local baked=(startup.sh nats-subscriber.js)
  local file epoch newest_file="" newest_epoch=0
  local service image_ref created created_epoch stale=()

  command -v docker >/dev/null 2>&1 || return 0

  for file in "${baked[@]}"; do
    epoch=$(_convex_file_mtime "$root/convex-core/$file") || continue
    if (( epoch > newest_epoch )); then
      newest_epoch=$epoch
      newest_file=$file
    fi
  done
  [[ -n "$newest_file" ]] || return 0

  for service in convex-gateway convex-subscriber; do
    # The running container's image first; else the image Compose would reuse.
    image_ref=$(docker inspect --format '{{.Image}}' "$service" 2>/dev/null) || image_ref=""
    [[ -n "$image_ref" ]] || image_ref="${project}-${service}"
    created=$(docker image inspect --format '{{.Created}}' "$image_ref" 2>/dev/null) || continue
    created_epoch=$(_convex_iso_epoch "$created") || continue
    if (( created_epoch < newest_epoch )); then
      stale+=("$service (image built $(_convex_epoch_text "$created_epoch"))")
    fi
  done

  if (( ${#stale[@]} > 0 )); then
    {
      printf 'WARNING: convex-core/%s changed %s, after the image of:\n' "$newest_file" "$(_convex_epoch_text "$newest_epoch")"
      printf '  - %s\n' "${stale[@]}"
      printf '  startup.sh and nats-subscriber.js are baked into the image (only convex-core/convex/ is bind-mounted),\n'
      printf '  so a stale image pushes none of the new Space settings and rooms stay pending_registration.\n'
      printf '  Rebuild and recreate: scripts/run-application-plane.sh build convex-gateway convex-subscriber\n'
      printf '                        scripts/run-application-plane.sh up -d --force-recreate convex-gateway convex-subscriber\n'
      printf '  (This is a heuristic on file and image timestamps; it never blocks the command.)\n'
    } >&2
  fi
  return 0
}
