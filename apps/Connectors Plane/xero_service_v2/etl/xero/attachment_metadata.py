"""Typed staging for Accounting attachment-list metadata, never file contents."""

import hashlib
import re
from urllib.parse import urlsplit
from uuid import UUID

from google.cloud import bigquery


TABLE = "attachment_metadata"
SNAPSHOT_TABLE = "attachment_list_snapshots"
SCHEMA = [
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("record_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("attachment_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("parent_endpoint", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("parent_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("filename", "STRING"),
    bigquery.SchemaField("mime_type", "STRING"),
    bigquery.SchemaField("content_length", "INTEGER"),
    bigquery.SchemaField("include_online", "BOOLEAN"),
    bigquery.SchemaField("url", "STRING"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]
SNAPSHOT_SCHEMA = [
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("snapshot_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("parent_endpoint", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("parent_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("run_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("attachment_ids", "STRING", mode="REPEATED"),
    bigquery.SchemaField("attachment_count", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]

_PARENT_ENDPOINTS = frozenset({
    "accounts", "bank_transactions", "bank_transfers", "contacts",
    "credit_notes", "invoices", "manual_journals", "purchase_orders",
    "quotes", "receipts", "repeating_invoices",
})


def _valid_uuid(value: object) -> bool:
    try:
        return isinstance(value, str) and str(UUID(value)) == value.lower()
    except (TypeError, ValueError):
        return False


def validate_parent_context(meta: dict) -> tuple[str, str]:
    """Require unambiguous, path-safe Accounting parent context."""
    endpoint = meta.get("x-parent-endpoint")
    parent_id = meta.get("x-parent-id")
    if endpoint not in _PARENT_ENDPOINTS:
        raise ValueError("attachment metadata has invalid parent endpoint")
    if not _valid_uuid(parent_id):
        raise ValueError("attachment metadata has invalid parent ID")
    return endpoint, parent_id


def extract_records(data: object) -> list[tuple[str, dict]]:
    """Validate the original Xero list wrapper before yielding any records."""
    if not isinstance(data, dict) or not isinstance(data.get("Attachments"), list):
        raise ValueError("Attachment response must have an Attachments array")
    records = []
    seen = set()
    for item in data["Attachments"]:
        if not isinstance(item, dict):
            raise ValueError("Attachment entry must be an object")
        attachment_id = item.get("AttachmentID")
        if not _valid_uuid(attachment_id):
            raise ValueError("Attachment entry has invalid AttachmentID")
        if attachment_id.lower() in seen:
            raise ValueError("Attachment list contains duplicate AttachmentID")
        seen.add(attachment_id.lower())
        records.append((attachment_id, item))
    return records


def _optional_string(payload: dict, field: str) -> str | None:
    value = payload.get(field)
    if value is not None and not isinstance(value, str):
        raise ValueError(f"Attachment {field} must be a string")
    return value


def _content_length(payload: dict) -> int | None:
    value = payload.get("ContentLength")
    if value is None:
        return None
    if isinstance(value, str) and re.fullmatch(r"[0-9]+", value):
        value = int(value)
    if type(value) is not int or not 0 <= value < 2**63:
        raise ValueError("Attachment ContentLength must fit a non-negative INTEGER")
    return value


def _safe_url(payload: dict) -> str | None:
    url = _optional_string(payload, "Url")
    if url is None:
        return None
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or parsed.netloc != "api.xero.com"
            or not parsed.path.startswith("/api.xro/2.0/")
            or parsed.query or parsed.fragment):
        raise ValueError("Attachment Url must be an unsigned Xero Accounting API URL")
    return url


def parse_record(record: dict) -> dict:
    parent_endpoint, parent_id = validate_parent_context(record.get("meta") or {})
    payload = record["payload"]
    attachment_id = payload.get("AttachmentID")
    if (not _valid_uuid(attachment_id)
            or record.get("record_id") != attachment_id):
        raise ValueError("Attachment record ID does not match AttachmentID")
    content_length = _content_length(payload)
    include_online = payload.get("IncludeOnline")
    if include_online is not None and type(include_online) is not bool:
        raise ValueError("Attachment IncludeOnline must be a boolean")
    return {
        "tenant_id": record["tenant_id"],
        "record_id": attachment_id,
        "attachment_id": attachment_id,
        "parent_endpoint": parent_endpoint,
        "parent_id": parent_id,
        "filename": _optional_string(payload, "FileName"),
        "mime_type": _optional_string(payload, "MimeType"),
        "content_length": content_length,
        "include_online": include_online,
        "url": _safe_url(payload),
        "synced_at": record["synced_at"],
    }


def parse_snapshot(record: dict) -> tuple[list[dict], dict]:
    meta = record.get("meta") or {}
    parent_endpoint, parent_id = validate_parent_context(meta)
    run_id = meta.get("x-run-id")
    source_path = record.get("source_object_path")
    if not isinstance(run_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", run_id):
        raise ValueError("attachment metadata requires a valid run ID")
    if not isinstance(source_path, str) or not source_path.endswith(".json.meta.json"):
        raise ValueError("attachment metadata requires a source sidecar path")
    attachments = extract_records(record["payload"])
    rows = [parse_record(record | {"record_id": attachment_id, "payload": payload})
            for attachment_id, payload in attachments]
    snapshot = {
        "tenant_id": record["tenant_id"],
        "snapshot_id": hashlib.sha256(source_path.encode("utf-8")).hexdigest(),
        "parent_endpoint": parent_endpoint,
        "parent_id": parent_id,
        "run_id": run_id,
        "attachment_ids": [attachment_id for attachment_id, _ in attachments],
        "attachment_count": len(attachments),
        "synced_at": record["synced_at"],
    }
    return rows, snapshot


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    rows, snapshots = [], []
    for record in reader.iter_records(TABLE, tenant_id=tenant_id, limit=limit):
        metadata, snapshot = parse_snapshot(record)
        rows.extend(metadata)
        snapshots.append(snapshot)
    if rows:
        writer.merge(
            TABLE, rows,
            key_columns=("tenant_id", "attachment_id", "parent_endpoint", "parent_id"),
            schema=SCHEMA,
        )
    if snapshots:
        writer.insert_if_absent(
            SNAPSHOT_TABLE, snapshots,
            key_columns=("tenant_id", "snapshot_id"),
            schema=SNAPSHOT_SCHEMA,
        )
    return {"records": len(rows), "snapshots": len(snapshots)}
