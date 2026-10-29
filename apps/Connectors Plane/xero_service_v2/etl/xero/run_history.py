"""Typed, payload-free warehouse history derived from durable GCS run manifests."""

import os
import re
from datetime import date, datetime
from uuid import UUID

from google.cloud import bigquery

from etl.xero import accounting_get
from etl.xero.extended import warehouse_target
from etl.xero.warehouse import ordinary_staging_dataset


RUN_SCHEMA = [
    bigquery.SchemaField("run_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("started_at", "TIMESTAMP", mode="REQUIRED"),
    bigquery.SchemaField("finished_at", "TIMESTAMP", mode="REQUIRED"),
    bigquery.SchemaField("mode", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("status", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("window_days", "INTEGER"),
    bigquery.SchemaField("entity_count", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("error_count", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("record_count", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("unavailable_count", "INTEGER"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]

ENTITY_SCHEMA = [
    bigquery.SchemaField("run_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("outcome_index", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("endpoint", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("request_key", "STRING"),
    bigquery.SchemaField("pages", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("records", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("termination", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("error_present", "BOOLEAN", mode="REQUIRED"),
    bigquery.SchemaField("outcome_class", "STRING"),
    bigquery.SchemaField("http_status", "INTEGER"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]

_MANIFEST_PATH = re.compile(
    r"_manifests/xero/(?P<date>\d{4}-\d{2}-\d{2})/"
    r"(?P<run_id>[0-9a-fA-F-]{36})\.json\.meta\.json"
)
_SENSITIVE_PREFIXES = ("bankfeeds_", "finance_", "identity_")
_OUTCOME_CLASSES = frozenset({
    "not_applicable",
    "source_unavailable",
    "access_denied",
    "failed_retryable",
    "failed_permanent",
    "failed_unknown",
})
_FAILURE_OUTCOME_CLASSES = frozenset({
    "failed_retryable",
    "failed_permanent",
    "failed_unknown",
})
_ACTIONABLE_OUTCOME_CLASSES = _FAILURE_OUTCOME_CLASSES | frozenset({"access_denied"})
_UNAVAILABLE_OUTCOME_CLASSES = frozenset({
    "not_applicable",
    "source_unavailable",
    "access_denied",
})


def _timestamp(value: str) -> datetime:
    try:
        result = datetime.fromisoformat(value)
    except (TypeError, ValueError) as exc:
        raise ValueError("invalid manifest timestamp") from exc
    if result.tzinfo is None:
        raise ValueError("manifest timestamp must have a timezone")
    return result


def _count(value, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"invalid manifest {field}")
    return value


def _outcome_class(entity: dict) -> str | None:
    value = entity.get("outcome_class")
    if value is None:
        return None
    if value not in _OUTCOME_CLASSES:
        raise ValueError("manifest outcome has an unsupported outcome class")
    return value


def _http_status(entity: dict) -> int | None:
    value = entity.get("http_status")
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or not 100 <= value <= 599:
        raise ValueError("manifest outcome has an invalid HTTP status")
    return value


def manifest_rows(meta_path: str, manifest: dict) -> tuple[dict, list[dict]]:
    """Validate a manifest and produce keyed rows without copying error text."""
    match = _MANIFEST_PATH.fullmatch(meta_path)
    if not match:
        raise ValueError("invalid manifest path")
    if not isinstance(manifest, dict):
        raise ValueError("manifest must be an object")
    try:
        path_run_id = str(UUID(match.group("run_id")))
        body_run_id = str(UUID(manifest["run_id"]))
        date.fromisoformat(match.group("date"))
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("invalid manifest run ID or date") from exc
    if path_run_id != body_run_id:
        raise ValueError("manifest run ID does not match path")
    started = _timestamp(manifest.get("started_at"))
    finished = _timestamp(manifest.get("finished_at"))
    if finished < started:
        raise ValueError("manifest finished before it started")
    mode = manifest.get("mode")
    if not isinstance(mode, str) or not mode:
        raise ValueError("invalid manifest mode")
    raw_entities = manifest.get("entities")
    if not isinstance(raw_entities, list):
        raise ValueError("manifest entities must be an array")
    window_days = manifest.get("window_days")
    if window_days is not None:
        window_days = _count(window_days, "window_days")

    outcomes = []
    for index, entity in enumerate(raw_entities):
        if not isinstance(entity, dict):
            raise ValueError("manifest contains a non-object outcome")
        tenant = entity.get("tenant")
        endpoint = entity.get("endpoint")
        termination = entity.get("termination")
        if not all(isinstance(value, str) and value for value in
                   (tenant, endpoint, termination)):
            raise ValueError("manifest outcome lacks tenant, endpoint or termination")
        request_key = entity.get("request_key")
        if request_key is not None and (not isinstance(request_key, str) or not request_key):
            raise ValueError("manifest outcome has an invalid request key")
        outcome_class = _outcome_class(entity)
        http_status = _http_status(entity)
        if outcome_class is not None and outcome_class != "not_applicable" and entity.get("error") is None:
            raise ValueError("manifest outcome class requires an error")
        if http_status is not None and entity.get("error") is None:
            raise ValueError("manifest HTTP status requires an error")
        outcomes.append({
            "run_id": body_run_id,
            "outcome_index": index,
            "tenant_id": tenant,
            "endpoint": endpoint,
            "request_key": request_key,
            "pages": _count(entity.get("pages"), "pages"),
            "records": _count(entity.get("records"), "records"),
            "termination": termination,
            "error_present": bool(entity.get("error")),
            "outcome_class": outcome_class,
            "http_status": http_status,
            "synced_at": finished,
        })

    errors = sum(
        row["termination"] == "hit_max_pages_cap"
        or row["outcome_class"] in _ACTIONABLE_OUTCOME_CLASSES
        or (row["outcome_class"] is None and (
            row["error_present"] or row["termination"] in ("error", "partial_detail_failure")
        ))
        for row in outcomes
    )
    unavailable = sum(row["outcome_class"] in _UNAVAILABLE_OUTCOME_CLASSES for row in outcomes)
    run = {
        "run_id": body_run_id,
        "started_at": started,
        "finished_at": finished,
        "mode": mode,
        "status": "failed" if errors else "succeeded",
        "window_days": window_days,
        "entity_count": len(outcomes),
        "error_count": errors,
        "record_count": sum(row["records"] for row in outcomes),
        "unavailable_count": unavailable,
        "synced_at": finished,
    }
    return run, outcomes


def target_dataset(bucket: str, endpoints: list[str], default_project: str) -> tuple[str, str]:
    """Route a manifest summary to the one dataset that holds its raw evidence.

    An ordinary Accounting collection can include parent-scoped Accounting GET
    detail responses in its manifest. Both response families are JSON in the
    ordinary bucket, so their run evidence belongs in the same ordinary
    staging dataset. Binary Accounting GET responses remain isolated in the
    document bucket and may not mix with ordinary responses.
    """
    accounting = [endpoint for endpoint in endpoints if endpoint.startswith("accounting_get_")]
    ordinary = os.environ.get("GCS_BUCKET")
    document = os.environ.get("XERO_DOCUMENT_GCS_BUCKET")
    restricted = os.environ.get("XERO_SENSITIVE_GCS_BUCKET")

    if bucket == document and bucket != ordinary:
        if len(accounting) != len(endpoints):
            raise ValueError("document manifests may contain only Accounting GET document operations")
        destinations = {accounting_get.target_dataset(endpoint, bucket) for endpoint in accounting}
        if len(destinations) != 1:
            raise ValueError("Accounting GET manifest mixes document and JSON operations")
        return destinations.pop()

    sensitive = any(endpoint.startswith(_SENSITIVE_PREFIXES) for endpoint in endpoints)
    if sensitive and bucket != restricted:
        raise ValueError("sensitive manifest requires the restricted bucket")
    if restricted and bucket == restricted:
        return warehouse_target("finance_balance_sheet", bucket=bucket)

    if ordinary and bucket != ordinary:
        raise ValueError("ordinary manifest requires the ordinary raw bucket")

    ordinary_destination = (default_project, ordinary_staging_dataset())
    for endpoint in accounting:
        if accounting_get.target_dataset(endpoint, bucket) != ordinary_destination:
            raise ValueError("ordinary manifest includes an Accounting GET document operation")
    return default_project, ordinary_staging_dataset()


def write_manifest_rows(writer, run: dict, outcomes: list[dict]) -> int:
    written = writer.merge("sync_runs", [run], key_columns=("run_id",), schema=RUN_SCHEMA)
    if outcomes:
        written += writer.merge(
            "sync_run_entities", outcomes,
            key_columns=("run_id", "outcome_index"), schema=ENTITY_SCHEMA,
        )
    return written
