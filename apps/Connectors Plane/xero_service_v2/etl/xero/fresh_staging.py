"""Pure append-only staging of GCSReader envelopes, without legacy entity parsing.

Hashes use compact, sorted-key UTF-8 JSON; composite identities hash JSON arrays
to avoid delimiter ambiguity. Version identities include zero-based source record
ordinals, keeping repeated logical IDs separate without changing their payloads.
Rows are independent snapshots, not mutable source references. A receipt counts
emitted business records and all immediate children;
expected_records retains the header count, which can differ for response wrappers.
Direct Accounting GET responses keep request/run identity under their own endpoint.
Extended membership wrappers without IDs use run/endpoint/source/request identity.
Report logical IDs include the producer's sorted x-report-params query signature.
No network access, publishing, conflict resolution, or inferred deletion occurs here.
"""

from collections import Counter
from datetime import datetime, timedelta, timezone
import hashlib
import json
import re
from types import MappingProxyType
from typing import Any
from uuid import UUID


Row = dict[str, Any]
_TIMESTAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?")
_XERO_DATE = re.compile(r"/Date\((-?\d+)(?:[+-]\d{4})?\)/")
_NATIVE_ID_FIELDS = (
    "LineItemID",
    "PaymentID",
    "JournalLineID",
    "ContactID",
    "AttachmentID",
    "TrackingOptionID",
    "TrackingCategoryID",
    "AccountID",
    "InvoiceID",
    "CreditNoteID",
    "BankTransactionID",
    "BankTransferID",
    "ManualJournalID",
    "PurchaseOrderID",
    "QuoteID",
    "ReceiptID",
    "ExpenseClaimID",
    "PrepaymentID",
    "OverpaymentID",
    "BatchPaymentID",
    "BudgetID",
    "ItemID",
    "UserID",
    "ContactGroupID",
    "AddressID",
    "PhoneID",
    "assetId",
    "assetTypeId",
    "projectId",
    "taskId",
    "timeEntryId",
    "id",
    "ID",
)
# Lifecycle fields/deletion enums verified against the catalog's pinned OpenAPI:
# https://github.com/XeroAPI/Xero-OpenAPI/tree/fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f
# xero_accounting.yaml, xero_assets.yaml, xero-projects.yaml. Wrappers have no policy.
_LIFECYCLE_POLICIES = MappingProxyType(
    {
        "accounts": ("Status", True),
        "bank_transactions": ("Status", True),
        "bank_transfers": ("Status", True),
        "batch_payments": ("Status", True),
        "contact_groups": ("Status", True),
        "credit_notes": ("Status", True),
        "expense_claims": ("Status", True),
        "invoices": ("Status", True),
        "manual_journals": ("Status", True),
        "payments": ("Status", True),
        "purchase_orders": ("Status", True),
        "quotes": ("Status", True),
        "repeating_invoices": ("Status", True),
        "tax_rates": ("Status", True),
        "tracking_categories": ("Status", True),
        "linked_transactions": ("Status", False),
        "overpayments": ("Status", False),
        "prepayments": ("Status", False),
        "receipts": ("Status", False),
        "contacts": ("ContactStatus", False),
        "organisations": ("OrganisationStatus", False),
        "assets_assets": ("assetStatus", False),
        "projects_projects": ("status", False),
        "projects_tasks": ("status", False),
    }
)
# The full parent JSON remains immutable raw staging, but these configuration
# children are deliberately not projected into the warehouse child grain.
_OUT_OF_SCOPE_CHILD_COLLECTIONS = MappingProxyType(
    {"branding_themes": frozenset({"PaymentServices"})}
)


