#!/usr/bin/env bash
set -euo pipefail

tenant_id="${TENANT_ID:-33f319b9-4d57-11ee-960c-025417856183}"
bearer_token="${VSV2_HTTP_BEARER_TOKEN:-local-dev-token}"
base_url="${VSV2_HTTP_BASE_URL:-http://127.0.0.1:8080}"
default_window_days="2026-04-27,2026-04-28,2026-04-29,2026-04-30,2026-05-01"
window_days_csv="${WINDOW_DAYS_CSV:-${default_window_days}}"
poll_timeout_secs="${POLL_TIMEOUT_SECS:-7200}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required" >&2
  exit 1
fi

# Ensure HTTP service is up with a non-empty bearer token.
VSV2_HTTP_BEARER_TOKEN="${bearer_token}" docker compose up -d vsv2-http >/dev/null

health_code="$(curl -s -o /dev/null -w '%{http_code}' "${base_url}/v1/health")"
if [[ "${health_code}" != "200" ]]; then
  echo "vsv2-http health check failed: HTTP ${health_code}" >&2
  exit 1
fi

TENANT_ID="${tenant_id}" \
VSV2_HTTP_BEARER_TOKEN="${bearer_token}" \
VSV2_HTTP_BASE_URL="${base_url}" \
WINDOW_DAYS_CSV="${window_days_csv}" \
POLL_TIMEOUT_SECS="${poll_timeout_secs}" \
python3 - <<'PY'
import datetime as dt
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request


tenant_id = os.environ["TENANT_ID"]
bearer_token = os.environ["VSV2_HTTP_BEARER_TOKEN"]
base_url = os.environ["VSV2_HTTP_BASE_URL"].rstrip("/")
window_days = [d for d in os.environ["WINDOW_DAYS_CSV"].split(",") if d]
poll_timeout_secs = int(os.environ.get("POLL_TIMEOUT_SECS", "7200"))


def sh(command: str) -> str:
    return subprocess.check_output(command, shell=True, text=True).strip()


def find_postgres_container() -> str:
    lines = sh("docker compose ps --format '{{.Name}} {{.Service}}'").splitlines()
    for line in lines:
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "postgres":
            return parts[0]
    raise RuntimeError("postgres service container not found")


pg_container = find_postgres_container()


def psql(query: str) -> str:
    payload = query.replace('"', '\\"')
    return sh(
        f'docker exec {pg_container} psql -U visma_v2 -d visma_v2 -At -F "|" -c "{payload}"'
    )


def api_json(method: str, rel_path: str, payload=None):
    headers = {"Authorization": f"Bearer {bearer_token}"}
    body = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        body = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        f"{base_url}{rel_path}",
        data=body,
        headers=headers,
        method=method,
    )
    with urllib.request.urlopen(req, timeout=60) as response:
        raw = response.read()
        data = json.loads(raw.decode("utf-8")) if raw else {}
        return response.status, data


def clear_active_backfills() -> list[str]:
    active_rows = psql(
        "SELECT operation_id::text FROM vsv2.sync_operation "
        "WHERE kind='backfill' AND status IN ('pending','running') ORDER BY created_at"
    )
    active_ids = [row for row in active_rows.splitlines() if row.strip()]
    if not active_ids:
        return []

    id_csv = ",".join(f"'{op_id}'" for op_id in active_ids)
    psql(
        "UPDATE vsv2.sync_operation "
        "SET status='failed', finished_at=NOW(), error='cleared for manual replay' "
        f"WHERE operation_id IN ({id_csv})"
    )
    return active_ids


def parse_backfill_id_from_conflict(message: str):
    match = re.search(r"/v1/backfills/([0-9a-fA-F-]{36})", message)
    return match.group(1) if match else None


def create_backfill(day: str):
    payload = {
        "tenant": tenant_id,
        "endpoints": ["all"],
        "date_from": f"{day}T00:00:00Z",
        "date_to": f"{day}T23:59:59Z",
    }
    try:
        _, response = api_json("POST", "/v1/sync/backfill", payload)
        return response["backfill_id"]
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="ignore")
        if exc.code == 409:
            try:
                data = json.loads(raw)
            except json.JSONDecodeError:
                data = {}
            operation_id = parse_backfill_id_from_conflict(data.get("message", ""))
            if operation_id:
                return operation_id
        raise RuntimeError(f"backfill create failed for {day}: HTTP {exc.code} {raw}") from exc


def normalize_state(raw_state: str, failed_count: int) -> str:
    state = (raw_state or "").lower()
    if state in {"completed", "succeeded"}:
        return "partial_success" if failed_count > 0 else "succeeded"
    return state


def poll_backfill(backfill_id: str):
    deadline = time.time() + poll_timeout_secs
    while time.time() < deadline:
        _, detail = api_json("GET", f"/v1/backfills/{backfill_id}")
        failures = detail.get("failed_endpoints") or []
        failed_count = len(failures)
        terminal_state = normalize_state(detail.get("status"), failed_count)
        if terminal_state in {"succeeded", "partial_success", "failed"}:
            return detail, terminal_state, failed_count
        time.sleep(2)
    raise RuntimeError(f"poll timeout for backfill {backfill_id}")


cleared = clear_active_backfills()
print("CLEARED_ACTIVE_BACKFILLS", ",".join(cleared) if cleared else "none")

rows = []
first_started_at = None
for day in window_days:
    backfill_id = create_backfill(day)
    detail, terminal_state, failed_count = poll_backfill(backfill_id)
    started_at = detail.get("started_at") or detail.get("created_at")
    if first_started_at is None and started_at:
        first_started_at = started_at
    rows.append(
        {
            "day": day,
            "backfill_id": backfill_id,
            "state": terminal_state,
            "records_synced": int(detail.get("records_synced") or 0),
            "attempted_endpoints": int(detail.get("attempted_endpoints") or 0),
            "succeeded_endpoints": int(detail.get("succeeded_endpoints") or 0),
            "failed_count": failed_count,
        }
    )

if first_started_at is None:
    first_started_at = dt.datetime.utcnow().replace(microsecond=0).isoformat() + "Z"

totals_line = psql(
    "SELECT COUNT(*)::bigint, "
    "COALESCE(SUM(records_in),0)::bigint, "
    "COALESCE(SUM(records_written),0)::bigint "
    "FROM vsv2.sync_run "
    f"WHERE tenant_id='{tenant_id}' AND started_at >= '{first_started_at}'::timestamptz"
)
totals = (totals_line.split("|") + ["0", "0", "0"])[:3]

linked_line = psql(
    "SELECT COUNT(*)::bigint "
    "FROM vsv2.local_bronze_record b "
    "JOIN vsv2.sync_run r ON r.run_id=b.last_run_id "
    f"WHERE b.tenant_id='{tenant_id}' AND r.started_at >= '{first_started_at}'::timestamptz"
)
linked_rows = linked_line.splitlines()[0] if linked_line else "0"

print("REPORT_BEGIN")
for row in rows:
    print(
        "\t".join(
            [
                row["day"],
                row["backfill_id"],
                row["state"],
                str(row["records_synced"]),
                str(row["attempted_endpoints"]),
                str(row["succeeded_endpoints"]),
                str(row["failed_count"]),
            ]
        )
    )
print(
    "\t".join(
        [
            "SQL_TOTALS",
            f"first_started_at={first_started_at}",
            f"total_runs={totals[0]}",
            f"sum_records_in={totals[1]}",
            f"sum_records_written={totals[2]}",
            f"linked_bronze_rows={linked_rows}",
        ]
    )
)
print("REPORT_END")
PY