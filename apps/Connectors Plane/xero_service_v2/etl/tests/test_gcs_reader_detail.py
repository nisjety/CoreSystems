from datetime import datetime, timezone

from unittest import TestCase

from etl.common.gcs_reader import GCSReader


def test_detail_response_with_single_object_is_one_record():
    reader = GCSReader.__new__(GCSReader)
    synced_at = datetime(2026, 1, 1, tzinfo=timezone.utc)
    rows = list(reader._extract_records(
        {"Budgets": {"BudgetID": "budget-1", "BudgetLines": [{"AccountID": "account-1"}]}},
        "budgets", "tenant-1", synced_at,
    ))
    assert len(rows) == 1
    assert rows[0]["record_id"] == "budget-1"
    assert rows[0]["payload"]["BudgetLines"][0]["AccountID"] == "account-1"


def test_bad_gcs_file_fails_batch_job():
    reader = GCSReader.__new__(GCSReader)

    def broken(_):
        raise ValueError("invalid JSON")

    reader.iter_records_from_meta = broken
    with TestCase().assertRaisesRegex(ValueError, "invalid JSON"):
        reader._fetch_records("bad.json.meta.json")


def test_historical_not_modified_file_skips_empty_body():
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: {"x-http-status": "304"}

    def should_not_read(_):
        raise AssertionError("304 response has no JSON body")

    reader._read_data = should_not_read
    assert list(reader.iter_records_from_meta("not-modified.json.meta.json")) == []


def test_generation_prefixed_raw_object_keeps_the_current_typed_projection_compatible():
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: {
        "x-http-status": "200",
        "x-tenant-id": "tenant-1",
        "x-endpoint": "payments",
        "x-api-version": "2.0",
        "x-synced-at": "2026-10-16T10:00:00Z",
    }
    reader._read_data = lambda _: {"Payments": [{"PaymentID": "payment-1"}]}

    rows = list(reader.iter_records_from_meta(
        "xero/550e8400-e29b-41d4-a716-446655440000/"
        "raw/xero/tenant-1/2.0/payments/2026-10-16/page.json.meta.json"
    ))

    assert [(row["tenant_id"], row["record_id"]) for row in rows] == [
        ("tenant-1", "payment-1"),
    ]
