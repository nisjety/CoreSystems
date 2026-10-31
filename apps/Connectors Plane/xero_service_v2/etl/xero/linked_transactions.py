"""Stage Xero billable-expense links between source and target transactions."""

from datetime import timezone

from etl.common.date_parser import parse_iso_datetime, parse_xero_datetime

BRONZE_TABLE = "xero_linked_transactions"
HEADER_TABLE = "linked_transactions"


def _updated_at(value):
    parsed = parse_xero_datetime(value) or parse_iso_datetime(value)
    if parsed is None:
        return None
    return parsed.replace(tzinfo=timezone.utc) if parsed.tzinfo is None else parsed.astimezone(timezone.utc)


def parse_header(record: dict) -> dict:
    payload = record["payload"]
    return {
        "tenant_id": record["tenant_id"],
        "record_id": record["record_id"],
        "synced_at": record["last_seen_at"],
        "first_seen_at": record["first_seen_at"],
        "linked_transaction_id": payload.get("LinkedTransactionID"),
        "source_transaction_id": payload.get("SourceTransactionID"),
        "source_line_item_id": payload.get("SourceLineItemID"),
        "source_transaction_type": payload.get("SourceTransactionTypeCode"),
        "contact_id": payload.get("ContactID"),
        "target_transaction_id": payload.get("TargetTransactionID"),
        "target_line_item_id": payload.get("TargetLineItemID"),
        "linked_transaction_type": payload.get("Type"),
        "status": payload.get("Status"),
        "updated_at": _updated_at(payload.get("UpdatedDateUTC")),
    }


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    headers = [
        parse_header(record)
        for record in reader.iter_records(BRONZE_TABLE, tenant_id=tenant_id, limit=limit)
    ]
    if headers:
        writer.merge(HEADER_TABLE, headers)
    return {"headers": len(headers)}
