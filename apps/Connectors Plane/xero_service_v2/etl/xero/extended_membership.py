"""Immutable evidence that an extended Xero collection was fetched completely."""

import hashlib
from uuid import UUID

from google.cloud import bigquery

from etl.xero.extended import COLLECTION_ENDPOINTS, LIST_SNAPSHOT_ENDPOINTS


TABLE = "extended_list_snapshots"
SCHEMA = [
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("snapshot_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("api_family", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("source_endpoint", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("request_key", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("run_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("record_ids", "STRING", mode="REPEATED"),
    bigquery.SchemaField("record_count", "INTEGER", mode="REQUIRED"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]


def parse_snapshot(record: dict) -> dict:
    meta = record.get("meta") or {}
    body = record.get("payload")
    endpoint = meta.get("x-endpoint")
    family = meta.get("x-api-family")
    source = meta.get("x-source-endpoint")
    path = record.get("source_object_path")
    if endpoint not in LIST_SNAPSHOT_ENDPOINTS or family != endpoint.removesuffix("_list_snapshots"):
        raise ValueError("invalid extended list snapshot family")
    if source not in COLLECTION_ENDPOINTS or not source.startswith(f"{family}_"):
        raise ValueError("invalid extended list source endpoint")
    if meta.get("x-http-status") != "200":
        raise ValueError("extended list snapshot requires HTTP 200")
    if not isinstance(path, str) or not path.endswith(".json.meta.json"):
        raise ValueError("extended list snapshot requires source sidecar")
    try:
        run_id = str(UUID(meta["x-run-id"]))
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("extended list snapshot requires run ID") from exc
    if not isinstance(body, dict):
        raise ValueError("extended list snapshot must be an object")
    request_key = body.get("request_key")
    ids = body.get("record_ids")
    count = body.get("record_count")
    if (body.get("source_endpoint") != source or not isinstance(request_key, str)
            or request_key != meta.get("x-request-key") or not isinstance(ids, list)
            or any(not isinstance(item, str) or not item or len(item) > 256 for item in ids)
            or len(ids) != len(set(ids)) or type(count) is not int or count != len(ids)
            or str(count) != meta.get("x-record-count")):
        raise ValueError("incomplete or inconsistent extended list snapshot")
    if source == "assets_assets" and request_key not in {"DRAFT", "REGISTERED", "DISPOSED"}:
        raise ValueError("asset list snapshot requires a known status")
    if source in {"projects_tasks", "projects_time"}:
        try:
            UUID(request_key)
        except ValueError as exc:
            raise ValueError("project child list requires a parent UUID") from exc
    elif source != "assets_assets" and request_key:
        raise ValueError("unpartitioned list snapshot has a request key")
    return {
        "tenant_id": record["tenant_id"],
        "snapshot_id": hashlib.sha256(path.encode("utf-8")).hexdigest(),
        "api_family": family,
        "source_endpoint": source,
        "request_key": request_key,
        "run_id": run_id,
        "record_ids": ids,
        "record_count": count,
        "synced_at": record["synced_at"],
    }


def run(reader, writer, tenant_id=None, limit=None):
    rows = [parse_snapshot(record) for record in
            reader.iter_records(TABLE, tenant_id=tenant_id, limit=limit)]
    if rows:
        writer.insert_if_absent(TABLE, rows, key_columns=("tenant_id", "snapshot_id"), schema=SCHEMA)
    return {"snapshots": len(rows)}