def _text(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value.strip() or value != value.strip():
        raise ValueError(f"{field} must be a nonempty string without surrounding whitespace")
    return value


def _uuid(value: Any, field: str) -> str:
    try:
        return str(UUID(_text(value, field)))
    except (ValueError, AttributeError) as exc:
        raise ValueError(f"{field} must be a UUID") from exc


def _numeric_string(value: Any, field: str, positive: bool = False) -> int:
    if not isinstance(value, str) or re.fullmatch(r"[0-9]+", value) is None:
        raise ValueError(f"{field} must be a numeric string")
    number = int(value)
    if positive and number == 0:
        raise ValueError(f"{field} must be positive")
    return number


def _timestamp(value: Any, field: str, require_aware: bool = True) -> datetime:
    try:
        if not isinstance(value, str) or _TIMESTAMP.fullmatch(value) is None:
            raise ValueError("not an ISO timestamp")
        parsed = datetime.fromisoformat(value)
        if parsed.tzinfo is None:
            if require_aware:
                raise ValueError("timezone required")
            # UpdatedDateUTC explicitly specifies UTC even without an ISO offset.
            parsed = parsed.replace(tzinfo=timezone.utc)
        return parsed.astimezone(timezone.utc)
    except (ValueError, OverflowError) as exc:
        raise ValueError(
            f"{field} must be a valid {'aware ' if require_aware else ''}timestamp"
        ) from exc


def _source_updated(value: Any) -> datetime | None:
    if value is None:
        return None
    match = _XERO_DATE.fullmatch(value) if isinstance(value, str) else None
    if match is None:
        return _timestamp(value, "UpdatedDateUTC", require_aware=False)
    try:
        return datetime(1970, 1, 1, tzinfo=timezone.utc) + timedelta(milliseconds=int(match[1]))
    except (ValueError, OverflowError) as exc:
        raise ValueError("UpdatedDateUTC is outside the timestamp range") from exc


def _validate_json(value: Any) -> None:
    if isinstance(value, dict):
        if any(not isinstance(key, str) for key in value):
            raise ValueError("JSON object keys must be strings")
        for child in value.values():
            _validate_json(child)
    elif isinstance(value, list):
        for child in value:
            _validate_json(child)
    elif value is not None and type(value) not in (str, int, float, bool):
        raise ValueError("payload must contain only JSON values")


def _json(value: Any) -> str:
    try:
        _validate_json(value)
        return json.dumps(
            value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False
        )
    except (TypeError, ValueError, RecursionError) as exc:
        raise ValueError("payload must be valid JSON") from exc


def _hash_json(value: Any) -> str:
    return _hash_payload(_json(value))


def _hash_payload(payload_json: str) -> str:
    try:
        return hashlib.sha256(payload_json.encode("utf-8")).hexdigest()
    except UnicodeError as exc:
        raise ValueError("payload must be valid UTF-8 JSON") from exc


def _receipt(meta: Row, bucket: str, object_name: str, meta_generation: str) -> Row:
    if not isinstance(meta, dict):
        raise ValueError("meta must be a dictionary")
    if meta.get("x-source-system") not in {"xero", "xero_v2"}:
        raise ValueError("x-source-system must be xero or xero_v2")
    generation = _uuid(meta.get("x-ingestion-generation"), "x-ingestion-generation")
    tenant_id = _text(meta.get("x-tenant-id"), "x-tenant-id")
    endpoint = _text(meta.get("x-endpoint"), "x-endpoint")
    if (tenant_id == "run") != (endpoint == "sync_manifest"):
        raise ValueError("x-tenant-id must be 'run' only for sync_manifest")
    if endpoint.startswith("report_"):
        _text(meta.get("x-report-params"), "x-report-params")
    run_id = _uuid(meta.get("x-run-id"), "x-run-id")
    observed_at = _timestamp(meta.get("x-synced-at"), "x-synced-at")
    if type(meta.get("x-http-status")) not in (str, int) or meta.get("x-http-status") not in (
        "200",
        "304",
        200,
        304,
    ):
        raise ValueError("x-http-status must be 200 or 304")
    _numeric_string(meta.get("x-data-generation"), "x-data-generation", positive=True)
    expected_records = _numeric_string(meta.get("x-record-count"), "x-record-count")
    identity = [
        generation,
        _text(bucket, "bucket"),
        _text(object_name, "object_name"),
        _text(meta_generation, "meta_generation"),
        meta["x-data-generation"],
    ]
    return {
        "receipt_id": _hash_json(identity),
        "generation": generation,
        "tenant_id": tenant_id,
        "endpoint": endpoint,
        "run_id": run_id,
        "observed_at": observed_at,
        "source_bucket": bucket,
        "source_object": object_name,
        "meta_generation": meta_generation,
        "data_generation": meta["x-data-generation"],
        "http_status": int(meta["x-http-status"]),
        "expected_records": expected_records,
        "record_count": 0,
        "child_count": 0,
        "metadata_json": _json(meta),
    }


def _is_direct(endpoint: str) -> bool:
    return endpoint.startswith("accounting_get_")


def _is_membership(endpoint: str) -> bool:
    return endpoint in {
        "assets_list_snapshots",
        "projects_list_snapshots",
        "bankfeeds_list_snapshots",
        "identity_list_snapshots",
        "finance_list_snapshots",
    }


def _is_wrapper(endpoint: str) -> bool:
    return endpoint == "attachment_metadata" or _is_direct(endpoint) or _is_membership(endpoint)


def _validate_record(record: Row, receipt: Row, source_system: str) -> None:
    if not isinstance(record, dict):
        raise ValueError("each record must be a dictionary")
    if record.get("tenant_id") != receipt["tenant_id"]:
        raise ValueError("record tenant_id does not match x-tenant-id")
    if "payload" not in record:
        raise ValueError("record payload is required")
    record_meta = record.get("meta")
    if record_meta is not None:
        if not isinstance(record_meta, dict):
            raise ValueError("record meta must be a dictionary")
        for key, field in (
            ("x-tenant-id", "tenant_id"),
            ("x-endpoint", "endpoint"),
            ("x-source-system", None),
            ("x-run-id", "run_id"),
            ("x-ingestion-generation", "generation"),
        ):
            if key in record_meta:
                value = record_meta[key]
                if key in {"x-run-id", "x-ingestion-generation"}:
                    value = _uuid(value, key)
                if value != (receipt[field] if field else source_system):
                    raise ValueError(f"record meta {key} does not match observation")
    payload = record["payload"]
    if isinstance(payload, dict):
        for field in ("tenant_id", "TenantID", "TenantId", "tenantId"):
            if field in payload and payload[field] != receipt["tenant_id"]:
                raise ValueError(f"payload {field} does not match x-tenant-id")


def _record_id(record: Row, receipt: Row, meta: Row) -> str:
    if _is_direct(receipt["endpoint"]):
        parent_id = meta.get("x-parent-record-id")
        if parent_id is not None:
            parent_id = _text(parent_id, "x-parent-record-id")
            operation_id = _text(meta.get("x-operation-id"), "x-operation-id")
            return f"{operation_id}:{parent_id}"
        run_id = _uuid(receipt["run_id"], "x-run-id")
        if "record_id" in record and _uuid(record["record_id"], "record_id") != run_id:
            raise ValueError("direct record_id must match x-run-id request identity")
        return run_id
    if "record_id" in record:
        record_id = _text(record["record_id"], "record_id")
        if receipt["endpoint"].startswith("report_"):
            return _hash_json([record_id, _text(meta.get("x-report-params"), "x-report-params")])
        return record_id
    if _is_membership(receipt["endpoint"]):
        source_endpoint = _text(meta.get("x-source-endpoint"), "x-source-endpoint")
        request_key = meta.get("x-request-key")
        if not isinstance(request_key, str):
            raise ValueError("x-request-key must be a string for membership identity")
        return _hash_json([receipt["run_id"], receipt["endpoint"], source_endpoint, request_key])
    raise ValueError("record_id is required")


def _source_lifecycle(payload: Row, endpoint: str) -> tuple[str | None, bool]:
    field, deletion_documented = _LIFECYCLE_POLICIES.get(endpoint, ("", False))
    value = payload.get(field) if field else None
    status = value if isinstance(value, str) and value else None
    return status, deletion_documented and status == "DELETED"


def _native_id(child: Any) -> str | None:
    if not isinstance(child, dict):
        return None
    fields = (
        *_NATIVE_ID_FIELDS,
        *(
            key
            for key in sorted(child)
            if key.endswith(("ID", "Id")) and key not in _NATIVE_ID_FIELDS
        ),
    )
    candidates = [
        str(child[key]) for key in fields if key in child and type(child[key]) in (str, int)
    ]
    return next((value for value in candidates if value), next(iter(candidates), None))


def _children(parent: Row, collection_name: str, children: list[Any]) -> list[Row]:
    native_ids = [_native_id(child) for child in children]
    counts = Counter(native_ids)
    # Reserve source-position keys so even unusual native IDs cannot collide.
    position_keys = {f"position:{index}" for index in range(len(children))}
    scope = {
        key: parent[key]
        for key in ("generation", "tenant_id", "endpoint", "record_id", "version_id")
    }
    return [
        scope
        | {
            "collection_name": collection_name,
            "child_key": native
            if native and counts[native] == 1 and native not in position_keys
            else f"position:{index}",
            "line_position": index,
            "native_id": native,
            "payload_json": payload_json,
            "payload_hash": _hash_payload(payload_json),
        }
        for index, (child, native) in enumerate(zip(children, native_ids))
        for payload_json in [_json(child)]
    ]


def _stage_record(
    record: Row,
    receipt: Row,
    meta: Row,
    source_position: int,
) -> tuple[Row | None, list[Row], list[Row]]:
    _validate_record(record, receipt, meta["x-source-system"])
    record_id = _record_id(record, receipt, meta)
    payload = record["payload"]
    if payload is None and _is_direct(receipt["endpoint"]):
        return None, [], []
    if not isinstance(payload, dict) and not (
        _is_direct(receipt["endpoint"]) and isinstance(payload, list)
    ):
        raise ValueError("record payload must be a JSON object")
    source = payload if isinstance(payload, dict) else {}
    excluded_collections = _OUT_OF_SCOPE_CHILD_COLLECTIONS.get(receipt["endpoint"], frozenset())
    staged_source = {
        key: value for key, value in source.items() if key not in excluded_collections
    }
    staged_payload = staged_source if isinstance(payload, dict) else payload
    payload_json = _json(staged_payload)
    status, deleted = _source_lifecycle(source, receipt["endpoint"])
    parent = {
        key: receipt[key]
        for key in ("generation", "tenant_id", "endpoint", "observed_at", "run_id", "receipt_id")
    } | {
        "record_id": record_id,
        "version_id": _hash_json(
            [
                receipt["receipt_id"],
                receipt["endpoint"],
                receipt["tenant_id"],
                record_id,
                source_position,
            ]
        ),
        "source_updated_at": _source_updated(source.get("UpdatedDateUTC")),
        "source_status": status,
        "source_deleted": deleted,
        "payload_json": payload_json,
        "payload_hash": _hash_payload(payload_json),
    }
    scope = {
        key: parent[key]
        for key in (
            "generation",
            "tenant_id",
            "endpoint",
            "record_id",
            "version_id",
            "observed_at",
            "source_updated_at",
        )
    }
    collections = [
        scope | {"collection_name": key, "child_count": len(value)}
        for key, value in sorted(staged_source.items())
        if isinstance(value, list)
    ]
    children = [
        child
        for collection in collections
        for child in _children(
            parent, collection["collection_name"], staged_source[collection["collection_name"]]
        )
    ]
    return parent, collections, children


def build_observation(
    meta: Row,
    records: list[Row],
    bucket: str,
    object_name: str,
    meta_generation: str,
) -> dict[str, list[Row]]:
    """Validate one observation and return all four tables, without changing inputs.

    HTTP 304 ignores supplied cached records and emits only a receipt. Direct
    binary envelopes (payload=None) also emit no business rows. Their raw pointer
    remains in source_bucket/source_object and the unchanged metadata_json.
    sync_manifest requires reserved tenant 'run', zero header count, and no records.
    Missing/null lists never assert collection membership; empty lists do.
    """
    receipt = _receipt(meta, bucket, object_name, meta_generation)
    if not isinstance(records, list):
        raise ValueError("records must be a list of dictionaries")
    if receipt["endpoint"] == "sync_manifest" and (records or receipt["expected_records"] != 0):
        raise ValueError("sync_manifest requires x-record-count=0 and no business records")
    if receipt["http_status"] == 304:
        staged = []
    else:
        if not _is_wrapper(receipt["endpoint"]) and receipt["expected_records"] != len(records):
            raise ValueError("x-record-count does not match records")
        staged = [
            _stage_record(record, receipt, meta, index) for index, record in enumerate(records)
        ]
    parents = [parent for parent, _, _ in staged if parent is not None]
    collections = [row for _, rows, _ in staged for row in rows]
    children = [row for _, _, rows in staged for row in rows]
    return {
        "v2_record_versions": parents,
        "v2_collection_versions": collections,
        "v2_child_versions": children,
        "v2_ingest_receipts": [
            receipt | {"record_count": len(parents), "child_count": len(children)}
        ],
    }
