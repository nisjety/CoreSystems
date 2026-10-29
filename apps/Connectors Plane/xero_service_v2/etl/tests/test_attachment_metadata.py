"""Attachment-list ETL contract and Cloud Function routing."""

from datetime import datetime, timezone
from pathlib import Path

import pytest

from etl.common.gcs_reader import GCSReader


META_PATH = (
    "raw/xero/tenant-1/2.0/attachment_metadata/2026-10-01/"
    "20261001_run_1.json.meta.json"
)
SYNCED_AT = "2026-10-01T10:00:00Z"
ATTACHMENT_ID = "aeb4f270-0000-0000-0000-000000000000"
PARENT_ID = "5f3c9a2e-0000-0000-0000-000000000000"
URL = f"https://api.xero.com/api.xro/2.0/Invoices/{PARENT_ID}/Attachments/{ATTACHMENT_ID}"


def _meta(**overrides):
    return {
        "x-http-status": "200",
        "x-tenant-id": "tenant-1",
        "x-endpoint": "attachment_metadata",
        "x-api-version": "2.0",
        "x-synced-at": SYNCED_AT,
        "x-run-id": "run-1",
        "x-parent-endpoint": "invoices",
        "x-parent-id": PARENT_ID,
    } | overrides


def _body(**overrides):
    return {"Attachments": [{
        "AttachmentID": ATTACHMENT_ID,
        "FileName": "invoice.pdf",
        "MimeType": "application/pdf",
        "ContentLength": "1234",
        "Url": URL,
        "IncludeOnline": False,
        "Content": "never-store-binary-content",
    } | overrides]}


def _reader(meta=None, body=None):
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: _meta() if meta is None else meta
    reader._read_data = lambda _: _body() if body is None else body
    return reader


def test_reader_extracts_original_attachment_objects_with_parent_context():
    rows = list(_reader().iter_records_from_meta(META_PATH))
    assert len(rows) == 1
    assert rows[0]["record_id"] == PARENT_ID
    assert rows[0]["payload"]["Attachments"][0]["FileName"] == "invoice.pdf"
    assert rows[0]["meta"]["x-parent-id"] == PARENT_ID
    assert rows[0]["source_object_path"] == META_PATH


@pytest.mark.parametrize("field,value", [
    ("x-parent-endpoint", None),
    ("x-parent-endpoint", "../invoices"),
    ("x-parent-endpoint", "attachment_metadata"),
    ("x-parent-endpoint", "finance_balance_sheet"),
    ("x-parent-id", None),
    ("x-parent-id", "../escape"),
    ("x-parent-id", "not-a-uuid"),
])
def test_reader_rejects_invalid_parent_before_reading_body(field, value):
    meta = _meta()
    meta[field] = value
    reader = _reader(meta=meta)
    reader._read_data = lambda _: pytest.fail("body read before parent validation")
    with pytest.raises(ValueError, match="parent"):
        list(reader.iter_records_from_meta(META_PATH))


@pytest.mark.parametrize("body", [
    {},
    {"Attachments": {}},
    {"Attachments": ["not-an-object"]},
    {"Attachments": [{"FileName": "missing-id"}]},
])
def test_reader_rejects_invalid_attachment_body(body):
    with pytest.raises(ValueError, match="Attachment"):
        list(_reader(body=body).iter_records_from_meta(META_PATH))


@pytest.mark.parametrize("overrides", [
    {"x-http-status": "206"},
    {"x-synced-at": "not-a-timestamp"},
    {"x-synced-at": "2026-10-01T10:00:00"},
])
def test_reader_does_not_treat_incomplete_or_undated_list_as_snapshot(overrides):
    with pytest.raises(ValueError):
        list(_reader(meta=_meta(**overrides), body={"Attachments": []}).iter_records_from_meta(META_PATH))


