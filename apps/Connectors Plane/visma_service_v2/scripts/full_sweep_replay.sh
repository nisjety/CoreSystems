#!/usr/bin/env bash
# full_sweep_replay.sh — Run 5-window incremental sync for all 73 endpoints
# Usage: bash scripts/full_sweep_replay.sh [--dry-run]
set -euo pipefail

TENANT="33f319b9-4d57-11ee-960c-025417856183"
WINDOWS=("2026-04-27" "2026-04-28" "2026-04-29" "2026-04-30" "2026-05-01")
# RFC3339 UTC windows: computed inline from each date string
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

# Safety guard: require explicit opt-in for non-dry full sweeps.
if ! $DRY_RUN && [[ "${ALLOW_FULL_SWEEP:-}" != "YES" ]]; then
  echo "Refusing to run full sweep without explicit opt-in."
  echo "Set ALLOW_FULL_SWEEP=YES to proceed, or pass --dry-run."
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

mapfile -t ENDPOINTS < <(
  find "$ROOT_DIR/config/endpoints" -maxdepth 1 -type f -name '*.json' ! -name '_schema.json' -exec basename {} .json \; | sort
)
TOTAL=${#ENDPOINTS[@]}
LOG_FILE="$ROOT_DIR/docs/evidence/full_sweep_$(date +%Y-%m-%d_%H%M%S).log"

mkdir -p "$ROOT_DIR/docs/evidence"

log() { echo "[$(date +%T)] $*" | tee -a "$LOG_FILE"; }

log "=== Full Sweep Replay ==="
log "Tenant:    $TENANT"
log "Endpoints: $TOTAL"
log "Windows:   ${WINDOWS[*]}"
log "Dry-run:   $DRY_RUN"
log "Guard:     ALLOW_FULL_SWEEP=${ALLOW_FULL_SWEEP:-<unset>}"
log "Log:       $LOG_FILE"
log ""

PASS=0
FAIL=0
SKIP=0

for ep_idx in "${!ENDPOINTS[@]}"; do
  ep="${ENDPOINTS[$ep_idx]}"
  ep_num=$((ep_idx + 1))
  log "[$ep_num/$TOTAL] endpoint=$ep"

  for win_idx in "${!WINDOWS[@]}"; do
    _win="${WINDOWS[$win_idx]}"
    date_from="${_win}T00:00:00Z"
    date_to="${_win}T23:59:59Z"

    if $DRY_RUN; then
      log "  [DRY] window=$date_from docker compose run --rm vsv2 sync --tenant $TENANT --endpoint $ep --date-from $date_from --date-to $date_to"
      ((SKIP++)) || true
      continue
    fi

    log "  window=$date_from ..."
    if docker compose -f "$ROOT_DIR/docker-compose.yml" run --rm -T vsv2 \
        sync \
        --tenant "$TENANT" \
        --endpoint "$ep" \
        --date-from "$date_from" \
        --date-to "$date_to" \
        >> "$LOG_FILE" 2>&1; then
      log "  window=$date_from OK"
      ((PASS++)) || true
    else
      log "  window=$date_from FAILED (exit $?)"
      ((FAIL++)) || true
    fi
  done

  log ""
done

log "=== Done ==="
log "PASS=$PASS  FAIL=$FAIL  SKIP=$SKIP"

if [[ $FAIL -gt 0 ]]; then
  log "WARNING: $FAIL window(s) failed — review $LOG_FILE"
  exit 1
fi
