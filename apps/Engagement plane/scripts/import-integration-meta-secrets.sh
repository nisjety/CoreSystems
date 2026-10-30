#!/bin/sh
# Import EXISTING Meta app credentials into the protected local store. The
# canonical Integration env is parsed as literal data, never executed, and
# provider credentials are never printed, created, rotated, or overwritten.
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
source_env=${INTEGRATION_META_SOURCE_ENV:-${INTEGRATION_LOCAL_SOURCE_ENV:-"$root/../Ingestion Plane/integration-corev2/.env"}}
secret_dir=${STUDIO_LOCAL_SECRETS_DIR:-"$root/.local/secrets"}
meta_file="$secret_dir/integration_meta_app_client_secret"
facebook_file="$secret_dir/integration_facebook_client_secret"
temporary=
trap 'if [ -n "$temporary" ]; then rm -f "$temporary"; fi' EXIT INT TERM

file_mode() {
  stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
}

if [ ! -f "$source_env" ] || [ -L "$source_env" ] || [ "$(file_mode "$source_env")" != 600 ]; then
  echo "The Integration source env must be a regular mode-0600 file" >&2
  exit 1
fi
if [ "$(basename "$secret_dir")" != secrets ] || [ ! -d "$secret_dir" ] ||
   [ -L "$secret_dir" ] || [ "$(file_mode "$secret_dir")" != 700 ]; then
  echo "The local secrets directory must be a non-symlink mode-0700 directory named secrets" >&2
  exit 1
fi

read_literal() {
  awk -v wanted="$1" -v required="$2" '
    {
      separator = index($0, "=")
      if (separator == 0 || substr($0, 1, separator - 1) != wanted) next
      count++
      value = substr($0, separator + 1)
      sub(/\r$/, "", value)
    }
    END {
      if (count > 1 || (required == "1" && (count != 1 || value == "")) || value ~ /[[:space:]\047"$\\]/) exit 2
      if (value != "") print value
    }
  ' "$source_env"
}

meta_id=$(read_literal META_APP_CLIENT_ID 1) || {
  echo "Expected one nonempty, unquoted META_APP_CLIENT_ID literal" >&2
  exit 1
}
meta_secret=$(read_literal META_APP_CLIENT_SECRET 1) || {
  echo "Expected one nonempty, unquoted META_APP_CLIENT_SECRET literal" >&2
  exit 1
}
facebook_id=$(read_literal FACEBOOK_CLIENT_ID 0) || {
  echo "FACEBOOK_CLIENT_ID must be a unique, unquoted literal when present" >&2
  exit 1
}
facebook_secret=$(read_literal FACEBOOK_CLIENT_SECRET 0) || {
  echo "FACEBOOK_CLIENT_SECRET must be a unique, unquoted literal when present" >&2
  exit 1
}
case "$meta_id" in
  *[!0-9]*|"") echo "META_APP_CLIENT_ID must be a numeric app ID" >&2; exit 1 ;;
esac
if [ -n "$facebook_id" ] || [ -n "$facebook_secret" ]; then
  case "$facebook_id" in
    *[!0-9]*|"") echo "The optional Facebook pair requires a numeric FACEBOOK_CLIENT_ID" >&2; exit 1 ;;
  esac
  if [ -z "$facebook_secret" ]; then
    echo "FACEBOOK_CLIENT_ID and FACEBOOK_CLIENT_SECRET must be configured together" >&2
    exit 1
  fi
fi

verify_existing() {
  if [ -e "$1" ] || [ -L "$1" ]; then
    if [ ! -f "$1" ] || [ -L "$1" ] || [ "$(file_mode "$1")" != 600 ] ||
       [ "$(cat "$1")" != "$2" ]; then
      echo "Existing $3 secret file differs or has unsafe permissions" >&2
      exit 1
    fi
  fi
}

# Validate every active target before writing anything. A stale Facebook file
# must not leave a partially imported unified Meta pair.
verify_existing "$meta_file" "$meta_secret" "Meta app"
if [ -n "$facebook_id" ]; then
  verify_existing "$facebook_file" "$facebook_secret" "Facebook app"
fi

install_missing() {
  if [ -e "$1" ] || [ -L "$1" ]; then
    verify_existing "$1" "$2" "provider"
    return
  fi
  umask 077
  temporary=$(mktemp "$secret_dir/.integration_provider_secret.XXXXXX")
  printf '%s' "$2" > "$temporary"
  chmod 600 "$temporary"
  if ! ln "$temporary" "$1"; then
    echo "Could not install provider secret without overwriting an existing file" >&2
    exit 1
  fi
  rm -f "$temporary"
  temporary=
}

install_missing "$meta_file" "$meta_secret"
if [ -n "$facebook_id" ]; then
  install_missing "$facebook_file" "$facebook_secret"
fi
echo "Existing Meta provider credentials verified in the protected local store."