def test_parser_writes_typed_metadata_only():
    from etl.xero import attachment_metadata

    record = next(_reader().iter_records_from_meta(META_PATH))

    class Reader:
        def iter_records(self, endpoint, **kwargs):
            assert endpoint == "attachment_metadata"
            yield record

    class Writer:
        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "attachment_list_snapshots"
            assert key_columns == ("tenant_id", "snapshot_id")
            assert rows[0]["attachment_ids"] == [ATTACHMENT_ID]
            assert rows[0]["attachment_count"] == 1
            assert rows[0]["run_id"] == "run-1"
            assert {field.name for field in schema} == set(rows[0])
            assert next(field.mode for field in schema if field.name == "attachment_ids") == "REPEATED"
            assert "never-store-binary-content" not in str(rows)
            return len(rows)

        def merge(self, table, rows, key_columns, schema):
            assert table == "attachment_metadata"
            assert key_columns == (
                "tenant_id", "attachment_id", "parent_endpoint", "parent_id",
            )
            assert {field.name for field in schema} == set(rows[0])
            assert {field.name: field.field_type for field in schema}["content_length"] == "INTEGER"
            assert {field.name: field.field_type for field in schema}["synced_at"] == "TIMESTAMP"
            assert rows[0] == {
                "tenant_id": "tenant-1",
                "record_id": ATTACHMENT_ID,
                "attachment_id": ATTACHMENT_ID,
                "parent_endpoint": "invoices",
                "parent_id": PARENT_ID,
                "filename": "invoice.pdf",
                "mime_type": "application/pdf",
                "content_length": 1234,
                "include_online": False,
                "url": URL,
                "synced_at": datetime(2026, 10, 1, 10, tzinfo=timezone.utc),
            }
            assert "never-store-binary-content" not in str(rows)
            return len(rows)

    assert attachment_metadata.run(Reader(), Writer()) == {"records": 1, "snapshots": 1}


@pytest.mark.parametrize("overrides", [
    {"ContentLength": -1},
    {"ContentLength": "not-a-count"},
    {"IncludeOnline": "false"},
    {"Url": "https://example.com/secret?token=abc"},
])
def test_parser_rejects_invalid_fields_without_writing(overrides):
    from etl.xero import attachment_metadata

    record = next(_reader(body=_body(**overrides)).iter_records_from_meta(META_PATH))

    class Reader:
        def iter_records(self, endpoint, **kwargs):
            yield record

    class Writer:
        def merge(self, *args, **kwargs):
            pytest.fail("invalid attachment was written")

        def insert_if_absent(self, *args, **kwargs):
            pytest.fail("invalid snapshot was written")

    with pytest.raises(ValueError):
        attachment_metadata.run(Reader(), Writer())


def test_parser_rejects_later_invalid_record_without_partial_write():
    from etl.xero import attachment_metadata

    first = next(_reader().iter_records_from_meta(META_PATH))
    second = next(_reader(body=_body(ContentLength=2**63)).iter_records_from_meta(META_PATH))

    class Reader:
        def iter_records(self, endpoint, **kwargs):
            yield first
            yield second

    class Writer:
        def merge(self, *args, **kwargs):
            pytest.fail("partial attachment batch was written")

        def insert_if_absent(self, *args, **kwargs):
            pytest.fail("partial snapshot batch was written")

    with pytest.raises(ValueError, match="ContentLength"):
        attachment_metadata.run(Reader(), Writer())


def test_empty_attachment_list_writes_complete_snapshot():
    from etl.xero import attachment_metadata

    records = list(_reader(body={"Attachments": []}).iter_records_from_meta(META_PATH))
    assert len(records) == 1

    class Reader:
        def iter_records(self, endpoint, **kwargs):
            return iter(records)

    class Writer:
        def merge(self, *args, **kwargs):
            pytest.fail("empty attachment batch was written")

        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "attachment_list_snapshots"
            assert rows[0]["attachment_ids"] == []
            assert rows[0]["attachment_count"] == 0
            return 1

    assert attachment_metadata.run(Reader(), Writer()) == {"records": 0, "snapshots": 1}


def test_snapshot_identity_is_stable_for_replay_and_distinct_per_file():
    from etl.xero.attachment_metadata import parse_snapshot

    record = next(_reader().iter_records_from_meta(META_PATH))
    _, first = parse_snapshot(record)
    _, replay = parse_snapshot(record)
    _, another = parse_snapshot(record | {"source_object_path": META_PATH.replace("run_1", "run_2")})
    assert first == replay
    assert first["snapshot_id"] != another["snapshot_id"]


@pytest.mark.parametrize("body", [
    {"Attachments": [{"AttachmentID": ATTACHMENT_ID}, {"AttachmentID": ATTACHMENT_ID}]},
    {"Attachments": [{"AttachmentID": ATTACHMENT_ID}, {"AttachmentID": "not-a-uuid"}]},
])
def test_invalid_lists_never_write_partial_snapshots(body):
    with pytest.raises(ValueError, match="Attachment"):
        list(_reader(body=body).iter_records_from_meta(META_PATH))


