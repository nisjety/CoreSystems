#!/usr/bin/env bash
set -euo pipefail

tenant='33f319b9-4d57-11ee-960c-025417856183'
endpoint='organizations'
report='docs/evidence/five-window-replay-2026-05-03.md'
script_start_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
windows=("2026-04-27" "2026-04-28" "2026-04-29" "2026-04-30" "2026-05-01")

mkdir -p docs/evidence

{
  echo "# Five-Window Replay Evidence"
  echo
  echo "- Executed at (UTC): ${script_start_utc}"
  echo "- Tenant: ${tenant}"
  echo "- Endpoint: ${endpoint}"
  echo
  echo "## Window Runs"
  echo
  echo "| Window (UTC) | Status | Records In | Records Written | Started At | Finished At |"
  echo "|---|---:|---:|---:|---|---|"
} > "$report"

for day in "${windows[@]}"; do
  from="${day}T00:00:00Z"
  to="${day}T23:59:59Z"
  echo "Running window ${from} -> ${to}"

  docker compose run --rm vsv2 sync --tenant "$tenant" --endpoint "$endpoint" --date-from "$from" --date-to "$to" >/tmp/vsv2_window_run.log 2>&1

  row="$(docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -P pager=off -At -F '|' -c "SELECT status,records_in,records_written,started_at,finished_at FROM vsv2.sync_run WHERE tenant_id='${tenant}' AND endpoint='${endpoint}' ORDER BY started_at DESC LIMIT 1;")"

  status="$(echo "$row" | cut -d'|' -f1)"
  rin="$(echo "$row" | cut -d'|' -f2)"
  rw="$(echo "$row" | cut -d'|' -f3)"
  started="$(echo "$row" | cut -d'|' -f4)"
  finished="$(echo "$row" | cut -d'|' -f5)"

  echo "| ${day} | ${status} | ${rin} | ${rw} | ${started} | ${finished} |" >> "$report"

  if [[ "$status" != "succeeded" ]]; then
    echo "Window ${day} failed; inspect /tmp/vsv2_window_run.log"
    exit 1
  fi
done

{
  echo
  echo "## Post-Run Checkpoint"
  echo
  echo "| Tenant | Endpoint | Source | Last Modified Watermark | Records Seen | Updated At |"
  echo "|---|---|---|---|---:|---|"
} >> "$report"

docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -P pager=off -At -F '|' -c "SELECT tenant_id,endpoint,source,last_modified_watermark,records_seen,updated_at FROM vsv2.sync_checkpoint WHERE tenant_id='${tenant}' AND endpoint='${endpoint}' ORDER BY updated_at DESC LIMIT 1;" | awk -F'|' '{printf("| %s | %s | %s | %s | %s | %s |\n",$1,$2,$3,$4,$5,$6)}' >> "$report"

{
  echo
  echo "## Out-of-Window Validation"
  echo
  echo "| Check | Value |"
  echo "|---|---:|"
} >> "$report"

oow="$(docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -P pager=off -At -c "SELECT COUNT(*) FROM vsv2.local_bronze_record WHERE tenant_id='${tenant}' AND endpoint='${endpoint}' AND (payload->>'lastModifiedDateTime')::timestamptz > '2026-05-01T23:59:59Z'::timestamptz;")"

echo "| Rows with lastModifiedDateTime > 2026-05-01T23:59:59Z | ${oow} |" >> "$report"

{
  echo
  echo "## Script-Scoped Run History"
  echo
  echo "| Started At | Status | Records In | Records Written | Finished At |"
  echo "|---|---:|---:|---:|---|"
} >> "$report"

docker compose exec -T postgres psql -U visma_v2 -d visma_v2 -P pager=off -At -F '|' -c "SELECT started_at,status,records_in,records_written,finished_at FROM vsv2.sync_run WHERE tenant_id='${tenant}' AND endpoint='${endpoint}' AND started_at >= '${script_start_utc}'::timestamptz ORDER BY started_at;" | awk -F'|' '{printf("| %s | %s | %s | %s | %s |\n",$1,$2,$3,$4,$5)}' >> "$report"

echo "Wrote ${report}"
