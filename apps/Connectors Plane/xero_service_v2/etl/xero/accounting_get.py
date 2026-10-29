"""Append-only warehouse evidence for catalogued Accounting GET responses."""

import json
import os
import re
from pathlib import Path

from google.cloud import bigquery

from etl.xero.warehouse import ordinary_staging_dataset


_CATALOG = json.loads(Path(__file__).with_name("accounting_get_ops.json").read_text())
SPEC_COMMIT = _CATALOG["spec_commit"]
OPERATIONS: dict[str, bool] = _CATALOG["operations"]
_PREFIX = "accounting_get_"
_ENDPOINT_OPERATIONS = {
    f"{_PREFIX}{operation_id.lower()}": (operation_id, binary)
    for operation_id, binary in OPERATIONS.items()
}
if len(_ENDPOINT_OPERATIONS) != len(OPERATIONS):
    raise RuntimeError("Accounting GET operation IDs collide after GCS key normalization")
_MAX_BQ_JSON_BYTES = 8 * 1024 * 1024

SCHEMA = [
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("operation_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("run_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("request_descriptor", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("raw_bucket", "STRING"),
    bigquery.SchemaField("raw_object_path", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("response_content_type", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("http_status", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("is_binary", "BOOLEAN", mode="REQUIRED"),
    bigquery.SchemaField("payload_in_bigquery", "BOOLEAN", mode="REQUIRED"),
    bigquery.SchemaField("payload_json", "STRING"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]


def operation_info(endpoint: str) -> tuple[str, bool]:
    if not isinstance(endpoint, str) or not endpoint.startswith(_PREFIX):
        raise ValueError("unsupported Accounting GET endpoint")
    operation = _ENDPOINT_OPERATIONS.get(endpoint)
    if operation is None:
        raise ValueError("unsupported Accounting GET operation")
    return operation


def target_dataset(endpoint: str, bucket: str) -> tuple[str, str]:
    _, binary = operation_info(endpoint)
    project = os.environ.get("GCP_PROJECT", "")
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", project):
        raise ValueError("GCP_PROJECT is required for Accounting GET responses")
    ordinary = os.environ.get("GCS_BUCKET", "")
    document = os.environ.get("XERO_DOCUMENT_GCS_BUCKET", "")
    if binary:
        dataset = os.environ.get("XERO_DOCUMENT_DATASET", "")
        if not document or document == ordinary or bucket != document:
            raise ValueError("binary Accounting GET requires a separate document bucket")
        if dataset != "staging_xero_documents":
            raise ValueError("XERO_DOCUMENT_DATASET must be staging_xero_documents")
        return project, dataset
    if not ordinary or bucket != ordinary or (document and bucket == document):
        raise ValueError("JSON Accounting GET requires the ordinary raw bucket")
    return project, ordinary_staging_dataset()


def validate_sidecar(endpoint: str, meta: dict) -> tuple[str, bool]:
    operation_id, binary = operation_info(endpoint)
    if meta.get("x-operation-id") != operation_id:
        raise ValueError("Accounting GET operation does not match endpoint")
    if meta.get("x-spec-commit") != SPEC_COMMIT:
        raise ValueError("Accounting GET spec revision mismatch")
    if meta.get("x-response-binary") != str(binary).lower():
        raise ValueError("Accounting GET binary classification mismatch")
    descriptor = meta.get("x-request-descriptor")
    if not isinstance(descriptor, str) or len(descriptor) > 2048:
        raise ValueError("invalid Accounting GET request descriptor")
    try:
        parsed = json.loads(descriptor)
    except (TypeError, ValueError) as exc:
        raise ValueError("invalid Accounting GET request descriptor") from exc
    if not isinstance(parsed, dict) or parsed.get("operation_id") != operation_id:
        raise ValueError("Accounting GET request descriptor operation mismatch")
    if not all(isinstance(parsed.get(name), list) for name in ("path", "query", "headers")):
        raise ValueError("Accounting GET request descriptor lacks parameter arrays")
    if not meta.get("x-run-id") or not meta.get("x-response-content-type"):
        raise ValueError("Accounting GET sidecar lacks response metadata")
    return operation_id, binary


def run(reader, writer, endpoint: str) -> dict[str, int]:
    operation_id, binary = operation_info(endpoint)
    rows = []
    for record in reader.iter_records(endpoint):
        meta = record.get("meta") or {}
        validate_sidecar(endpoint, meta)
        payload = record.get("payload")
        if binary and payload is not None:
            raise ValueError("binary Accounting GET must not enter BigQuery payload")
        if not binary and not isinstance(payload, (dict, list)):
            raise ValueError("JSON Accounting GET must contain an object or array")
        payload_json = None if binary else json.dumps(
            payload, separators=(",", ":"), ensure_ascii=False)
        if payload_json is not None and len(payload_json.encode("utf-8")) > _MAX_BQ_JSON_BYTES:
            payload_json = None
        rows.append({
            "tenant_id": record["tenant_id"],
            "operation_id": operation_id,
            "run_id": meta["x-run-id"],
            "request_descriptor": meta["x-request-descriptor"],
            "raw_bucket": record.get("source_bucket", ""),
            "raw_object_path": record["source_object_path"],
            "response_content_type": meta["x-response-content-type"],
            "http_status": int(meta["x-http-status"]),
            "is_binary": binary,
            "payload_in_bigquery": payload_json is not None,
            "payload_json": payload_json,
            "synced_at": record["synced_at"],
        })
    if not rows:
        return {"records": 0}
    return {"records": writer.insert_if_absent(
        "accounting_get_responses", rows,
        # A run can issue one direct GET per source resource. The canonical
        # request descriptor preserves those distinct resources but collapses
        # retry deliveries of the same Xero request. Raw history retains every
        # immutable object path as the forensic source of truth.
        key_columns=("tenant_id", "operation_id", "run_id", "request_descriptor"),
        schema=SCHEMA,
    )}
