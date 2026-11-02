"""BigQuery MERGE loader and shadow writer.

MERGE strategy:
  1. Load records to a short-lived temp table via load_table_from_json
     (auto-detect schema, WRITE_TRUNCATE).
  2. Run a MERGE DML statement — temp table as source, target as destination.
  3. DELETE the temp table in a finally block.

Shadow writes use WRITE_APPEND to `<table>_dm_shadow` — no MERGE needed,
callers run the 7-day shadow_diff before flipping the live flag.

Normalizer: when `normalizer_name` is supplied, each record is reshaped to
the bronze schema via REGISTRY[normalizer_name] before any BQ operation.
Orchestrator fields (tenant_id, _source, _synced_at) injected by the Rust
enrich() step are preserved across the reshape.
"""

from __future__ import annotations

import hashlib
import json
import logging
import uuid
from collections.abc import Iterable
from dataclasses import dataclass
from functools import lru_cache
from typing import Any

from google.cloud import bigquery
from google.cloud.bigquery import SchemaField

_LOG = logging.getLogger(__name__)


class NormalizerOutputError(ValueError):
    """Raised when a normalizer's output is structurally invalid.

    Distinct from generic ValueError so callers (orchestrator) can
    quarantine or skip the batch instead of crash-looping. Carries the
    normalizer name, record index, and per-record reason for triage.
    """

    def __init__(
        self,
        *,
        normalizer_name: str,
        record_index: int,
        reason: str,
        sample_keys: list[str] | None = None,
    ) -> None:
        self.normalizer_name = normalizer_name
        self.record_index = record_index
        self.reason = reason
        self.sample_keys = sample_keys or []
        super().__init__(
            f"{normalizer_name}: record[{record_index}]: {reason}"
            + (f" (keys={sample_keys[:8]}...)" if sample_keys else "")
        )


CHUNK_SIZE = 500
COMPOSITE_PK_SEPARATOR = "\x1f"
_TENANT_ID = SchemaField("tenant_id", "STRING", mode="NULLABLE")
_RAW_STAGE_TABLE = "__vsv2_raw_stage"
_QUARANTINE_TABLE = "__vsv2_quarantine"

# Per-endpoint chunk size overrides. Calibrated against observed Visma v1
# volumes and BQ load-job sweet spots:
#   * Low-volume reference tables (currencies, ledgers, branches, ~10-100
#     records/sync) keep CHUNK_SIZE — overhead of more granular chunking
#     outweighs the load-job cost amortisation.
#   * High-volume transactional endpoints (general_ledger_transactions,
#     sales_orders, customer_invoices, customer_documents) — chunks of
#     2000 reduce the number of round-trips by 4x while staying well
#     under the 10MB JSON load limit per chunk (Visma records are ~1-3KB).
#   * Wide-payload endpoints (sales_orders_v3 with line-level JSON arrays)
#     use 1000 to balance round-trips against per-chunk payload size.
# Default is CHUNK_SIZE for any endpoint not listed here.
ENDPOINT_CHUNK_SIZE_OVERRIDES: dict[str, int] = {
    "general_ledger_transactions": 2000,
    "customer_invoices": 2000,
    "customer_documents": 2000,
    "customer_payments": 2000,
    "supplier_invoices": 2000,
    "supplier_payments": 2000,
    "cash_transactions": 2000,
    "journal_transactions": 2000,
    "sales_orders": 1000,
    "sales_orders_v3": 1000,
    "general_ledger_balances": 2000,
    "general_ledger_balances_v2": 2000,
}


def chunk_size_for(endpoint_name: str | None) -> int:
    """Return the optimal chunk size for the given endpoint.

    Falls back to ``CHUNK_SIZE`` for unknown endpoints. Used by the
    orchestrator's chunked-load loop so high-volume endpoints amortise the
    BQ load-job cost across more rows per chunk.
    """
    if endpoint_name is None:
        return CHUNK_SIZE
    return ENDPOINT_CHUNK_SIZE_OVERRIDES.get(endpoint_name, CHUNK_SIZE)


# Metadata injected by Rust's enrich() step. Preserved through normalizer
# reshaping so BQ always has tenant-scoped keys and accurate lineage regardless
# of normalizer output.
_ORCHESTRATOR_FIELDS = frozenset({"tenant_id", "_source", "_synced_at"})


