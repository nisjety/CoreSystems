"""Raw staging in BigQuery: one table per endpoint, the latest version of every record as JSON.

This is the landing layer (`dw_staging_visma`). It keeps what Visma returned, without typing it:
Dataform builds the typed bronze layer from it. That keeps 135 endpoints with nested, sparse
payloads out of BigQuery's schema autodetection, which drops fields that are empty in a batch
and then fails the MERGE when a later batch has them.

Every table has the same shape:

  tenant_id, record_key   identity; `record_key` = the key fields, tenant first, joined by \\x1f
  source                  erp_rest | datamart_graphql | expense_api
  payload                 JSON: the record as fetched, minus `_synced_at` and `_page_index`
  payload_hash            SHA-256 of the canonical payload, without the paging block (below)
  first_seen_at           when the record first arrived
  updated_at              when its payload last changed (not when it was last fetched)
  last_run_id             the sync run that wrote the row last
  deleted_at              when Visma stopped having the record (see below); NULL while it exists

A record counts as deleted in two ways. DataMart sends a tombstone (`deletedDatabaseRecord`), which
is stored as it arrives. The ERP APIs never return a deleted record, so a COMPLETE scan decides:
when the caller says the batch holds everything Visma has for the tenant and endpoint, every live
row of that tenant that is not in it gets `deleted_at`. A record that returns is cleared again. A
scan that would delete more than a fifth of the tenant's rows is refused (a partial answer from
an API looks exactly like a mass deletion), unless the caller insists.

Writing is idempotent. Records are loaded into a temporary table and MERGEd on
(tenant_id, record_key); only new rows and rows whose payload changed are written, so a full
refresh of an unchanged endpoint rewrites nothing.
"""

from __future__ import annotations

import hashlib
import logging
import os
import time
import uuid
from collections.abc import Iterable, Iterator
from datetime import UTC, datetime, timedelta
from typing import Any

from google.api_core.exceptions import BadRequest, GoogleAPICallError, NotFound
from google.cloud import bigquery
from google.cloud.bigquery import SchemaField

from vsv2_tooling.bigquery.loader import (
    _canonical_json,
    _parse_primary_key,
    _record_pk,
    chunked,
)

_LOG = logging.getLogger(__name__)

# Fields Rust adds that change on every fetch: they say nothing about the record.
VOLATILE_FIELDS = frozenset({"_synced_at", "_page_index"})

# The ERP list endpoints repeat the response's paging block in every row,
# `metadata: {"totalCount": N, "maxPageSize": M}`. It describes the query that returned the row,
# not the record: one new invoice raises `totalCount` in the row of every other invoice, and a
# filtered fetch carries a different count than a full one. It stays in the payload as fetched
# but out of the hash, so it never makes a row look changed (it did in 55 tables, 2.2 million rows).
PAGING_METADATA_KEYS = frozenset({"totalCount", "maxPageSize"})

# Rows per load job. A row is a few KB of JSON, so this keeps a job in the tens of megabytes
# while a 600,000-row endpoint still needs only about 30 load jobs and one MERGE.
LOAD_CHUNK_ROWS = 20_000

# Seconds to wait for one BigQuery job. A job whose connection died (a laptop that slept, a
# network that dropped) otherwise waits for ever and holds the whole sync with it.
LOAD_TIMEOUT_S = 900
MERGE_TIMEOUT_S = 1800

# How long a temporary load table lives if its step never gets to delete it. It is counted from the
# moment the table is created: a step can wait for hours before that (a laptop that slept, a slow
# fetch). An expiry counted from the start of the step is then already past when BigQuery sees it.
TEMP_TABLE_TTL = timedelta(hours=24)

# A complete scan may mark at most this share of a tenant's live rows deleted (and always up to
# DELETE_GUARD_FLOOR rows, so small tables can change freely).
DELETE_GUARD_FRACTION = 0.2
DELETE_GUARD_FLOOR = 50

# BigQuery serialises concurrent DML on one table and can reject one with this message.
_RETRY_MESSAGES = ("concurrent update", "could not serialize access")
_MERGE_ATTEMPTS = 5

STAGING_SCHEMA = [
    SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    SchemaField("record_key", "STRING", mode="REQUIRED"),
    SchemaField("source", "STRING"),
    SchemaField("payload", "JSON", mode="REQUIRED"),
    SchemaField("payload_hash", "STRING", mode="REQUIRED"),
    SchemaField("first_seen_at", "TIMESTAMP", mode="REQUIRED"),
    SchemaField("updated_at", "TIMESTAMP", mode="REQUIRED"),
    SchemaField("last_run_id", "STRING"),
    SchemaField("deleted_at", "TIMESTAMP"),
]

