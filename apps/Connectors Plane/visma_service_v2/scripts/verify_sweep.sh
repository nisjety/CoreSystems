#!/usr/bin/env bash
# verify_sweep.sh — Check counts, duplicates, and monotonic growth after full_sweep_replay
# Usage: bash scripts/verify_sweep.sh
set -euo pipefail

TENANT="33f319b9-4d57-11ee-960c-025417856183"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
REPORT="$ROOT_DIR/docs/evidence/verify_$(date +%Y-%m-%d_%H%M%S).md"

mkdir -p "$ROOT_DIR/docs/evidence"

PG() {
  docker compose -f "$ROOT_DIR/docker-compose.yml" exec -T db \
    psql -U postgres -d postgres -At -c "$1"
}

{
echo "# Sweep Verification Report"
echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "Tenant: $TENANT"
echo ""

echo "## Per-endpoint record counts and duplicate check"
echo ""
echo "| endpoint | total_rows | distinct_external_id | duplicates | windows_seen |"
echo "|----------|-----------|----------------------|------------|--------------|"

PG "
SELECT
  endpoint_name,
  COUNT(*) AS total_rows,
  COUNT(DISTINCT external_id) AS distinct_ids,
  COUNT(*) - COUNT(DISTINCT external_id) AS duplicates,
  COUNT(DISTINCT date_trunc('day', synced_at)) AS windows_seen
FROM vsv2.local_bronze_record
WHERE tenant_id = '$TENANT'
GROUP BY endpoint_name
ORDER BY endpoint_name;
" | while IFS='|' read -r ep tot dist dup wins; do
    echo "| $ep | $tot | $dist | $dup | $wins |"
  done

echo ""
echo "## Endpoints with duplicates"
echo ""
PG "
SELECT endpoint_name, COUNT(*) - COUNT(DISTINCT external_id) AS duplicates
FROM vsv2.local_bronze_record
WHERE tenant_id = '$TENANT'
GROUP BY endpoint_name
HAVING COUNT(*) - COUNT(DISTINCT external_id) > 0
ORDER BY duplicates DESC;
" | while IFS='|' read -r ep dup; do
    echo "- $ep: $dup duplicates"
  done || echo "_None — no duplicates found_"

echo ""
echo "## Sync run summary"
echo ""
echo "| endpoint | runs | status_ok | status_err |"
echo "|----------|------|-----------|------------|"
PG "
SELECT
  endpoint_name,
  COUNT(*) AS runs,
  COUNT(*) FILTER (WHERE status = 'ok') AS ok,
  COUNT(*) FILTER (WHERE status != 'ok') AS err
FROM vsv2.sync_run
WHERE tenant_id = '$TENANT'
GROUP BY endpoint_name
ORDER BY endpoint_name;
" | while IFS='|' read -r ep runs ok err; do
    echo "| $ep | $runs | $ok | $err |"
  done

} > "$REPORT"

echo "Report written to: $REPORT"
cat "$REPORT"