def _apply_normalizer(
    records: list[dict[str, Any]],
    normalizer_name: str,
) -> list[dict[str, Any]]:
    """Apply the named normalizer to every record.

    Validates each output is a dict and reports structural problems via
    ``NormalizerOutputError`` with the offending record index so the
    orchestrator can quarantine the batch instead of crash-looping.
    """
    from vsv2_tooling.normalizers import REGISTRY  # local import avoids circular dep

    fn = REGISTRY.get(normalizer_name)
    if fn is None:
        raise ValueError(f"Unknown normalizer: {normalizer_name!r}. Available: {sorted(REGISTRY)}")
    result = []
    for idx, r in enumerate(records):
        try:
            normalized = fn(r)
        except Exception as exc:  # normalizer raised — wrap with context
            raise NormalizerOutputError(
                normalizer_name=normalizer_name,
                record_index=idx,
                reason=f"normalizer raised {type(exc).__name__}: {exc}",
                sample_keys=sorted(r.keys()) if isinstance(r, dict) else [],
            ) from exc
        if not isinstance(normalized, dict):
            raise NormalizerOutputError(
                normalizer_name=normalizer_name,
                record_index=idx,
                reason=(f"returned {type(normalized).__name__}, expected dict"),
                sample_keys=sorted(r.keys()) if isinstance(r, dict) else [],
            )
        # Rust-injected metadata takes precedence over normalizer lineage.
        lineage = {key: r[key] for key in _ORCHESTRATOR_FIELDS if key in r}
        tombstone = (
            {"isDeleted": r["deletedDatabaseRecord"]}
            if r.get("_source") == "datamart_graphql"
            and isinstance(r.get("deletedDatabaseRecord"), bool)
            else {}
        )
        result.append({**normalized, **lineage, **tombstone})
    return result


def chunked(
    iterable: Iterable[dict[str, Any]], size: int = CHUNK_SIZE
) -> Iterable[list[dict[str, Any]]]:
    """Yield successive chunks of `size` records."""
    chunk: list[dict[str, Any]] = []
    for item in iterable:
        chunk.append(item)
        if len(chunk) >= size:
            yield chunk
            chunk = []
    if chunk:
        yield chunk


def _ensure_tenant_id(client: bigquery.Client, table_ref: str) -> None:
    table_meta = client.get_table(table_ref)
    if any(field.name == "tenant_id" for field in table_meta.schema):
        return
    table_meta.schema = [*table_meta.schema, _TENANT_ID]
    client.update_table(table_meta, ["schema"])


