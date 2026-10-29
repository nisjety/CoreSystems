"""Complete extended-list snapshots, including empty collections."""

from datetime import datetime, timezone

import pytest

from etl.common.gcs_reader import GCSReader


PATH = (
    "raw/xero/tenant-1/1.0/assets_list_snapshots/2026-10-01/"
    "20261001T100000Z_abc123_p001.json.meta.json"
)
META = {
    "x-http-status": "200",
    "x-tenant-id": "tenant-1",
    "x-endpoint": "assets_list_snapshots",
    "x-api-version": "1.0",
    "x-api-family": "assets",
    "x-source-endpoint": "assets_assets",
    "x-request-key": "REGISTERED",
    "x-record-count": "2",
    "x-run-id": "5f3c9a2e-0000-0000-0000-000000000000",
    "x-synced-at": "2026-10-01T10:00:00Z",
}
BODY = {
    "source_endpoint": "assets_assets",
    "request_key": "REGISTERED",
    "record_ids": ["asset-1", "asset-2"],
    "record_count": 2,
}


def _reader(meta=None, body=None):
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: META if meta is None else meta
    reader._read_data = lambda _: BODY if body is None else body
    return reader


def test_reader_yields_complete_snapshot_not_individual_ids():
    records = list(_reader().iter_records_from_meta(PATH))
    assert len(records) == 1
    assert records[0]["payload"] == BODY
    assert records[0]["source_object_path"] == PATH


def test_empty_list_is_preserved_and_inserted():
    from etl.xero import extended_membership

    meta = META | {"x-record-count": "0"}
    body = BODY | {"record_ids": [], "record_count": 0}
    record = next(_reader(meta, body).iter_records_from_meta(PATH))

    class Reader:
        def iter_records(self, table, **kwargs):
            yield record

    class Writer:
        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "extended_list_snapshots"
            assert key_columns == ("tenant_id", "snapshot_id")
            assert rows[0]["record_ids"] == []
            assert rows[0]["record_count"] == 0
            assert rows[0]["source_endpoint"] == "assets_assets"
            assert rows[0]["request_key"] == "REGISTERED"
            assert rows[0]["synced_at"] == datetime(2026, 10, 1, 10, tzinfo=timezone.utc)
            return 1

    assert extended_membership.run(Reader(), Writer()) == {"snapshots": 1}


@pytest.mark.parametrize("meta,body", [
    (META | {"x-http-status": "206"}, BODY),
    (META | {"x-run-id": "broken"}, BODY),
    (META | {"x-source-endpoint": "finance_cashflow"}, BODY),
    (META | {"x-endpoint": "finance_list_snapshots"}, BODY),
    (META | {"x-api-family": "bankfeeds"}, BODY),
    (META | {"x-request-key": "DRAFT"}, BODY),
    (META | {"x-record-count": "1"}, BODY),
    (META, BODY | {"record_ids": ["asset-1", "asset-1"]}),
    (META, BODY | {"record_count": 1}),
    (META, BODY | {"record_ids": [""]}),
    (META | {"x-source-endpoint": "assets_asset_types", "x-request-key": "unexpected"},
     BODY | {"source_endpoint": "assets_asset_types", "request_key": "unexpected"}),
    (META | {"x-source-endpoint": "projects_tasks", "x-api-family": "projects",
             "x-endpoint": "projects_list_snapshots", "x-request-key": "broken"},
     BODY | {"source_endpoint": "projects_tasks", "request_key": "broken"}),
])
def test_invalid_membership_is_rejected(meta, body):
    from etl.xero.extended_membership import parse_snapshot

    record = next(_reader().iter_records_from_meta(PATH))
    record = record | {"meta": meta, "payload": body}
    with pytest.raises(ValueError):
        parse_snapshot(record)


def test_sensitive_list_snapshot_requires_restricted_bucket(monkeypatch):
    from etl.xero.extended import warehouse_target

    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("XERO_SENSITIVE_DATASET", "staging_xero_sensitive")
    monkeypatch.setenv("XERO_SENSITIVE_GCS_BUCKET", "restricted-raw")
    with pytest.raises(ValueError, match="restricted bucket"):
        warehouse_target("bankfeeds_list_snapshots", bucket="ordinary-raw")
    assert warehouse_target("bankfeeds_list_snapshots", bucket="restricted-raw") == (
        "dw-prod", "staging_xero_sensitive",
    )
    with pytest.raises(ValueError, match="ordinary Xero data"):
        warehouse_target("assets_list_snapshots", bucket="restricted-raw")
    with pytest.raises(ValueError, match="GCS bucket"):
        warehouse_target("assets_list_snapshots", bucket="")
