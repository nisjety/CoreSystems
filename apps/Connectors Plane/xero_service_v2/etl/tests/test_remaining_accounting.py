"""Active Accounting endpoints stage their complete queryable grain."""

from datetime import date, datetime, timezone

from etl.xero import linked_transactions, repeating_invoices


SYNCED_AT = datetime(2026, 9, 29, tzinfo=timezone.utc)


def _record(payload, record_id):
    return {
        "tenant_id": "tenant-a",
        "record_id": record_id,
        "first_seen_at": SYNCED_AT,
        "last_seen_at": SYNCED_AT,
        "payload": payload,
    }


def test_linked_transaction_preserves_source_and_optional_target():
    record = _record({
        "LinkedTransactionID": "link-a",
        "SourceTransactionID": "bill-a",
        "SourceLineItemID": "line-a",
        "SourceTransactionTypeCode": "ACCPAY",
        "ContactID": "contact-a",
        "TargetTransactionID": "invoice-a",
        "TargetLineItemID": "line-b",
        "Type": "BILLABLEEXPENSE",
        "Status": "BILLED",
        "UpdatedDateUTC": "2026-09-29T10:30:00",
    }, "link-a")
    row = linked_transactions.parse_header(record)
    assert row["source_transaction_id"] == "bill-a"
    assert row["source_line_item_id"] == "line-a"
    assert row["source_transaction_type"] == "ACCPAY"
    assert row["target_transaction_id"] == "invoice-a"
    assert row["target_line_item_id"] == "line-b"
    assert row["updated_at"] == datetime(2026, 9, 29, 10, 30, tzinfo=timezone.utc)


def test_linked_transaction_accepts_xero_date_and_missing_target():
    record = _record({
        "LinkedTransactionID": "link-a",
        "UpdatedDateUTC": "/Date(1780099200000+0000)/",
    }, "link-a")
    row = linked_transactions.parse_header(record)
    assert row["target_transaction_id"] is None
    assert row["updated_at"].tzinfo == timezone.utc


def _template(lines=None):
    return _record({
        "RepeatingInvoiceID": "template-a",
        "ID": "template-a",
        "Type": "ACCREC",
        "Status": "AUTHORISED",
        "Contact": {"ContactID": "contact-a", "Name": "Customer"},
        "Schedule": {
            "Unit": "MONTHLY", "Period": 1, "DueDateType": "DAYSAFTERINVOICEDATE",
            "DueDate": 14, "StartDate": "/Date(1780099200000)/",
            "NextScheduledDate": "/Date(1780099200000)/",
            "NextScheduledDateString": "2026-05-30",
        },
        "LineAmountTypes": "Inclusive",
        "CurrencyCode": "AUD",
        "SubTotal": 90.0,
        "TotalTax": 10.0,
        "Total": 100.0,
        "LineItems": lines if lines is not None else [{
            "LineItemID": "line-a", "AccountID": "account-a",
            "AccountCode": "200", "Description": "Service",
            "Quantity": 1, "UnitAmount": 100.0, "LineAmount": 100.0,
            "TaxAmount": 10.0, "TaxType": "OUTPUT",
            "Tracking": [{"TrackingCategoryID": "category-a", "Option": "North"}],
        }],
    }, "template-a")


def test_repeating_invoice_preserves_schedule_and_line():
    record = _template()
    header = repeating_invoices.parse_header(record)
    lines = repeating_invoices.parse_lines(record)
    assert header["contact_id"] == "contact-a"
    assert header["schedule_period"] == 1
    assert header["next_scheduled_date_local"] == date(2026, 5, 30)
    assert header["line_amount_types"] == "Inclusive"
    assert header["line_item_count"] == 1
    assert lines[0]["line_position"] == 0
    assert lines[0]["line_item_id"] == "line-a"
    assert lines[0]["tracking_category_1_id"] == "category-a"
    assert lines[0]["tracking_option_1_name"] == "North"


def test_repeating_invoice_empty_lines_writes_header_only():
    class Reader:
        def iter_records(self, *args, **kwargs):
            yield _template(lines=[])

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, **kwargs):
            self.calls.append((table, rows, kwargs))

    writer = Writer()
    assert repeating_invoices.run(Reader(), writer) == {"headers": 1, "lines": 0}
    assert [call[0] for call in writer.calls] == ["repeating_invoices"]


def test_repeating_invoice_lines_are_snapshot_keyed():
    class Reader:
        def iter_records(self, *args, **kwargs):
            yield _template()

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, **kwargs):
            self.calls.append((table, rows, kwargs))

    writer = Writer()
    assert repeating_invoices.run(Reader(), writer) == {"headers": 1, "lines": 1}
    assert writer.calls[1][0] == "repeating_invoice_line_snapshots"
    assert writer.calls[1][2]["key_columns"] == (
        "tenant_id", "record_id", "line_position", "synced_at"
    )
