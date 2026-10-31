# sync-credentials.sh: sourced, never run. export_sync_credentials REPO SECRETS exports what
# cmd/doffin-sync needs to sync into the local Studio stack's anbud_core: DOFFIN_API_KEY from
# REPO/.env, and ANBUD_DSN and ANBUD_MIGRATE_DSN on loopback 127.0.0.1:9540 with the passwords
# from Studio's protected secrets (the files scripts/engagement-compose.sh reads). Nothing is printed.

# sync_local_dir REPO prints Studio's .local directory, which holds the protected secrets: beside the
# repository, or in Engagement plane once anbud-core has moved to engagement-plane (2026-10-28).
sync_local_dir() {
  local dir
  for dir in "$1/../.local" "$1/../../Engagement plane/.local"; do
    if [ -d "$dir/secrets" ]; then
      (cd "$dir" && pwd)
      return 0
    fi
  done
  echo "Studio's protected secrets were not found beside $1 or in Engagement plane" >&2
  return 1
}

sync_secret() {
  local file="$1/$2"
  if [ ! -f "$file" ] || [ -L "$file" ] || [ "$(stat -f %Lp "$file")" != "600" ]; then
    echo "missing or unprotected secret: $2" >&2
    return 1
  fi
  tr -d '\n' <"$file"
}

export_sync_credentials() {
  local repo="$1" secrets="$2" app migrator
  set -a
  # shellcheck disable=SC1091
  . "$repo/.env"
  set +a
  app="$(sync_secret "$secrets" anbud_db_password)" || return 1
  migrator="$(sync_secret "$secrets" anbud_migrator_db_password)" || return 1
  export ANBUD_DSN="postgresql://anbud_app:${app}@127.0.0.1:9540/anbud_core?sslmode=disable"
  export ANBUD_MIGRATE_DSN="postgresql://anbud_migrator:${migrator}@127.0.0.1:9540/anbud_core?sslmode=disable"
}
