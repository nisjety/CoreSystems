#!/usr/bin/env bash
# daily-sync.sh: one daily Doffin sync into the local Studio stack's anbud_core.
#
# The sync is a host-side command (docs/REQUIREMENTS.md section 9): the database is reached on
# loopback, 127.0.0.1:9540, where studio-postgres is published, so the TLS rule of db.CheckTLS
# admits sslmode=disable.
#
# The window is the last ANBUD_SYNC_LOOKBACK_DAYS closed UTC days (default 14), with the last
# four rechecked: done days are skipped, so a daily run costs about one day of downloads plus
# eight searches (docs/ARCHITECTURE.md, budget arithmetic). Another running sync holds the budget
# lock, and this one then exits 7 without calling Doffin.
#
# Two ways to run it:
#   - From the repo (scripts/daily-sync.sh): it builds cmd/doffin-sync, sources anbud-core/.env
#     for DOFFIN_API_KEY and reads the database passwords from Studio's protected secrets.
#     Log: Engagement plane/.local/logs/anbud-daily-sync.log.
#   - As installed by scripts/install-daily-sync.sh, next to a doffin-sync binary and a sync.env
#     in ~/Library/Application Support/no.triodelab.anbud: launchd may not read the external
#     volume the repo is on, so the installed copy needs nothing from it.
#     Log: ~/Library/Logs/anbud-daily-sync.log.
# Credentials are read, never printed.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
lookback="${ANBUD_SYNC_LOOKBACK_DAYS:-14}"
recheck="${ANBUD_SYNC_RECHECK_DAYS:-4}"
max_downloads="${ANBUD_SYNC_MAX_DOWNLOADS:-2000}"
case "$lookback$recheck$max_downloads" in *[!0-9]*) echo "the lookback, recheck and download limit must be whole numbers" >&2; exit 1 ;; esac

if [ -f "$here/sync.env" ]; then
  bin="$here/doffin-sync"
  log="$HOME/Library/Logs/anbud-daily-sync.log"
  exec >>"$log" 2>&1
  set -a
  # shellcheck disable=SC1091
  . "$here/sync.env"
  set +a
else
  repo="$(cd "$here/.." && pwd)"
  # shellcheck source=/dev/null
  . "$here/sync-credentials.sh"
  local_dir="$(sync_local_dir "$repo")"
  bin="$local_dir/bin/doffin-sync"
  mkdir -p "$local_dir/logs" "$local_dir/bin"
  chmod 700 "$local_dir/logs" "$local_dir/bin"
  exec >>"$local_dir/logs/anbud-daily-sync.log" 2>&1
  (cd "$repo" && GOTOOLCHAIN=local go build -o "$bin" ./cmd/doffin-sync)
  export_sync_credentials "$repo" "$local_dir/secrets"
fi

echo "=== $(date -u +%Y-%m-%dT%H:%M:%SZ) daily sync: last $lookback closed days, recheck $recheck"
if [ -z "${DOFFIN_API_KEY:-}" ] || [ -z "${ANBUD_DSN:-}" ] || [ -z "${ANBUD_MIGRATE_DSN:-}" ]; then
  echo "DOFFIN_API_KEY, ANBUD_DSN and ANBUD_MIGRATE_DSN must all be set"
  exit 1
fi

from="$(date -u -v-"${lookback}"d +%Y-%m-%d)"
to="$(date -u -v-1d +%Y-%m-%d)"

# Doffin first, then TED (buyer countries ANBUD_TED_COUNTRIES, default NOR,SWE,DNK,FIN,ISL), then BOAMP, then
# Find a Tender. The four share the budget lock, so they run one after the other; a failure of one does not skip the others.
countries="${ANBUD_TED_COUNTRIES:-NOR,SWE,DNK,FIN,ISL}"
case "$countries" in *[!A-Z,]*) echo "ANBUD_TED_COUNTRIES must be comma-separated ISO 3166-1 alpha-3 codes"; exit 1 ;; esac
worst=0
for source in doffin ted boamp fts bzp; do
  args=(-source "$source" -from "$from" -to "$to" -recheck "$recheck" -max-downloads "$max_downloads")
  [ "$source" = ted ] && args+=(-countries "$countries")
  echo "--- $source"
  set +e
  "$bin" "${args[@]}"
  code=$?
  set -e
  echo "=== $(date -u +%Y-%m-%dT%H:%M:%SZ) $source exit $code (0 done, 2 gaps remain, 3 paused by the source, 7 another sync running)"
  [ "$code" -gt "$worst" ] && worst=$code
done
exit "$worst"
