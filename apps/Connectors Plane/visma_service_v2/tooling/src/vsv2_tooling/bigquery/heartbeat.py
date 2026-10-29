"""Sync heartbeat: one row per successful endpoint write, the population gate's record of a sync.

`vsv2-bq-stage` writes an endpoint's records to its staging table. When that write succeeds, the
same process appends one row to `<dataset>._sync_heartbeat` (the staging dataset,
`aquatiq-datavarehus.dw_staging_visma` in production):

  tenant_id     the tenant the batch belongs to
  endpoint      the endpoint name, the same name as its staging table (e.g. customer_invoices)
  finished_at   when the staging write finished, UTC
  status        'succeeded': only a successful write writes a heartbeat
  rows_written  distinct records the write delivered (0 for an empty batch: still a sync)

MAX(finished_at) per tenant and endpoint is the last sync. It replaces the write-timestamp proxies
(the bronze syncedAt, the silver synced_at): those move when a row is rewritten, not when a sync
runs. A sync that writes nothing looked stale, and a silver rebuild moved the threshold.

A heartbeat never fails the sync. The records are already written when it is attempted, so a
refused or unanswered heartbeat is logged and the gate reports the tenant stale, which is visible.
It also never waits longer than HEARTBEAT_DEADLINE_S: the client calls carry that timeout, and the
write runs under a hard deadline in case a call hangs past it.

Rows are streamed (insert_rows_json), not loaded as jobs: one load job per endpoint and tenant
would exceed BigQuery's load-job quota for a single table.
"""

from __future__ import annotations

import logging
import threading
from datetime import UTC, datetime
from typing import Any

from google.cloud import bigquery
from google.cloud.bigquery import SchemaField

_LOG = logging.getLogger(__name__)

HEARTBEAT_TABLE = "_sync_heartbeat"
SUCCEEDED = "succeeded"
# The longest a heartbeat may take, client construction included. The stage command's own write
# timeout is an hour, so without this a stuck heartbeat would hold up a sync that already succeeded.
HEARTBEAT_DEADLINE_S = 30.0
HEARTBEAT_SCHEMA = [
    SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    SchemaField("endpoint", "STRING", mode="REQUIRED"),
    SchemaField("finished_at", "TIMESTAMP", mode="REQUIRED"),
    SchemaField("status", "STRING", mode="REQUIRED"),
    SchemaField("rows_written", "INT64", mode="REQUIRED"),
]


class HeartbeatError(RuntimeError):
    """BigQuery refused the heartbeat row."""


def heartbeat_table_id(project: str, dataset: str) -> str:
    return f"{project}.{dataset}.{HEARTBEAT_TABLE}"


def heartbeat_row(
    *, tenant_id: str, endpoint: str, rows_written: int, finished_at: datetime
) -> dict[str, Any]:
    """The row as BigQuery stores it. `finished_at` must be timezone-aware."""
    return {
        "tenant_id": tenant_id,
        "endpoint": endpoint,
        "finished_at": finished_at.astimezone(UTC).isoformat(),
        "status": SUCCEEDED,
        "rows_written": int(rows_written),
    }


def write_heartbeat(
    client: bigquery.Client,
    *,
    project: str,
    dataset: str,
    tenant_id: str,
    endpoint: str,
    rows_written: int,
    finished_at: datetime | None = None,
) -> None:
    """Append one heartbeat row, creating the table on first use. Raises when BigQuery refuses."""
    table_id = heartbeat_table_id(project, dataset)
    row = heartbeat_row(
        tenant_id=tenant_id,
        endpoint=endpoint,
        rows_written=rows_written,
        finished_at=finished_at or datetime.now(UTC),
    )
    # Each call gets the deadline as its timeout, and as the retry deadline, so retries cannot run
    # past it either.
    retry = bigquery.DEFAULT_RETRY.with_deadline(HEARTBEAT_DEADLINE_S)
    client.create_table(
        bigquery.Table(table_id, schema=HEARTBEAT_SCHEMA),
        exists_ok=True,
        retry=retry,
        timeout=HEARTBEAT_DEADLINE_S,
    )
    errors = client.insert_rows_json(table_id, [row], retry=retry, timeout=HEARTBEAT_DEADLINE_S)
    if errors:
        raise HeartbeatError(f"{table_id} refused the heartbeat: {errors}")


def record_heartbeat(
    *,
    project: str,
    dataset: str,
    tenant_id: str,
    endpoint: str,
    rows_written: int,
    client: bigquery.Client | None = None,
) -> bool:
    """write_heartbeat that never raises and never waits longer than HEARTBEAT_DEADLINE_S.

    The write runs in a daemon thread, so a call that BigQuery never answers cannot hold the sync
    up: the thread is abandoned, the heartbeat is reported as not written, and the process can exit.
    A failure, client construction included, is logged.

    Returns True when the row was written. The caller is a successful sync, and it stays one.
    """
    outcome: dict[str, BaseException] = {}
    written = threading.Event()

    def attempt() -> None:
        try:
            write_heartbeat(
                client or bigquery.Client(project=project),
                project=project,
                dataset=dataset,
                tenant_id=tenant_id,
                endpoint=endpoint,
                rows_written=rows_written,
            )
        except Exception as error:  # a heartbeat must never fail the sync that just wrote
            outcome["error"] = error
        finally:
            written.set()

    worker = threading.Thread(target=attempt, name=f"heartbeat-{endpoint}", daemon=True)
    worker.start()
    if not written.wait(HEARTBEAT_DEADLINE_S):
        # The row may still land after this; the table is the truth, and the gate reads the table.
        _LOG.warning(
            "heartbeat not written for tenant %s, endpoint %s: no answer from BigQuery "
            "within %.0f s",
            tenant_id,
            endpoint,
            HEARTBEAT_DEADLINE_S,
        )
        return False
    if "error" in outcome:
        error = outcome["error"]
        _LOG.warning(
            "heartbeat not written for tenant %s, endpoint %s: %s: %s",
            tenant_id,
            endpoint,
            type(error).__name__,
            error,
        )
        return False
    return True
