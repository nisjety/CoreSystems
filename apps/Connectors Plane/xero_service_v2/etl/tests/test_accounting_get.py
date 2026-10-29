import json
from datetime import datetime, timezone

import pytest

from etl.common.gcs_reader import GCSReader
from etl.xero import accounting_get
from etl.xero.run_history import target_dataset as manifest_target_dataset


def test_pinned_catalog_has_126_gets_and_26_binary_documents():
    assert len(accounting_get.OPERATIONS) == 126
    assert sum(accounting_get.OPERATIONS.values()) == 26
    assert accounting_get.operation_info("accounting_get_getinvoicehistory") == (
        "getInvoiceHistory", False)
    assert accounting_get.operation_info("accounting_get_getinvoiceaspdf") == (
        "getInvoiceAsPdf", True)
    with pytest.raises(ValueError):
        accounting_get.operation_info("accounting_get_getInvoiceHistory")
    with pytest.raises(ValueError):
        accounting_get.operation_info("accounting_get_deleteInvoice")


def test_binary_requires_separate_document_bucket_and_dataset(monkeypatch):
    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("GCS_BUCKET", "ordinary-raw")
    monkeypatch.setenv("XERO_DOCUMENT_GCS_BUCKET", "private-documents")
    monkeypatch.setenv("XERO_DOCUMENT_DATASET", "staging_xero_documents")
    assert accounting_get.target_dataset(
        "accounting_get_getinvoicehistory", "ordinary-raw") == (
            "dw-prod", "dw_staging_xero")
    assert accounting_get.target_dataset(
        "accounting_get_getinvoiceaspdf", "private-documents") == (
            "dw-prod", "staging_xero_documents")
    with pytest.raises(ValueError):
        accounting_get.target_dataset(
            "accounting_get_getinvoiceaspdf", "ordinary-raw")
    with pytest.raises(ValueError):
        accounting_get.target_dataset(
            "accounting_get_getinvoicehistory", "private-documents")


def test_binary_sidecar_yields_pointer_without_reading_document():
    endpoint = "accounting_get_getinvoiceaspdf"
    descriptor = json.dumps({"operation_id": "getInvoiceAsPdf", "path": [],
                             "query": [], "headers": []})
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: {
        "x-http-status": "200", "x-endpoint": endpoint,
        "x-api-version": "2.0", "x-tenant-id": "tenant-1",
        "x-synced-at": "2026-10-02T00:00:00Z", "x-run-id": "run-1",
        "x-operation-id": "getInvoiceAsPdf", "x-response-binary": "true",
        "x-response-content-type": "application/pdf",
        "x-request-descriptor": descriptor,
        "x-spec-commit": accounting_get.SPEC_COMMIT,
    }
    reader._read_data = lambda _: pytest.fail("binary body must not be decoded as JSON")
    path = f"raw/xero/tenant-1/2.0/{endpoint}/2026-10-02/file.pdf.meta.json"
    rows = list(reader.iter_records_from_meta(path))
    assert len(rows) == 1
    assert rows[0]["payload"] is None
    assert rows[0]["source_object_path"].endswith("file.pdf")


def test_attachment_binary_can_have_its_actual_file_mime_type():
    endpoint = "accounting_get_getinvoiceattachmentbyid"
    reader = GCSReader.__new__(GCSReader)
    reader._read_meta = lambda _: {
        "x-http-status": "200", "x-endpoint": endpoint,
        "x-api-version": "2.0", "x-tenant-id": "tenant-1",
        "x-synced-at": "2026-10-02T00:00:00Z", "x-run-id": "run-2",
        "x-operation-id": "getInvoiceAttachmentById", "x-response-binary": "true",
        "x-response-content-type": "image/jpeg",
        "x-request-descriptor": json.dumps({"operation_id": "getInvoiceAttachmentById",
                                            "path": [], "query": [], "headers": []}),
        "x-spec-commit": accounting_get.SPEC_COMMIT,
    }
    reader._read_data = lambda _: pytest.fail("binary body must not be decoded")
    path = f"raw/xero/tenant-1/2.0/{endpoint}/2026-10-02/file.bin.meta.json"
    assert list(reader.iter_records_from_meta(path))[0]["payload"] is None


def test_accounting_get_manifest_stays_in_matching_dataset(monkeypatch):
    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("GCS_BUCKET", "ordinary-raw")
    monkeypatch.setenv("XERO_DOCUMENT_GCS_BUCKET", "private-documents")
    monkeypatch.setenv("XERO_DOCUMENT_DATASET", "staging_xero_documents")
    assert manifest_target_dataset(
        "private-documents", ["accounting_get_getinvoiceaspdf"], "dw-prod"
    ) == ("dw-prod", "staging_xero_documents")
    assert manifest_target_dataset(
        "ordinary-raw", ["accounts", "accounting_get_getaccount"], "dw-prod"
    ) == ("dw-prod", "dw_staging_xero")
    with pytest.raises(ValueError):
        manifest_target_dataset("private-documents", ["accounts"], "dw-prod")
    with pytest.raises(ValueError):
        manifest_target_dataset(
            "ordinary-raw", ["accounting_get_getinvoices", "accounting_get_getinvoiceaspdf"],
            "dw-prod",
        )