# The temporary table carries what the MERGE needs: no run id (a query parameter), no deleted_at.
_LOAD_SCHEMA = [
    SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    SchemaField("record_key", "STRING", mode="REQUIRED"),
    SchemaField("source", "STRING"),
    SchemaField("payload", "JSON", mode="REQUIRED"),
    SchemaField("payload_hash", "STRING", mode="REQUIRED"),
    SchemaField("fetched_at", "TIMESTAMP", mode="REQUIRED"),
    SchemaField("is_deleted", "BOOL", mode="REQUIRED"),
]


class StageStats:
    """Counts kept while rows are built as a stream."""

    def __init__(self) -> None:
        self.rows = 0
        self.duplicates = 0


def _without_paging(payload: dict[str, Any]) -> dict[str, Any]:
    """The payload as far as the record goes: no `metadata` that holds only paging counts.

    A `metadata` with anything else in it is data of the record and stays.
    """
    metadata = payload.get("metadata")
    if isinstance(metadata, dict) and metadata and set(metadata) <= PAGING_METADATA_KEYS:
        return {k: v for k, v in payload.items() if k != "metadata"}
    return payload


def iter_stage_rows(
    records: Iterable[dict[str, Any]],
    primary_keys: list[str],
    *,
    source: str,
    now: datetime,
    stats: StageStats,
) -> Iterator[dict[str, Any]]:
    """Yield one staging row per distinct key, holding only key and hash in memory.

    Two records with one key are fine when identical (a page boundary served a row twice) and an
    error when they differ: staging holds one version per key, so the second would silently
    replace the first.
    """
    hash_by_key: dict[str, str] = {}
    for record in records:
        tenant = record.get("tenant_id")
        if not isinstance(tenant, str) or not tenant.strip():
            raise ValueError("record has no tenant_id: staging rows are tenant-scoped")
        key = _record_pk(record, primary_keys)
        payload = {k: v for k, v in record.items() if k not in VOLATILE_FIELDS}
        digest = hashlib.sha256(
            _canonical_json(_without_paging(payload)).encode("utf-8")
        ).hexdigest()
        prior = hash_by_key.get(key)
        if prior is not None:
            if prior != digest:
                raise ValueError(f"duplicate primary key with different payload: {key!r}")
            stats.duplicates += 1
            continue
        hash_by_key[key] = digest
        stats.rows += 1
        yield {
            "tenant_id": tenant,
            "record_key": key,
            "source": record.get("_source") or source,
            "payload": payload,
            "payload_hash": digest,
            "fetched_at": record.get("_synced_at") or now.isoformat(),
            "is_deleted": bool(record.get("deletedDatabaseRecord")),
        }


def build_stage_rows(
    records: list[dict[str, Any]],
    primary_keys: list[str],
    *,
    source: str,
    now: datetime,
) -> tuple[list[dict[str, Any]], int]:
    """(rows ready to load, exact duplicates dropped): `iter_stage_rows`, collected."""
    stats = StageStats()
    rows = list(iter_stage_rows(records, primary_keys, source=source, now=now, stats=stats))
    return rows, stats.duplicates


def merge_sql(target_id: str, temp_id: str) -> str:
    """The MERGE that writes new rows, changed rows, tombstones and rows coming back from deleted.

    An unchanged live row is not touched, so a full refresh of an unchanged endpoint rewrites
    nothing.
    """
    return f"""
        MERGE `{target_id}` AS T
        USING `{temp_id}` AS S
        ON T.tenant_id = S.tenant_id AND T.record_key = S.record_key
        WHEN MATCHED AND T.payload_hash != S.payload_hash THEN
            UPDATE SET source = S.source, payload = S.payload, payload_hash = S.payload_hash,
                       updated_at = S.fetched_at, last_run_id = @run_id,
                       deleted_at = IF(S.is_deleted, S.fetched_at, NULL)
        WHEN MATCHED AND S.is_deleted AND T.deleted_at IS NULL THEN
            UPDATE SET deleted_at = S.fetched_at, last_run_id = @run_id
        WHEN MATCHED AND NOT S.is_deleted AND T.deleted_at IS NOT NULL THEN
            UPDATE SET deleted_at = NULL, updated_at = S.fetched_at, last_run_id = @run_id
        WHEN NOT MATCHED THEN
            INSERT (tenant_id, record_key, source, payload, payload_hash,
                    first_seen_at, updated_at, last_run_id, deleted_at)
            VALUES (S.tenant_id, S.record_key, S.source, S.payload, S.payload_hash,
                    S.fetched_at, S.fetched_at, @run_id, IF(S.is_deleted, S.fetched_at, NULL))
    """


