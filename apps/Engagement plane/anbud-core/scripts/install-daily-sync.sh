#!/usr/bin/env bash
# install-daily-sync.sh: runs the daily Doffin sync every morning on this Mac through launchd.
#
#   scripts/install-daily-sync.sh            install, or reinstall after the sync code changes
#   scripts/install-daily-sync.sh uninstall  remove it, with its binary and credentials
#
# launchd jobs may not read the external volume the repo is on (macOS answers "Operation not
# permitted"), so the job gets everything it needs in ~/Library/Application Support/<label>:
# a doffin-sync built now from the working tree, a copy of daily-sync.sh, and sync.env (mode
# 600) with DOFFIN_API_KEY and the two loopback DSNs. Reinstall after changing the sync or
# rotating a credential. The job runs at 06:15 local time and once at load (at login), so a
# morning the Mac slept through is caught up. Credentials are written, never printed.
set -euo pipefail

label="no.triodelab.anbud-daily-sync"
plist="$HOME/Library/LaunchAgents/$label.plist"
appdir="$HOME/Library/Application Support/$label"
scripts="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$scripts/.." && pwd)"
domain="gui/$(id -u)"

launchctl bootout "$domain/$label" 2>/dev/null || true
if [ "${1:-}" = "uninstall" ]; then
  rm -f "$plist"
  rm -rf "$appdir"
  echo "removed $label, its binary and its credentials"
  exit 0
fi

mkdir -p "$appdir" "$HOME/Library/Logs" "$(dirname "$plist")"
chmod 700 "$appdir"
(cd "$repo" && GOTOOLCHAIN=local go build -o "$appdir/doffin-sync" ./cmd/doffin-sync)
cp "$scripts/daily-sync.sh" "$appdir/daily-sync.sh"
chmod 700 "$appdir/daily-sync.sh"

(
  # shellcheck source=/dev/null
  . "$scripts/sync-credentials.sh"
  local_dir="$(sync_local_dir "$repo")" || exit 1
  export_sync_credentials "$repo" "$local_dir/secrets"
  umask 077
  for name in DOFFIN_API_KEY ANBUD_DSN ANBUD_MIGRATE_DSN; do
    value="${!name:-}"
    case "$value" in
      "" | *[!A-Za-z0-9:/@._?=-]*) echo "$name is missing or has characters this installer will not write" >&2; exit 1 ;;
    esac
    printf "%s='%s'\n" "$name" "$value"
  done >"$appdir/sync.env.new"
  mv "$appdir/sync.env.new" "$appdir/sync.env"
)
chmod 600 "$appdir/sync.env"

cat >"$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array><string>/bin/bash</string><string>$appdir/daily-sync.sh</string></array>
  <key>StartCalendarInterval</key>
  <dict><key>Hour</key><integer>6</integer><key>Minute</key><integer>15</integer></dict>
  <key>RunAtLoad</key><true/>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/$label.log</string>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
PLIST
chmod 644 "$plist"
launchctl bootstrap "$domain" "$plist"
echo "installed $label: daily at 06:15 and at login; log in ~/Library/Logs/anbud-daily-sync.log"