def test_cloud_function_routes_document_pointer_without_reading_body(monkeypatch):
    from etl.cloud_function import main as cloud_function

    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("GCS_BUCKET", "ordinary-raw")
    monkeypatch.setenv("XERO_DOCUMENT_GCS_BUCKET", "private-documents")
    monkeypatch.setenv("XERO_DOCUMENT_DATASET", "staging_xero_documents")
    monkeypatch.setattr(cloud_function, "GCS_BUCKET", "ordinary-raw")
    monkeypatch.setattr(cloud_function, "PROJECT", "dw-prod")
    calls = []

    class Reader:
        def __init__(self, bucket, project):
            assert (bucket, project) == ("private-documents", "dw-prod")

        def iter_records_from_meta(self, path):
            assert "accounting_get_getinvoiceaspdf" in path
            return [{"tenant_id": "tenant-1", "payload": None,
                     "source_object_path": path.removesuffix(".meta.json"),
                     "synced_at": datetime(2026, 10, 2, tzinfo=timezone.utc),
                     "meta": {"x-run-id": "run-1", "x-operation-id": "getInvoiceAsPdf",
                              "x-spec-commit": accounting_get.SPEC_COMMIT,
                              "x-request-descriptor": json.dumps({
                                  "operation_id": "getInvoiceAsPdf", "path": [],
                                  "query": [], "headers": []}),
                              "x-response-content-type": "application/pdf",
                              "x-response-binary": "true", "x-http-status": "200"}}]

    class Writer:
        def __init__(self, project, dataset):
            assert (project, dataset) == ("dw-prod", "staging_xero_documents")

        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "accounting_get_responses"
            assert rows[0]["payload_json"] is None
            calls.append(rows[0]["raw_object_path"])
            return len(rows)

    monkeypatch.setattr(cloud_function, "GCSReader", Reader)
    monkeypatch.setattr(cloud_function, "BQWriter", Writer)

    class Event:
        data = {"bucket": "private-documents", "name":
                "raw/xero/tenant-1/2.0/accounting_get_getinvoiceaspdf/2026-10-02/a.pdf.meta.json"}

    cloud_function.process_gcs_upload(Event())
    assert calls == [Event.data["name"].removesuffix(".meta.json")]


def test_cloud_function_rejects_event_without_bucket(monkeypatch):
    from etl.cloud_function import main as cloud_function

    monkeypatch.delenv("XERO_DOCUMENT_GCS_BUCKET", raising=False)

    class Event:
        data = {"name": "raw/xero/t/2.0/accounting_get_getinvoices/2026-10-02/a.json.meta.json"}

    with pytest.raises(ValueError, match="unexpected GCS bucket"):
        cloud_function.process_gcs_upload(Event())


def test_json_response_is_landed_as_append_only_warehouse_evidence():
    endpoint = "accounting_get_getinvoicehistory"
    descriptor = json.dumps({"operation_id": "getInvoiceHistory", "path": [],
                             "query": [], "headers": []})
    row = {
        "tenant_id": "tenant-1", "record_id": "run-1",
        "payload": {"HistoryRecords": [{"Details": "created"}]},
        "synced_at": datetime(2026, 10, 2, tzinfo=timezone.utc),
        "source_object_path": "raw/xero/tenant-1/2.0/accounting_get_getinvoicehistory/2026-10-02/a.json",
        "meta": {"x-run-id": "run-1", "x-operation-id": "getInvoiceHistory",
                 "x-request-descriptor": descriptor,
                 "x-spec-commit": accounting_get.SPEC_COMMIT,
                 "x-response-content-type": "application/json",
                 "x-http-status": "200", "x-response-binary": "false"},
    }

    class Reader:
        def iter_records(self, *_args, **_kwargs):
            yield row

    class Writer:
        def insert_if_absent(self, table, rows, key_columns, schema):
            assert table == "accounting_get_responses"
            assert key_columns == (
                "tenant_id", "operation_id", "run_id", "request_descriptor",
            )
            assert json.loads(rows[0]["payload_json"])["HistoryRecords"][0]["Details"] == "created"
            assert rows[0]["raw_object_path"].endswith("a.json")
            return len(rows)

    assert accounting_get.run(Reader(), Writer(), endpoint)["records"] == 1


def test_large_json_keeps_only_the_raw_pointer(monkeypatch):
    monkeypatch.setattr(accounting_get, "_MAX_BQ_JSON_BYTES", 20)
    endpoint = "accounting_get_getinvoicehistory"
    record = {
        "tenant_id": "tenant-1", "payload": {"HistoryRecords": [{"Details": "long value"}]},
        "synced_at": datetime(2026, 10, 2, tzinfo=timezone.utc),
        "source_object_path": "raw/xero/tenant-1/2.0/accounting_get_getinvoicehistory/2026-10-02/a.json",
        "meta": {"x-run-id": "run-1", "x-operation-id": "getInvoiceHistory",
                 "x-spec-commit": accounting_get.SPEC_COMMIT,
                 "x-request-descriptor": json.dumps({
                     "operation_id": "getInvoiceHistory", "path": [],
                     "query": [], "headers": []}),
                 "x-response-content-type": "application/json",
                 "x-http-status": "200", "x-response-binary": "false"},
    }

    class Reader:
        def iter_records(self, *_args):
            yield record

    class Writer:
        def insert_if_absent(self, _table, rows, **_kwargs):
            assert rows[0]["payload_json"] is None
            assert rows[0]["payload_in_bigquery"] is False
            assert rows[0]["raw_object_path"].endswith("a.json")
            return len(rows)

    assert accounting_get.run(Reader(), Writer(), endpoint)["records"] == 1
