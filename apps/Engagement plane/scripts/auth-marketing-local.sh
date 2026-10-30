#!/bin/sh
# Local-only Auth deployment wrapper. Never provisions a token or restarts by itself.
set -eu
studio_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
control_root="$studio_root/../Control Plane"
secret_dir=${STUDIO_LOCAL_SECRETS_DIR:-"$studio_root/.local/secrets"}
source_env="$secret_dir/control_plane_compose"
registry_file="$secret_dir/marketing_auth_plane_principals.json"

file_mode() {
  stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
}

if [ "$(basename "$secret_dir")" != secrets ] || [ -L "$secret_dir" ] ||
   [ ! -d "$secret_dir" ] || [ "$(file_mode "$secret_dir")" != 700 ]; then
  echo 'Use a mode-0700 protected secrets directory named secrets' >&2
  exit 1
fi
for file in "$source_env" "$registry_file"; do
  if [ ! -f "$file" ] || [ -L "$file" ] || [ "$(file_mode "$file")" != 600 ]; then
    echo 'Auth deployment inputs must be regular mode-0600 protected files' >&2
    exit 1
  fi
done

export CONTROL_PLANE_SECRETS_FILE="$source_env"
export STUDIO_MARKETING_AUTH_PRINCIPALS_FILE="$registry_file"
exec bash "$control_root/scripts/run-control-plane.sh" \
  -f "$studio_root/docker-compose.auth-marketing.local.yml" "$@"