def missing_count_sql(target_id: str, temp_id: str) -> str:
    """Live rows of the tenant, and how many of them the scan did not contain."""
    return f"""
        SELECT COUNT(*) AS live, COUNTIF(S.record_key IS NULL) AS missing
        FROM `{target_id}` AS T
        LEFT JOIN (SELECT DISTINCT record_key FROM `{temp_id}`) AS S USING (record_key)
        WHERE T.tenant_id = @tenant AND T.deleted_at IS NULL
    """


def mark_missing_sql(target_id: str, temp_id: str) -> str:
    return f"""
        UPDATE `{target_id}` AS T
        SET deleted_at = CURRENT_TIMESTAMP(), last_run_id = @run_id
        WHERE T.tenant_id = @tenant AND T.deleted_at IS NULL
          AND NOT EXISTS (SELECT 1 FROM `{temp_id}` AS S WHERE S.record_key = T.record_key)
    """


def deletion_allowed(live: int, missing: int, *, force: bool = False) -> bool:
    """Whether a complete scan may mark `missing` of `live` rows deleted."""
    if force or missing == 0:
        return True
    return missing <= max(DELETE_GUARD_FLOOR, int(live * DELETE_GUARD_FRACTION))


# Where a dataset is created when it does not exist yet. An existing dataset keeps its own location.
DEFAULT_LOCATION = "europe-north2"


def default_location() -> str:
    return os.environ.get("VSV2_BIGQUERY_LOCATION") or DEFAULT_LOCATION


def ensure_dataset(client: bigquery.Client, project: str, dataset: str, location: str) -> None:
    """Create the staging dataset when it does not exist, so a new project needs no manual setup.

    Needs `bigquery.datasets.create` the first time only; with a service account that may not
    create datasets, create the dataset once by hand and this finds it.
    """
    try:
        client.get_dataset(f"{project}.{dataset}")
    except NotFound:
        created = bigquery.Dataset(f"{project}.{dataset}")
        created.location = location
        created.description = "Raw staging of Visma v2: one JSON table per endpoint (vsv2)."
        client.create_dataset(created, exists_ok=True)
        _LOG.info("created dataset %s.%s in %s", project, dataset, location)


def ensure_table(
    client: bigquery.Client, table_id: str, *, endpoint: str, primary_key: str
) -> None:
    """Create the endpoint's staging table when it does not exist yet."""
    table = bigquery.Table(table_id, schema=STAGING_SCHEMA)
    table.clustering_fields = ["tenant_id", "record_key"]
    table.description = (
        f"Raw staging of Visma v2 endpoint '{endpoint}'. Latest version of each record as JSON. "
        f"Key: {primary_key} (joined by \\x1f). Written by vsv2 (vsv2-bq-stage)."
    )
    client.create_table(table, exists_ok=True)


def _is_retryable(error: GoogleAPICallError) -> bool:
    text = str(error).lower()
    return isinstance(error, BadRequest) and any(m in text for m in _RETRY_MESSAGES)


def _run_dml(client: bigquery.Client, sql: str, params: list[bigquery.ScalarQueryParameter]) -> int:
    config = bigquery.QueryJobConfig(query_parameters=params)
    for attempt in range(1, _MERGE_ATTEMPTS + 1):
        try:
            job = client.query(sql, job_config=config)
            job.result(timeout=MERGE_TIMEOUT_S)
            return int(job.num_dml_affected_rows or 0)
        except GoogleAPICallError as error:
            if attempt == _MERGE_ATTEMPTS or not _is_retryable(error):
                raise
            delay = 2.0 * attempt
            _LOG.warning("DML hit a concurrent update, retry %d in %.0fs", attempt, delay)
            time.sleep(delay)
    raise AssertionError("unreachable")


def _run_merge(client: bigquery.Client, sql: str, run_id: str) -> int:
    return _run_dml(client, sql, [bigquery.ScalarQueryParameter("run_id", "STRING", run_id)])


