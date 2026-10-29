#!/usr/bin/env bash
set -euo pipefail

# Rebuilds the Quickwit read model from canonical Postgres through
# quickwit-adapter-rs. This is the deterministic recovery/backfill path for
# knowledge_units, wiki versions, source_objects, retrieval logs, and source
# logs.

ADAPTER_URL="${ADAPTER_URL:-http://127.0.0.1:9204}"
ORG_ID=""
CLEAR=true

usage() {
  cat <<'USAGE'
Usage: scripts/rebuild-quickwit.sh [--org ORG_ID] [--no-clear]

Options:
  --org ORG_ID   Rebuild only one org. Without this, all orgs are rebuilt.
  --no-clear     Do not clear existing Quickwit data before replay.

Environment:
  ADAPTER_URL    quickwit-adapter base URL (default: http://127.0.0.1:9204)
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --org)
      ORG_ID="${2:-}"
      shift 2
      ;;
    --no-clear)
      CLEAR=false
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -n "$ORG_ID" ]]; then
  BODY="{\"org_id\":\"${ORG_ID}\",\"clear\":${CLEAR}}"
else
  BODY="{\"clear\":${CLEAR}}"
fi

curl -sS -f -XPOST "${ADAPTER_URL}/admin/rebuild" \
  -H 'content-type: application/json' \
  -d "$BODY"
echo