def test_silver_model_uses_latest_complete_parent_snapshot():
    path = Path(__file__).resolve().parents[2] / "Dataform/definitions/silver/xero/dimentinals/dim_attachment.sqlx"
    sql = path.read_text()
    assert 'ref("dw_staging_xero", "attachment_list_snapshots")' in sql
    assert 'ref("dw_staging_xero", "attachment_metadata")' in sql
    assert "PARTITION BY tenant_id, parent_endpoint, parent_id" in sql
    assert "UNNEST(s.attachment_ids)" in sql
    assert "m.attachment_id = listed_attachment_id" in sql
    assert "PARTITION BY tenant_id, parent_endpoint, parent_id, attachment_id" in sql


def test_cloud_function_rejects_missing_parent_before_writer(monkeypatch):
    from etl.cloud_function import main as cloud_function

    monkeypatch.setattr(cloud_function, "GCS_BUCKET", "raw-bucket")
    monkeypatch.setattr(cloud_function, "PROJECT", "dw-project")
    monkeypatch.setattr(
        cloud_function, "GCSReader",
        lambda **kwargs: _reader(meta=_meta(**{"x-parent-id": ""})),
    )
    monkeypatch.setattr(
        cloud_function, "BQWriter",
        lambda **kwargs: pytest.fail("writer constructed before parent validation"),
    )

    class Event:
        data = {"bucket": "raw-bucket", "name": META_PATH}

    with pytest.raises(ValueError, match="parent"):
        cloud_function.process_gcs_upload(Event())


def test_cloud_function_routes_attachment_metadata(monkeypatch):
    from etl.cloud_function import main as cloud_function

    monkeypatch.setattr(cloud_function, "GCS_BUCKET", "raw-bucket")
    monkeypatch.setattr(cloud_function, "PROJECT", "dw-project")
    monkeypatch.setattr(cloud_function, "GCSReader", lambda **kwargs: _reader())
    calls = []

    class Writer:
        def __init__(self, project, dataset):
            assert (project, dataset) == ("dw-project", "dw_staging_xero")

        def merge(self, table, rows, key_columns, schema):
            calls.append((table, rows[0]["parent_id"]))
            return len(rows)

        def insert_if_absent(self, table, rows, key_columns, schema):
            calls.append((table, rows[0]["parent_id"]))
            return len(rows)

    monkeypatch.setattr(cloud_function, "BQWriter", Writer)

    class Event:
        data = {"bucket": "raw-bucket", "name": META_PATH}

    cloud_function.process_gcs_upload(Event())
    assert calls == [
        ("attachment_metadata", PARENT_ID),
        ("attachment_list_snapshots", PARENT_ID),
    ]


def test_cloud_function_routes_empty_attachment_list(monkeypatch):
    from etl.cloud_function import main as cloud_function

    monkeypatch.setattr(cloud_function, "GCS_BUCKET", "raw-bucket")
    monkeypatch.setattr(cloud_function, "PROJECT", "dw-project")
    monkeypatch.setattr(
        cloud_function, "GCSReader", lambda **kwargs: _reader(body={"Attachments": []}),
    )
    calls = []

    class Writer:
        def __init__(self, project, dataset):
            assert (project, dataset) == ("dw-project", "dw_staging_xero")

        def merge(self, *args, **kwargs):
            pytest.fail("empty list must not write metadata")

        def insert_if_absent(self, table, rows, key_columns, schema):
            calls.append((table, rows[0]["attachment_ids"]))
            return 1

    monkeypatch.setattr(cloud_function, "BQWriter", Writer)

    class Event:
        data = {"bucket": "raw-bucket", "name": META_PATH}

    cloud_function.process_gcs_upload(Event())
    assert calls == [("attachment_list_snapshots", [])]


def test_backfill_replays_empty_attachment_list(monkeypatch):
    from etl import backfill_gcs

    class Reader:
        def iter_records(self, vendor, endpoint, tenant_id):
            assert (vendor, endpoint, tenant_id) == ("xero", "attachment_metadata", "tenant-1")
            yield from _reader(body={"Attachments": []}).iter_records_from_meta(META_PATH)

    class Writer:
        def merge(self, *args, **kwargs):
            pytest.fail("empty list must not write metadata")

        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "attachment_list_snapshots"
            assert rows[0]["attachment_ids"] == []
            return 1

    monkeypatch.setattr(backfill_gcs, "_reader_writer_for_this_thread", lambda *args: (Reader(), Writer()))
    assert backfill_gcs.backfill_tenant_endpoint("tenant-1", "attachment_metadata") == {
        "records": 0, "snapshots": 1,
    }


def test_backfill_registry_can_replay_attachment_metadata():
    from etl.backfill_gcs import PARSERS
    from etl.xero import attachment_metadata

    assert PARSERS["attachment_metadata"] is attachment_metadata
