#!/usr/bin/env bash
# WS02-S1 / WS01-S3 / WS01-S4: no Space flag may default ON in a release artifact.
#
# A pure text lint (no docker engine needed), in the style of the other compose contract tests:
#   * docker-compose.production.yml and docker-compose.release.yml: every `SPACE_*` environment
#     entry must be off ("false", "0", "off", "no", or a `${VAR:-<off word>}` default). The base
#     file's dev defaults and docker-compose.override.yml (dev-only) are not release artifacts.
#   * docker-compose.yml (the base the release files overlay): the two human-post switches keep
#     their default OFF (`:-0`), and SPACE_REALTIME_EMIT's release overlays state "false".
#
# The script lints the real files, then proves the lint itself by feeding it fixtures that turn a
# flag on (each must fail) and ones that keep it off (each must pass).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEPLOY="$ROOT_DIR/deploy"

fail() { echo "space flags compose lint: $*" >&2; exit 1; }

is_on_word() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    true | 1 | on | yes | enabled) return 0 ;;
    *) return 1 ;;
  esac
}

# Prints "<flag> <effective default>" for every SPACE_* entry of a compose file. The effective
# default of `${VAR:-x}` / `${VAR-x}` is x; a quoted or bare literal is itself.
space_flag_defaults() {
  tr -d '\r' < "$1" | awk '
    /^[[:space:]]*#/ { next }
    match($0, /^[[:space:]]*SPACE_[A-Z0-9_]+:/) {
      line = $0
      sub(/^[[:space:]]*/, "", line)
      name = line
      sub(/:.*/, "", name)
      value = line
      sub(/^[^:]*:[[:space:]]*/, "", value)
      sub(/[[:space:]]+#.*$/, "", value)
      gsub(/^"|"$/, "", value)
      gsub(/^'\''|'\''$/, "", value)
      if (value ~ /^\$\{[A-Za-z0-9_]+:?-/) {
        sub(/^\$\{[A-Za-z0-9_]+:?-/, "", value)
        sub(/\}$/, "", value)
      }
      print name, value
    }'
}

# Fails when any SPACE_* flag in the file defaults on.
lint_release_file() {
  local file="$1" name value found=0
  [[ -f "$file" ]] || fail "missing $file"
  while read -r name value; do
    [[ -n "${name:-}" ]] || continue
    found=$((found + 1))
    if is_on_word "${value:-}"; then
      echo "space flags compose lint: $(basename "$file"): $name defaults on ($value)" >&2
      return 1
    fi
  done < <(space_flag_defaults "$file")
  [[ "$found" -gt 0 ]] || { echo "space flags compose lint: $(basename "$file"): no SPACE_* flag found" >&2; return 1; }
  return 0
}

for release in docker-compose.production.yml docker-compose.release.yml; do
  lint_release_file "$DEPLOY/$release" || exit 1
done

# The base file keeps the human-post switches dark by default.
base_defaults="$(space_flag_defaults "$DEPLOY/docker-compose.yml")"
for flag in SPACE_HUMAN_POSTS_API SPACE_HUMAN_POSTS_CORE; do
  value="$(awk -v f="$flag" '$1 == f { print $2; exit }' <<<"$base_defaults")"
  [[ -n "$value" ]] || fail "docker-compose.yml does not set $flag"
  is_on_word "$value" && fail "docker-compose.yml: $flag defaults on ($value)"
done

# The lint proves itself on fixtures.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
write_fixture() {
  printf 'services:\n  session-core:\n    environment:\n      %s\n' "$2" > "$WORK/$1.yml"
}
for bad in 'SPACE_REALTIME_EMIT: "true"' 'SPACE_REALTIME_EMIT: true' 'SPACE_REALTIME_EMIT: "1"' \
  'SPACE_HUMAN_POSTS_CORE: ${SPACE_HUMAN_POSTS_CORE:-1}' 'SPACE_HUMAN_POSTS_API: ${SPACE_HUMAN_POSTS_API:-true}' \
  'SPACE_STREAM_X: "on"'; do
  write_fixture bad "$bad"
  if lint_release_file "$WORK/bad.yml" 2>/dev/null; then
    fail "the lint accepted a flag that defaults on: $bad"
  fi
done
for good in 'SPACE_REALTIME_EMIT: "false"' 'SPACE_REALTIME_EMIT: "0"' \
  'SPACE_HUMAN_POSTS_CORE: ${SPACE_HUMAN_POSTS_CORE:-0}' 'SPACE_EVENT_LOG_RETENTION_DAYS: "14"'; do
  write_fixture good "$good"
  lint_release_file "$WORK/good.yml" || fail "the lint refused an off flag: $good"
done
printf 'services:\n  x:\n    environment:\n      OTHER: "true"\n' > "$WORK/none.yml"
if lint_release_file "$WORK/none.yml" 2>/dev/null; then
  fail "the lint passed a file with no SPACE_* flag at all"
fi

echo "space flags compose lint: ok"
