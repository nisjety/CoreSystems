"""Lossless staging for non-Accounting Xero API resources."""

import json
import os
import re
from uuid import UUID

from google.cloud import bigquery

from etl.xero.warehouse import ordinary_staging_dataset


# Response shape, identifier field, and whether a dated response is a snapshot.
EXTENDED_ENDPOINTS: dict[str, tuple[str, str | None, bool]] = {
    "assets_assets": ("items", "assetId", False),
    "assets_asset_details": ("object", "assetId", False),
    "assets_asset_types": ("array", "assetTypeId", False),
    "assets_settings": ("object", None, False),
    "bankfeeds_feed_connections": ("items", "id", False),
    "bankfeeds_feed_connection_details": ("object", "id", False),
    "bankfeeds_statements": ("items", "id", False),
    "bankfeeds_statement_details": ("object", "id", False),
    "projects_projects": ("items", "projectId", False),
    "projects_project_details": ("object", "projectId", False),
    "projects_users": ("items", "userId", False),
    "projects_tasks": ("items", "taskId", False),
    "projects_task_details": ("object", "taskId", False),
    "projects_time": ("items", "timeEntryId", False),
    "projects_time_details": ("object", "timeEntryId", False),
    "identity_connections": ("array", "id", False),
    "finance_cash_validation": ("array", "accountId", True),
    "finance_balance_sheet": ("object", None, True),
    "finance_cashflow": ("object", None, True),
    "finance_profit_and_loss": ("object", None, True),
    "finance_trial_balance": ("object", None, True),
    "finance_revenue_by_contact": ("object", None, True),
    "finance_expense_by_contact": ("object", None, True),
    "finance_bank_statements_plus": ("object", "bankAccountId", True),
}

SENSITIVE_FAMILIES = frozenset({"bankfeeds", "finance", "identity"})

COLLECTION_ENDPOINTS = frozenset({
    "assets_assets", "assets_asset_types",
    "projects_projects", "projects_users", "projects_tasks", "projects_time",
    "bankfeeds_feed_connections", "bankfeeds_statements", "identity_connections",
})
PROJECT_CHILD_ENDPOINTS = frozenset({
    "projects_tasks", "projects_task_details", "projects_time", "projects_time_details",
})
LIST_SNAPSHOT_ENDPOINTS = frozenset({
    "assets_list_snapshots", "projects_list_snapshots",
    "bankfeeds_list_snapshots", "identity_list_snapshots",
})

EXTENDED_RECORD_SCHEMA = [
    bigquery.SchemaField("tenant_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("endpoint", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("api_family", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("record_id", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("payload_json", "STRING", mode="REQUIRED"),
    bigquery.SchemaField("snapshot_date", "STRING"),
    bigquery.SchemaField("request_key", "STRING"),
    bigquery.SchemaField("run_id", "STRING"),
    bigquery.SchemaField("synced_at", "TIMESTAMP", mode="REQUIRED"),
]


def warehouse_target(endpoint: str, *, bucket: str) -> tuple[str, str]:
    """Require explicit project and restricted dataset for sensitive families."""
    if endpoint not in EXTENDED_ENDPOINTS and endpoint not in LIST_SNAPSHOT_ENDPOINTS:
        raise ValueError(f"unsupported extended endpoint: {endpoint}")
    if not isinstance(bucket, str) or not bucket:
        raise ValueError("GCS bucket must be explicitly set for extended Xero data")
    project = os.environ.get("GCP_PROJECT", "")
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", project):
        raise ValueError("GCP_PROJECT must be explicitly set for extended Xero data")
    family = endpoint.split("_", 1)[0]
    if family in SENSITIVE_FAMILIES:
        restricted_bucket = os.environ.get("XERO_SENSITIVE_GCS_BUCKET", "")
        if not restricted_bucket or bucket != restricted_bucket:
            raise ValueError("sensitive Xero data requires the configured restricted bucket")
        dataset = os.environ.get("XERO_SENSITIVE_DATASET", "")
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", dataset):
            raise ValueError("XERO_SENSITIVE_DATASET is required for sensitive Xero data")
        if dataset != "staging_xero_sensitive":
            raise ValueError("XERO_SENSITIVE_DATASET must be staging_xero_sensitive for Dataform")
        return project, dataset
    if bucket and bucket == os.environ.get("XERO_SENSITIVE_GCS_BUCKET", ""):
        raise ValueError("ordinary Xero data cannot be loaded from the restricted bucket")
    return project, ordinary_staging_dataset()


def extract_records(data, endpoint: str, meta: dict) -> list[tuple[str, dict]]:
    """Validate a documented response shape and retain every record unchanged."""
    shape, id_field, snapshot = EXTENDED_ENDPOINTS[endpoint]
    if shape == "items":
        if not isinstance(data, dict) or not isinstance(data.get("items"), list):
            raise ValueError(f"{endpoint} response has no items array")
        items = data["items"]
    elif shape == "array":
        if not isinstance(data, list):
            raise ValueError(f"{endpoint} response is not an array")
        items = data
    else:
        if not isinstance(data, dict):
            raise ValueError(f"{endpoint} response is not an object")
        items = [data]

    request_key = meta.get("x-request-key") or meta.get("x-snapshot-date")
    if snapshot and not request_key:
        raise ValueError(f"{endpoint} snapshot has no request key")
    result = []
    for item in items:
        if not isinstance(item, dict):
            raise ValueError(f"{endpoint} contains a non-object record")
        native_id = item.get(id_field) if id_field else None
        if id_field and not native_id:
            raise ValueError(f"{endpoint} record missing {id_field}")
        record_id = str(native_id) if native_id else "settings"
        if endpoint in PROJECT_CHILD_ENDPOINTS:
            parent = meta.get("x-request-key")
            try:
                parent = str(UUID(parent))
            except (TypeError, ValueError) as exc:
                raise ValueError(f"{endpoint} requires project UUID request key") from exc
            record_id = f"{parent}:{record_id}"
        if snapshot:
            record_id = f"{request_key}:{record_id}" if native_id else str(request_key)
        result.append((record_id, item))
    return result


def run(reader, writer, endpoint: str):
    """Merge complete response records into one flexible staging table."""
    if endpoint not in EXTENDED_ENDPOINTS:
        raise ValueError(f"unsupported extended endpoint: {endpoint}")
    rows = []
    for record in reader.iter_records(endpoint):
        meta = record.get("meta") or {}
        rows.append({
            "tenant_id": record["tenant_id"],
            "endpoint": endpoint,
            "api_family": meta.get("x-api-family", endpoint.split("_", 1)[0]),
            "record_id": record["record_id"],
            "payload_json": json.dumps(record["payload"], separators=(",", ":"),
                                       ensure_ascii=False),
            "snapshot_date": meta.get("x-snapshot-date") or "",
            "request_key": meta.get("x-request-key") or "",
            "run_id": meta.get("x-run-id") or "",
            "synced_at": record["synced_at"],
        })
    if not rows:
        return {"records": 0}
    return {"records": writer.merge(
        "extended_records", rows,
        key_columns=("tenant_id", "endpoint", "record_id"),
        schema=EXTENDED_RECORD_SCHEMA,
    )}
