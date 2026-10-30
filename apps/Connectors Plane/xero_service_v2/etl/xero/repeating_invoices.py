"""Stage Xero repeating-invoice templates and current line snapshots."""

from etl.common.date_parser import parse_iso_date, parse_xero_datetime

BRONZE_TABLE = "xero_repeating_invoices"
HEADER_TABLE = "repeating_invoices"
LINE_TABLE = "repeating_invoice_line_snapshots"


def _object(value):
    return value if isinstance(value, dict) else {}


def _lines(payload):
    lines = payload.get("LineItems") or []
    if not isinstance(lines, list):
        raise ValueError("RepeatingInvoice.LineItems must be an array")
    return lines


def _tracking(items, index, field):
    return items[index].get(field) if index < len(items) else None


def parse_header(record: dict) -> dict:
    payload = record["payload"]
    contact = _object(payload.get("Contact"))
    schedule = _object(payload.get("Schedule"))
    return {
        "tenant_id": record["tenant_id"],
        "record_id": record["record_id"],
        "synced_at": record["last_seen_at"],
        "first_seen_at": record["first_seen_at"],
        "repeating_invoice_id": payload.get("RepeatingInvoiceID"),
        "invoice_type": payload.get("Type"),
        "status": payload.get("Status"),
        "reference": str(payload["Reference"]) if payload.get("Reference") is not None else None,
        "line_amount_types": payload.get("LineAmountTypes"),
        "currency_code": payload.get("CurrencyCode"),
        "branding_theme_id": payload.get("BrandingThemeID"),
        "contact_id": contact.get("ContactID"),
        "contact_name": contact.get("Name"),
        "sub_total": payload.get("SubTotal"),
        "total_tax": payload.get("TotalTax"),
        "total": payload.get("Total"),
        "schedule_unit": schedule.get("Unit"),
        "schedule_period": schedule.get("Period"),
        "schedule_due_date_type": schedule.get("DueDateType"),
        "schedule_due_date_offset": schedule.get("DueDate"),
        "schedule_start_date": parse_xero_datetime(schedule.get("StartDate")),
        "schedule_end_date": parse_xero_datetime(schedule.get("EndDate")),
        "next_scheduled_date": parse_xero_datetime(schedule.get("NextScheduledDate")),
        "next_scheduled_date_local": parse_iso_date(schedule.get("NextScheduledDateString")),
        "updated_at": parse_xero_datetime(payload.get("UpdatedDateUTC")),
        "has_attachments": payload.get("HasAttachments"),
        "approved_for_sending": payload.get("ApprovedForSending"),
        "include_pdf": payload.get("IncludePDF"),
        "send_copy": payload.get("SendCopy"),
        "mark_as_sent": payload.get("MarkAsSent"),
        "line_item_count": len(_lines(payload)),
    }


def parse_lines(record: dict) -> list[dict]:
    payload = record["payload"]
    rows = []
    for position, line in enumerate(_lines(payload)):
        if not isinstance(line, dict):
            raise ValueError("RepeatingInvoice.LineItems entries must be objects")
        tracking = line.get("Tracking") or []
        rows.append({
            "tenant_id": record["tenant_id"],
            "record_id": record["record_id"],
            "synced_at": record["last_seen_at"],
            "repeating_invoice_id": payload.get("RepeatingInvoiceID"),
            "line_position": position,
            "line_item_id": line.get("LineItemID"),
            "account_id": line.get("AccountID"),
            "account_code": line.get("AccountCode"),
            "item_code": line.get("ItemCode"),
            "description": line.get("Description"),
            "quantity": line.get("Quantity"),
            "unit_amount": line.get("UnitAmount"),
            "line_amount": line.get("LineAmount"),
            "tax_amount": line.get("TaxAmount"),
            "tax_type": line.get("TaxType"),
            "discount_rate": line.get("DiscountRate"),
            "discount_amount": line.get("DiscountAmount"),
            "tracking_category_1_id": _tracking(tracking, 0, "TrackingCategoryID"),
            "tracking_category_1_name": _tracking(tracking, 0, "Name"),
            "tracking_option_1_id": _tracking(tracking, 0, "TrackingOptionID"),
            "tracking_option_1_name": _tracking(tracking, 0, "Option"),
            "tracking_category_2_id": _tracking(tracking, 1, "TrackingCategoryID"),
            "tracking_category_2_name": _tracking(tracking, 1, "Name"),
            "tracking_option_2_id": _tracking(tracking, 1, "TrackingOptionID"),
            "tracking_option_2_name": _tracking(tracking, 1, "Option"),
        })
    return rows


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    headers, lines = [], []
    for record in reader.iter_records(BRONZE_TABLE, tenant_id=tenant_id, limit=limit):
        headers.append(parse_header(record))
        lines.extend(parse_lines(record))
    if headers:
        writer.merge(HEADER_TABLE, headers)
    if lines:
        writer.merge(
            LINE_TABLE, lines,
            key_columns=("tenant_id", "record_id", "line_position", "synced_at"),
        )
    return {"headers": len(headers), "lines": len(lines)}