def _canonical_json(record: dict[str, Any]) -> str:
    return json.dumps(record, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


@lru_cache(maxsize=256)
def _split_path(path: str) -> tuple[str, ...]:
    """Cache dotted-path splits. Called once per distinct PK path; hot loop
    of dedup/merge then just walks the precomputed tuple. The handful of
    distinct PK paths in the system fits trivially in the LRU."""
    return tuple(path.split("."))


def _get_path(record: dict[str, Any], path: str) -> Any:
    current: Any = record
    for part in _split_path(path):
        if isinstance(current, str):
            try:
                current = json.loads(current)
            except json.JSONDecodeError as exc:
                raise ValueError(f"primary key field missing from record: {path}") from exc
        if not isinstance(current, dict) or part not in current:
            raise ValueError(f"primary key field missing from record: {path}")
        current = current[part]
    return current


def _stringify_pk_value(value: Any) -> str:
    if value is None:
        raise ValueError("primary key value cannot be null")
    if isinstance(value, str):
        if not value.strip():
            raise ValueError("primary key value cannot be empty")
        return value
    if isinstance(value, (int, float, bool)):
        return str(value)
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _record_pk(record: dict[str, Any], primary_keys: list[str]) -> str:
    return COMPOSITE_PK_SEPARATOR.join(
        _stringify_pk_value(_get_path(record, key)) for key in primary_keys
    )


def _write_raw_stage(
    *,
    client: bigquery.Client,
    project: str,
    dataset: str,
    table: str,
    primary_keys: list[str],
    records: list[dict[str, Any]],
    run_id: str,
    endpoint: str,
    source: str,
) -> None:
    stage_id = f"{project}.{dataset}.{_RAW_STAGE_TABLE}"
    stage_records = []
    for record in records:
        raw_payload_json = _canonical_json(record)
        stage_records.append(
            {
                "sync_run_id": run_id,
                "tenant_id": record.get("tenant_id"),
                "endpoint": endpoint,
                "source": record.get("_source") or source,
                "target_table": table,
                "partition_key": record.get("_partition_key") or "default",
                "page_index": record.get("_page_index"),
                "primary_key": _record_pk(record, primary_keys),
                "payload_hash": hashlib.sha256(raw_payload_json.encode("utf-8")).hexdigest(),
                "raw_payload_json": raw_payload_json,
                "fetched_at": record.get("_synced_at"),
            }
        )

    job_config = bigquery.LoadJobConfig(
        write_disposition="WRITE_APPEND",
        source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        autodetect=True,
    )
    load_job = client.load_table_from_json(stage_records, stage_id, job_config=job_config)
    load_job.result()


def _dedupe_records_by_pk(
    records: list[dict[str, Any]],
    primary_keys: list[str],
) -> tuple[list[dict[str, Any]], int]:
    out: list[dict[str, Any]] = []
    seen_payload_by_key: dict[str, str] = {}
    duplicates_dropped = 0

    for record in records:
        key = _record_pk(record, primary_keys)
        payload = _canonical_json(record)
        prior = seen_payload_by_key.get(key)
        if prior == payload:
            duplicates_dropped += 1
            continue
        if prior is not None:
            raise ValueError(f"duplicate primary key with different payload: {key!r}")
        seen_payload_by_key[key] = payload
        out.append(record)

    return out, duplicates_dropped


def merge(
    *,
    project: str,
    dataset: str,
    table: str,
    primary_key: str,
    records: list[dict[str, Any]],
    normalizer_name: str | None = None,
    run_id: str | None = None,
    endpoint: str | None = None,
    source: str | None = None,
    client: bigquery.Client | None = None,
) -> int:
    """MERGE records into `project.dataset.table`.

    Returns the number of rows affected (inserted + updated).
    Raises `google.api_core.exceptions.GoogleAPIError` on BQ errors.
    """
    if not records:
        return 0

    if normalizer_name:
        records = _apply_normalizer(records, normalizer_name)

    if client is None:
        client = bigquery.Client(project=project)

    primary_keys = _parse_primary_key(primary_key)
    records, _duplicates_dropped = _dedupe_records_by_pk(records, primary_keys)
    temp_id = f"{project}.{dataset}.{table}_tmp_{uuid.uuid4().hex[:8]}"
    target_id = f"{project}.{dataset}.{table}"
    _ensure_tenant_id(client, target_id)

    if run_id and endpoint and source:
        _write_raw_stage(
            client=client,
            project=project,
            dataset=dataset,
            table=table,
            primary_keys=primary_keys,
            records=records,
            run_id=run_id,
            endpoint=endpoint,
            source=source,
        )

    job_config = bigquery.LoadJobConfig(
        write_disposition="WRITE_TRUNCATE",
        source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        autodetect=True,
    )

    try:
        load_job = client.load_table_from_json(records, temp_id, job_config=job_config)
        load_job.result()

        temp_meta = client.get_table(temp_id)
        columns = [f.name for f in temp_meta.schema]

        pk_top_level = {pk.split(".", maxsplit=1)[0] for pk in primary_keys}
        missing_pk = sorted(pk for pk in pk_top_level if pk not in columns)
        if missing_pk:
            raise ValueError(
                "primary key field(s) missing from loaded records: " + ", ".join(missing_pk)
            )
        on_clause, set_clause, insert_cols, insert_vals = _build_merge_clauses(
            tuple(columns), tuple(primary_keys)
        )

        merge_sql = f"""
            MERGE `{target_id}` AS T
            USING `{temp_id}` AS S
            ON {on_clause}
            WHEN MATCHED THEN
                UPDATE SET {set_clause}
            WHEN NOT MATCHED THEN
                INSERT ({insert_cols}) VALUES ({insert_vals})
        """

        merge_job = client.query(merge_sql)
        merge_job.result()
        return merge_job.num_dml_affected_rows or len(records)

    finally:
        client.delete_table(temp_id, not_found_ok=True)


def _parse_primary_key(primary_key: str) -> list[str]:
    keys = [part.strip() for part in primary_key.split(",") if part.strip()]
    if not keys:
        raise ValueError("primary_key must contain at least one field")
    return keys


def _field_expr(alias: str, path: str) -> str:
    return f"{alias}." + ".".join(f"`{part}`" for part in _split_path(path))


@lru_cache(maxsize=256)
def _build_merge_clauses(
    columns: tuple[str, ...], primary_keys: tuple[str, ...]
) -> tuple[str, str, str, str]:
    """Cache MERGE clause fragments by (columns, primary_keys) shape.

    Each hourly sync runs the same MERGE SQL for the same endpoint with the
    same schema — caching the joins, set_clause, insert_cols, insert_vals
    strings means repeated calls reuse the precomputed work. Returns a
    4-tuple: (on_clause, set_clause, insert_cols, insert_vals).
    """
    pk_top_level = {pk.split(".", maxsplit=1)[0] for pk in primary_keys}
    set_clause = ", ".join(f"T.`{c}` = S.`{c}`" for c in columns if c not in pk_top_level)
    insert_cols = ", ".join(f"`{c}`" for c in columns)
    insert_vals = ", ".join(f"S.`{c}`" for c in columns)
    on_clause = " AND ".join(
        f"{_field_expr('T', pk)} IS NOT DISTINCT FROM {_field_expr('S', pk)}" for pk in primary_keys
    )
    return on_clause, set_clause, insert_cols, insert_vals


def write_shadow(
    *,
    project: str,
    dataset_shadow: str,
    table: str,
    records: list[dict[str, Any]],
    normalizer_name: str | None = None,
    client: bigquery.Client | None = None,
) -> int:
    """Append records to `project.dataset_shadow.<table>_dm_shadow`.

    Intended for the pilot shadow-mode window before live cutover.
    Returns the number of rows written.
    """
    if not records:
        return 0

    if normalizer_name:
        records = _apply_normalizer(records, normalizer_name)

    if client is None:
        client = bigquery.Client(project=project)

    shadow_id = f"{project}.{dataset_shadow}.{table}_dm_shadow"
    job_config = bigquery.LoadJobConfig(
        write_disposition="WRITE_APPEND",
        source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        autodetect=True,
    )

    load_job = client.load_table_from_json(records, shadow_id, job_config=job_config)
    load_job.result()
    return len(records)


def write_quarantine(
    *,
    project: str,
    dataset: str,
    records: list[dict[str, Any]],
    client: bigquery.Client | None = None,
) -> int:
    """Append rejected records to `project.dataset.__vsv2_quarantine`."""
    if not records:
        return 0

    if client is None:
        client = bigquery.Client(project=project)

    quarantine_id = f"{project}.{dataset}.{_QUARANTINE_TABLE}"
    job_config = bigquery.LoadJobConfig(
        write_disposition="WRITE_APPEND",
        source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        autodetect=True,
    )

    load_job = client.load_table_from_json(records, quarantine_id, job_config=job_config)
    load_job.result()
    return len(records)


@dataclass(frozen=True)
class MergeResult:
    """Result of a :func:`merge_with_shadow` call.

    ``merged`` is the rowcount returned by the live MERGE. ``shadowed`` is the
    rowcount written to the shadow table (zero when shadow mode is off).
    """

    merged: int
    shadowed: int


def merge_with_shadow(
    *,
    project: str,
    dataset: str,
    table: str,
    primary_key: str,
    records: list[dict[str, Any]],
    normalizer_name: str | None = None,
    run_id: str | None = None,
    endpoint: str | None = None,
    source: str | None = None,
    client: bigquery.Client | None = None,
    shadow_mode: str = "off",
    dataset_shadow: str | None = None,
) -> MergeResult:
    """MERGE records into the live target and optionally a shadow target.

    ``shadow_mode`` is a feature flag controlled by the orchestrator. The
    three legal values map onto the pilot-cutover state machine for a given
    endpoint x tenant:

    * ``"off"`` — only the live MERGE runs. Default. Pre-pilot tenants and
      endpoints that have already cut over use this.
    * ``"dual"`` — both live MERGE and shadow append run. Used during the
      pilot window so ``shadow_diff`` can compare bronze vs shadow over 7
      days before the cutover.
    * ``"shadow_only"`` — only the shadow append runs. Used when an endpoint
      is being validated in isolation without affecting bronze (e.g. a new
      normalizer under review).

    ``dataset_shadow`` defaults to ``f"{dataset}_dm_shadow"`` when not given.

    Raises:
        ValueError: if ``shadow_mode`` is unrecognised, or if a shadow
            destination is required but cannot be inferred.
    """
    if shadow_mode not in ("off", "dual", "shadow_only"):
        raise ValueError(f"shadow_mode must be one of off/dual/shadow_only, got {shadow_mode!r}")

    if not records:
        return MergeResult(merged=0, shadowed=0)

    if client is None:
        client = bigquery.Client(project=project)

    merged = 0
    shadowed = 0

    if shadow_mode in ("off", "dual"):
        merged = merge(
            project=project,
            dataset=dataset,
            table=table,
            primary_key=primary_key,
            records=records,
            normalizer_name=normalizer_name,
            run_id=run_id,
            endpoint=endpoint,
            source=source,
            client=client,
        )

    if shadow_mode in ("dual", "shadow_only"):
        shadow_dataset = dataset_shadow or f"{dataset}_dm_shadow"
        shadowed = write_shadow(
            project=project,
            dataset_shadow=shadow_dataset,
            table=table,
            records=records,
            normalizer_name=normalizer_name,
            client=client,
        )

    _LOG.info(
        "merge_with_shadow",
        extra={
            "endpoint": endpoint,
            "table": table,
            "shadow_mode": shadow_mode,
            "merged": merged,
            "shadowed": shadowed,
            "record_count": len(records),
        },
    )
    return MergeResult(merged=merged, shadowed=shadowed)