def mark_missing_deleted(
    client: bigquery.Client,
    *,
    target_id: str,
    temp_id: str,
    tenant: str,
    run_id: str,
    force: bool = False,
) -> dict[str, int]:
    """Set `deleted_at` on the tenant's live rows that a complete scan did not contain."""
    params = [
        bigquery.ScalarQueryParameter("tenant", "STRING", tenant),
        bigquery.ScalarQueryParameter("run_id", "STRING", run_id),
    ]
    count = next(
        iter(
            client.query(
                missing_count_sql(target_id, temp_id),
                job_config=bigquery.QueryJobConfig(query_parameters=params[:1]),
            ).result(timeout=MERGE_TIMEOUT_S)
        )
    )
    live, missing = int(count.live), int(count.missing)
    if not deletion_allowed(live, missing, force=force):
        _LOG.warning(
            "%s: a complete scan lacks %d of %d live rows of %s; refusing to mark them deleted",
            target_id,
            missing,
            live,
            tenant,
        )
        return {"deleted": 0, "delete_skipped": missing}
    if missing == 0:
        return {"deleted": 0, "delete_skipped": 0}
    deleted = _run_dml(client, mark_missing_sql(target_id, temp_id), params)
    return {"deleted": deleted, "delete_skipped": 0}


def _create_temp(client: bigquery.Client, temp: bigquery.Table) -> None:
    """Create a step's temporary table, its expiry counted from this moment.

    The expiry is set here and not where the step starts: the first records of a step can arrive
    hours later, and an expiry counted from the start is then already past when BigQuery sees it,
    so BigQuery refuses the table.
    """
    temp.expires = datetime.now(UTC) + TEMP_TABLE_TTL
    client.create_table(temp)


def stage(
    *,
    project: str,
    dataset: str,
    table: str,
    primary_key: str,
    records: Iterable[dict[str, Any]],
    run_id: str,
    endpoint: str,
    source: str,
    tenant: str | None = None,
    complete: bool = False,
    force_delete: bool = False,
    location: str | None = None,
    client: bigquery.Client | None = None,
) -> dict[str, int]:
    """Write `records` to `project.dataset.table`.

    Returns {"rows": delivered, "changed": written, "deleted": marked deleted,
    "delete_skipped": rows a refused deletion would have marked}.

    `records` may be a stream: rows are built and loaded a chunk at a time, so a 600,000-record
    endpoint never sits in memory as Python objects. `rows` counts the distinct records
    delivered, `changed` the rows the MERGE inserted or updated. An empty batch still creates the
    table, so an endpoint with no data has one too.

    `complete` says the batch is everything Visma has for `tenant` and this endpoint: the tenant's
    live rows that are not in it are marked deleted. That needs `tenant`, also when the batch is
    empty (everything is gone).
    """
    if complete and not tenant:
        raise ValueError("a complete scan needs the tenant it is complete for")
    client = client or bigquery.Client(project=project)
    primary_keys = _parse_primary_key(primary_key)
    target_id = f"{project}.{dataset}.{table}"
    ensure_dataset(client, project, dataset, location or default_location())
    ensure_table(client, target_id, endpoint=endpoint, primary_key=primary_key)

    now = datetime.now(UTC)
    stats = StageStats()
    rows = iter_stage_rows(records, primary_keys, source=source, now=now, stats=stats)
    temp_id = f"{project}.{dataset}.__stage_{table}_{uuid.uuid4().hex[:8]}"
    temp = bigquery.Table(temp_id, schema=_LOAD_SCHEMA)
    created = False
    result = {"rows": 0, "changed": 0, "deleted": 0, "delete_skipped": 0}
    try:
        for chunk in chunked(rows, LOAD_CHUNK_ROWS):
            if not created:
                _create_temp(client, temp)
                created = True
            config = bigquery.LoadJobConfig(
                write_disposition="WRITE_APPEND",
                source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
                schema=_LOAD_SCHEMA,
            )
            client.load_table_from_json(chunk, temp_id, job_config=config).result(
                timeout=LOAD_TIMEOUT_S
            )
        if not created and not complete:
            return result
        if not created:  # a complete scan that found nothing: an empty set of keys
            _create_temp(client, temp)
            created = True
        if stats.rows:
            result["changed"] = _run_merge(client, merge_sql(target_id, temp_id), run_id)
        result["rows"] = stats.rows
        if complete:
            result.update(
                mark_missing_deleted(
                    client,
                    target_id=target_id,
                    temp_id=temp_id,
                    tenant=tenant or "",
                    run_id=run_id,
                    force=force_delete,
                )
            )
    finally:
        if created:
            client.delete_table(temp_id, not_found_ok=True)
    if stats.duplicates:
        _LOG.info("%s: %d exact duplicate record(s) dropped", endpoint, stats.duplicates)
    return result
