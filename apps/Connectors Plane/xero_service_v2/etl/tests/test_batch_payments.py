"""Batch-payment staging preserves header and payment-line data."""

from datetime import date, datetime, timezone

from etl.xero import batch_payments


def _record(payments=None):
    return {
        "tenant_id": "tenant-a",
        "record_id": "batch-a",
        "first_seen_at": datetime(2026, 9, 1, tzinfo=timezone.utc),
        "last_seen_at": datetime(2026, 9, 29, tzinfo=timezone.utc),
        "payload": {
            "BatchPaymentID": "batch-a",
            "Account": {"AccountID": "account-a", "CurrencyCode": "AUD"},
            "Date": "/Date(1780099200000+0000)/",
            "DateString": "2026-05-30T00:00:00",
            "UpdatedDateUTC": "/Date(1780099200000)/",
            "Type": "PAYBATCH",
            "Status": "AUTHORISED",
            "TotalAmount": 30.0,
            "IsReconciled": True,
            "Details": "Supplier batch",
            "ReconciledStatementLineId": "statement-a",
            "Payments": payments if payments is not None else [
                {
                    "PaymentID": "payment-a",
                    "Amount": 20.0,
                    "BankAmount": 18.0,
                    "Invoice": {"InvoiceID": "invoice-a", "CurrencyCode": "NZD"},
                    "Details": "first",
                },
                {
                    "PaymentID": "payment-b",
                    "Amount": 10.0,
                    "BankAmount": 9.0,
                    "Invoice": {"InvoiceID": "invoice-b", "CurrencyCode": "NZD"},
                },
            ],
        },
    }


def test_batch_payment_header_and_lines_preserve_fx_fields():
    record = _record()
    header = batch_payments.parse_header(record)
    lines = batch_payments.parse_lines(record)

    assert header["batch_payment_id"] == "batch-a"
    assert header["bank_account_id"] == "account-a"
    assert header["account_currency_code"] == "AUD"
    assert header["payment_date_local"] == date(2026, 5, 30)
    assert header["updated_at"].tzinfo == timezone.utc
    assert header["payment_count"] == 2
    assert header["reconciled_statement_line_id"] == "statement-a"
    assert [line["line_index"] for line in lines] == [0, 1]
    assert lines[0]["payment_id"] == "payment-a"
    assert lines[0]["invoice_currency_code"] == "NZD"
    assert lines[0]["bank_amount"] == 18.0
    assert lines[0]["account_currency_code"] == "AUD"


def test_empty_batch_writes_header_only():
    class Reader:
        def iter_records(self, *args, **kwargs):
            yield _record(payments=[])

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, **kwargs):
            self.calls.append((table, rows, kwargs))

    writer = Writer()
    result = batch_payments.run(Reader(), writer)
    assert result == {"headers": 1, "lines": 0}
    assert [call[0] for call in writer.calls] == ["batch_payments"]


def test_batch_line_merge_uses_snapshot_and_position_key():
    class Reader:
        def iter_records(self, *args, **kwargs):
            yield _record()

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, **kwargs):
            self.calls.append((table, rows, kwargs))

    writer = Writer()
    result = batch_payments.run(Reader(), writer)
    assert result == {"headers": 1, "lines": 2}
    assert writer.calls[1][0] == "batch_payment_line_snapshots"
    assert writer.calls[1][2]["key_columns"] == (
        "tenant_id", "record_id", "line_index", "synced_at"
    )
