from datetime import datetime, timezone

import pytest

from etl.common.gcs_reader import GCSReader
from etl.xero import extended


@pytest.mark.parametrize(
    ("endpoint", "body", "expected_id"),
    [
        ("assets_assets", {"items": [{"assetId": "asset-1"}]}, "asset-1"),
        ("assets_asset_types", [{"assetTypeId": "type-1"}], "type-1"),
        ("projects_projects", {"items": [{"projectId": "project-1"}]}, "project-1"),
        ("projects_tasks", {"items": [{"taskId": "task-1"}]},
         "5f3c9a2e-0000-0000-0000-000000000000:task-1"),
        ("projects_time", {"items": [{"timeEntryId": "time-1"}]},
         "5f3c9a2e-0000-0000-0000-000000000000:time-1"),
        ("bankfeeds_statements", {"items": [{"id": "statement-1"}]}, "statement-1"),
        ("identity_connections", [{"id": "connection-1"}], "connection-1"),
    ],
)
def test_extended_collections_extract_records(endpoint, body, expected_id):
    reader = GCSReader.__new__(GCSReader)
    now = datetime(2026, 9, 30, tzinfo=timezone.utc)
    meta = {"x-request-key": "5f3c9a2e-0000-0000-0000-000000000000"} if endpoint in {
        "projects_tasks", "projects_time",
    } else {}
    rows = list(reader._extract_records(body, endpoint, "tenant-1", now, meta))
    assert len(rows) == 1
    assert rows[0]["record_id"] == expected_id


def test_extended_finance_snapshot_gets_stable_daily_id():
    reader = GCSReader.__new__(GCSReader)
    now = datetime(2026, 9, 30, tzinfo=timezone.utc)
    meta = {"x-api-family": "finance", "x-snapshot-date": "2026-09-30"}
    rows = list(reader._extract_records(
        {"balanceDate": "2026-09-30", "asset": {}},
        "finance_balance_sheet", "tenant-1", now, meta,
    ))
    assert len(rows) == 1
    assert rows[0]["record_id"] == "2026-09-30"


def test_extended_parser_preserves_complete_payload_as_json():
    row = {"tenant_id": "tenant-1", "record_id": "asset-1",
           "payload": {"assetId": "asset-1", "nested": {"amount": 4}},
           "synced_at": datetime(2026, 9, 30, tzinfo=timezone.utc),
           "meta": {"x-endpoint": "assets_assets", "x-api-family": "assets"}}

    class Reader:
        def iter_records(self, *_args, **_kwargs):
            yield row

    class Writer:
        def merge(self, table, rows, key_columns, schema):
            assert table == "extended_records"
            assert key_columns == ("tenant_id", "endpoint", "record_id")
            assert rows[0]["payload_json"] == '{"assetId":"asset-1","nested":{"amount":4}}'
            assert next(field for field in schema if field.name == "snapshot_date").field_type == "STRING"
            return len(rows)

    assert extended.run(Reader(), Writer(), endpoint="assets_assets")["records"] == 1


@pytest.mark.parametrize(
    ("endpoint", "version"),
    [("assets_assets", "1.0"), ("projects_projects", "2.0"),
     ("bankfeeds_statements", "1.0"), ("finance_balance_sheet", "1.0"),
     ("identity_connections", "identity"), ("accounts", "2.0")],
)
def test_gcs_reader_scans_the_correct_api_version(endpoint, version):
    reader = GCSReader.__new__(GCSReader)

    class Client:
        def list_blobs(self, _bucket, prefix):
            assert prefix == f"raw/xero/tenant-1/{version}/{endpoint}/"
            return []

    reader._client = Client()
    reader.bucket_name = "bucket"
    assert list(reader.iter_records("xero", endpoint, tenant_id="tenant-1")) == []


def test_extended_target_requires_explicit_project_and_restricted_dataset(monkeypatch):
    monkeypatch.delenv("GCP_PROJECT", raising=False)
    monkeypatch.delenv("XERO_SENSITIVE_DATASET", raising=False)
    with pytest.raises(ValueError, match="GCP_PROJECT"):
        extended.warehouse_target("assets_assets", bucket="ordinary-raw")
    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    assert extended.warehouse_target("assets_assets", bucket="ordinary-raw") == (
        "dw-prod", "dw_staging_xero")
    monkeypatch.setenv("XERO_SENSITIVE_GCS_BUCKET", "restricted-raw")
    with pytest.raises(ValueError, match="XERO_SENSITIVE_DATASET"):
        extended.warehouse_target("finance_balance_sheet", bucket="restricted-raw")
    monkeypatch.setenv("XERO_SENSITIVE_DATASET", "staging_xero_sensitive")
    assert extended.warehouse_target("finance_balance_sheet", bucket="restricted-raw") == (
        "dw-prod", "staging_xero_sensitive")


def test_sidecar_cannot_attribute_another_tenant():
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: {
        "x-http-status": "200", "x-endpoint": "assets_assets",
        "x-tenant-id": "other-tenant", "x-synced-at": "2026-09-30T00:00:00Z",
    }
    reader._read_data = lambda _: {"items": [{"assetId": "asset-1"}]}
    path = "raw/xero/tenant-1/1.0/assets_assets/2026-09-30/file.json.meta.json"
    with pytest.raises(ValueError, match="tenant"):
        list(reader.iter_records_from_meta(path))
