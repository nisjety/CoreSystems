"""Delete reconciliation — soft-delete bronze rows missing from ERP REST.

Flow (called by the Rust orchestrator via vsv2-bq-reconcile):
  1. Rust does a full ERP REST ID scan and writes PKs as JSONL (one JSON value per line).
  2. This module queries BQ for all non-deleted PKs.
  3. Rows in BQ but absent from ERP are soft-deleted via `_deleted_at`.

Safety: if the ERP ID set is empty the reconciler refuses to proceed (prevents
a transient API failure from wiping an entire table).

`_deleted_at TIMESTAMP` is added to the target table automatically if missing.
"""

from __future__ import annotations

import json
from pathlib import Path

from google.cloud import bigquery
from google.cloud.bigquery import SchemaField

_DELETED_AT = SchemaField("_deleted_at", "TIMESTAMP", mode="NULLABLE")
_CHUNK = 5_000  # IDs per DML batch (stays well under BQ query-size limits)
_COMPOSITE_PK_SEPARATOR = "\x1f"


def _ensure_deleted_at(client: bigquery.Client, table_ref: str) -> None:
    table = client.get_table(table_ref)
    if any(f.name == "_deleted_at" for f in table.schema):
        return
    table.schema = [*table.schema, _DELETED_AT]
    client.update_table(table, ["schema"])


def _escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace("'", "\\'")


def reconcile(
    *,
    project: str,
    dataset: str,
    table: str,
    pk: str,
    erp_ids: set[str],
    tenant_id: str | None = None,
    client: bigquery.Client | None = None,
) -> int:
    """Soft-delete BQ rows whose PK is absent from `erp_ids`.

    Returns the number of rows marked deleted.
    Raises ValueError if `erp_ids` is empty (safety guard).
    """
    if not erp_ids:
        raise ValueError(
            f"reconcile: ERP returned 0 IDs for {table}. "
            "Refusing to soft-delete to avoid wiping the table."
        )

    if client is None:
        client = bigquery.Client(project=project)

    table_ref = f"{project}.{dataset}.{table}"
    _ensure_deleted_at(client, table_ref)

    pk_parts = _parse_primary_key(pk)
    pk_expr = _pk_string_expr(pk_parts)
    tenant_filter = ""
    if tenant_id:
        tenant_filter = f" AND `tenant_id` = '{_escape(tenant_id)}'"

    bq_query = f"""
        SELECT {pk_expr} AS pk_str
        FROM `{table_ref}`
        WHERE _deleted_at IS NULL
          {tenant_filter}
    """
    bq_ids = {row["pk_str"] for row in client.query(bq_query).result()}

    to_delete = bq_ids - erp_ids
    if not to_delete:
        return 0

    total = 0
    ids = list(to_delete)
    for i in range(0, len(ids), _CHUNK):
        chunk = ids[i : i + _CHUNK]
        in_clause = ", ".join(f"'{_escape(v)}'" for v in chunk)
        dml = f"""
            UPDATE `{table_ref}`
            SET _deleted_at = CURRENT_TIMESTAMP()
            WHERE {pk_expr} IN ({in_clause})
              AND _deleted_at IS NULL
              {tenant_filter}
        """
        job = client.query(dml)
        job.result()
        total += job.num_dml_affected_rows or 0

    return total


def load_erp_ids(path: Path) -> set[str]:
    """Read one JSON value per line; return as a set of strings."""
    ids: set[str] = set()
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        val = json.loads(line)
        ids.add(str(val))
    return ids


def _parse_primary_key(primary_key: str) -> list[str]:
    keys = [part.strip() for part in primary_key.split(",") if part.strip()]
    if not keys:
        raise ValueError("primary key must contain at least one field")
    return keys


def _field_expr(path: str) -> str:
    return ".".join(f"`{part}`" for part in path.split("."))


def _pk_string_expr(pk_parts: list[str]) -> str:
    casts = [f"COALESCE(CAST({_field_expr(part)} AS STRING), '')" for part in pk_parts]
    if len(casts) == 1:
        return casts[0]
    separator = f"'{_COMPOSITE_PK_SEPARATOR}'"
    args: list[str] = []
    for index, cast in enumerate(casts):
        if index:
            args.append(separator)
        args.append(cast)
    return f"CONCAT({', '.join(args)})"
