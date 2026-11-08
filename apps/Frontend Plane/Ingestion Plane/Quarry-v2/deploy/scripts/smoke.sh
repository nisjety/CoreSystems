#!/usr/bin/env bash
set -euo pipefail

EDGE="${EDGE:-http://localhost:8080}"
CTRL="${CTRL:-http://localhost:8081}"

echo "== health =="
curl -fsS "$EDGE/health" && echo
curl -fsS "$CTRL/health" && echo

echo "== scrape =="
curl -fsS -X POST "$EDGE/v1/scrape" \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com"}' | jq '.data | {status, fingerprint, "md_bytes": .formats.markdown.bytes, link_count: (.formats.links|length)}'

echo "== list jobs =="
curl -fsS "$CTRL/v1/jobs" | jq '.data | length'
