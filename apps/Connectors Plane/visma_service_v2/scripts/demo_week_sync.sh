#!/usr/bin/env bash
# demo_week_sync.sh
# Safe demo helper: sync only explicit endpoints for a single bounded window.
set -euo pipefail

TENANT="33f319b9-4d57-11ee-960c-025417856183"
DATE_FROM="2026-04-27T00:00:00Z"
DATE_TO="2026-05-03T23:59:59Z"
RUN=false
CONFIRM=false

ENDPOINTS=()

usage() {
  cat <<'EOF'
Usage:
  bash scripts/demo_week_sync.sh --endpoint <name> [--endpoint <name> ...] [options]

Required:
  --endpoint <name>        Endpoint to sync (repeat for multiple)

Optional:
  --tenant <uuid>          Tenant id (default: Aquatiq tenant)
  --date-from <rfc3339>    Inclusive lower bound (default: 2026-04-27T00:00:00Z)
  --date-to <rfc3339>      Inclusive upper bound (default: 2026-05-03T23:59:59Z)
  --run                    Execute sync (otherwise preview only)
  --confirm-demo           Required together with --run
  -h, --help               Show help

Safety model:
  - No endpoint defaults: you must choose endpoints explicitly.
  - "all" endpoint is rejected.
  - Execution requires both --run and --confirm-demo.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --endpoint)
      [[ $# -ge 2 ]] || { echo "Missing value for --endpoint"; exit 2; }
      ENDPOINTS+=("$2")
      shift 2
      ;;
    --tenant)
      [[ $# -ge 2 ]] || { echo "Missing value for --tenant"; exit 2; }
      TENANT="$2"
      shift 2
      ;;
    --date-from)
      [[ $# -ge 2 ]] || { echo "Missing value for --date-from"; exit 2; }
      DATE_FROM="$2"
      shift 2
      ;;
    --date-to)
      [[ $# -ge 2 ]] || { echo "Missing value for --date-to"; exit 2; }
      DATE_TO="$2"
      shift 2
      ;;
    --run)
      RUN=true
      shift
      ;;
    --confirm-demo)
      CONFIRM=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1"
      usage
      exit 2
      ;;
  esac
done

if [[ ${#ENDPOINTS[@]} -eq 0 ]]; then
  echo "At least one --endpoint is required."
  usage
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

for endpoint in "${ENDPOINTS[@]}"; do
  if [[ "$endpoint" == "all" ]]; then
    echo "Refusing to run with endpoint=all. Specify explicit endpoint names."
    exit 2
  fi

  config_file="$ROOT_DIR/config/endpoints/${endpoint}.json"
  if [[ ! -f "$config_file" ]]; then
    echo "Endpoint config not found: $config_file"
    exit 2
  fi
done

echo "Demo sync configuration"
echo "  Tenant:    $TENANT"
echo "  Date from: $DATE_FROM"
echo "  Date to:   $DATE_TO"
echo "  Endpoints: ${ENDPOINTS[*]}"
echo ""

echo "Commands:"
for endpoint in "${ENDPOINTS[@]}"; do
  echo "  docker compose run --rm -T vsv2 sync --tenant $TENANT --endpoint $endpoint --date-from $DATE_FROM --date-to $DATE_TO"
done

if ! $RUN; then
  echo ""
  echo "Preview mode only. Re-run with --run --confirm-demo to execute."
  exit 0
fi

if ! $CONFIRM; then
  echo "Safety check failed: --confirm-demo is required for execution."
  echo "Re-run with: --run --confirm-demo"
  exit 2
fi

PASS=0
FAIL=0

for endpoint in "${ENDPOINTS[@]}"; do
  echo "Running endpoint=$endpoint"
  if docker compose run --rm -T vsv2 sync \
      --tenant "$TENANT" \
      --endpoint "$endpoint" \
      --date-from "$DATE_FROM" \
      --date-to "$DATE_TO"; then
    ((PASS++)) || true
  else
    echo "endpoint=$endpoint FAILED"
    ((FAIL++)) || true
  fi
done

echo "Demo week sync completed: PASS=$PASS FAIL=$FAIL"

if [[ $FAIL -gt 0 ]]; then
  exit 1
fi
