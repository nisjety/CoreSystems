#!/bin/sh
# One-time local import of an EXISTING Google OAuth client secret. This never
# creates or rotates a provider credential, and never prints its value.
set -eu

root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
source_env=${INTEGRATION_GOOGLE_SOURCE_ENV:-"$root/../Ingestion Plane/integration-corev2/.env"}
secret_dir=${STUDIO_LOCAL_SECRETS_DIR:-"$root/.local/secrets"}
target="$secret_dir/integration_google_client_secret"

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

secret=$(awk '
  /^GOOGLE_CLIENT_SECRET=/ {
    count++
    value = substr($0, index($0, "=") + 1)
    sub(/\r$/, "", value)
  }
  END {
    if (count != 1 || value == "" || value ~ /[[:space:]\047\"$\\]/) exit 2
    print value
  }
' "$source_env") || {
  echo "Expected one nonempty, unquoted GOOGLE_CLIENT_SECRET literal" >&2
  exit 1
}

if [ -e "$target" ] || [ -L "$target" ]; then
  if [ ! -f "$target" ] || [ -L "$target" ] || [ "$(file_mode "$target")" != 600 ] ||
     [ "$(cat "$target")" != "$secret" ]; then
    echo "Existing Google client secret file differs or has unsafe permissions" >&2
    exit 1
  fi
  echo "Existing protected Google client secret file verified."
  exit 0
fi

umask 077
temporary=$(mktemp "$secret_dir/.integration_google_client_secret.XXXXXX")
trap 'rm -f "$temporary"' EXIT INT TERM
printf '%s' "$secret" > "$temporary"
chmod 600 "$temporary"
ln "$temporary" "$target" || {
  echo "Could not install Google client secret without overwriting an existing file" >&2
  exit 1
}
echo "Existing Google client secret imported to the protected local store."
