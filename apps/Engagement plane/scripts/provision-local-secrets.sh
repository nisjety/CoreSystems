#!/bin/sh
# Provision only local development credentials. Provider OAuth credentials and
# Integration Core bearers are deliberately outside this script's authority.
set -eu

studio_root=$(CDPATH= cd "$(dirname "$0")/.." && pwd)
studio_secret_dir=${STUDIO_LOCAL_SECRETS_DIR:-"$studio_root/.local/secrets"}
if [ "$(basename "$studio_secret_dir")" != secrets ] || [ -L "$studio_secret_dir" ]; then
  echo "STUDIO_LOCAL_SECRETS_DIR must be a non-symlink directory named secrets" >&2
  exit 1
fi
if ! command -v openssl >/dev/null 2>&1; then
  echo "openssl is required to provision local development secrets" >&2
  exit 1
fi

umask 077
if [ ! -d "$studio_secret_dir" ]; then
  mkdir -p "$studio_secret_dir"
  chmod 700 "$studio_secret_dir"
fi
studio_secret_dir_mode=$(stat -f '%Lp' "$studio_secret_dir" 2>/dev/null || stat -c '%a' "$studio_secret_dir")
if [ "$studio_secret_dir_mode" != 700 ]; then
  echo "The local secrets directory must have mode 0700" >&2
  exit 1
fi

for studio_secret_name in \
  social_internal_api_key leads_internal_api_key marketing_internal_api_key \
  anbud_internal_api_key anbud_db_password anbud_migrator_db_password \
  shared_nats_studio_leaf_password shared_nats_control_leaf_password shared_nats_ingestion_leaf_password \
  studio_db_password studio_dragonfly_password studio_nats_provisioner_password \
  studio_grafana_password; do
  studio_secret_file="$studio_secret_dir/$studio_secret_name"
  if [ -e "$studio_secret_file" ] || [ -L "$studio_secret_file" ]; then
    if [ ! -f "$studio_secret_file" ] || [ -L "$studio_secret_file" ] || [ ! -s "$studio_secret_file" ]; then
      echo "$studio_secret_name must be an existing nonempty regular file" >&2
      exit 1
    fi
    studio_secret_mode=$(stat -f '%Lp' "$studio_secret_file" 2>/dev/null || stat -c '%a' "$studio_secret_file")
    case "$studio_secret_mode" in
      400|600) ;;
      *) echo "$studio_secret_name must have mode 0400 or 0600" >&2; exit 1 ;;
    esac
    continue
  fi
  studio_secret_temp=$(mktemp "$studio_secret_dir/.$studio_secret_name.XXXXXX")
  if ! openssl rand -hex 32 > "$studio_secret_temp"; then
    rm -f "$studio_secret_temp"
    echo "could not provision $studio_secret_name" >&2
    exit 1
  fi
  chmod 600 "$studio_secret_temp"
  if ! ln "$studio_secret_temp" "$studio_secret_file"; then
    rm -f "$studio_secret_temp"
    echo "could not install $studio_secret_name without overwriting it" >&2
    exit 1
  fi
  rm -f "$studio_secret_temp"
done

echo "Local Studio development secret files are ready; values were not displayed."
