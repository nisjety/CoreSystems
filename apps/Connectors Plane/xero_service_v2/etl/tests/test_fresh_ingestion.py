"""Generation admission must run before any historical payload is read."""

from datetime import datetime, timezone

import pytest

from etl.xero.fresh_ingestion import FreshConfig, admit, PinnedReader


GEN = "132c7f06-47c9-4c2c-b7df-8cd09f16b555"


def config():
    return FreshConfig(GEN, datetime(2026, 10, 6, tzinfo=timezone.utc))


def meta(**changes):
    return {"x-source-system": "xero_v2", "x-ingestion-generation": GEN,
            "x-synced-at": "2026-10-06T12:00:00Z", "x-tenant-id": "tenant",
            "x-endpoint": "payments", "x-api-version": "2.0",
            "x-data-generation": "123", "x-data-crc32c": "AAAAAA==", **changes}


def test_generation_and_cutoff_are_required_together(monkeypatch):
    monkeypatch.delenv("XERO_INGESTION_GENERATION", raising=False)
    monkeypatch.delenv("XERO_FRESH_START_AT", raising=False)
    assert FreshConfig.from_env() is None
    monkeypatch.setenv("XERO_INGESTION_GENERATION", GEN)
    with pytest.raises(ValueError, match="start"):
        FreshConfig.from_env()
    monkeypatch.setenv("XERO_FRESH_START_AT", "2026-10-06T00:00:00Z")
    assert FreshConfig.from_env() == config()
    monkeypatch.setenv("XERO_INGESTION_GENERATION", "bad")
    with pytest.raises(ValueError, match="generation"):
        FreshConfig.from_env()


def test_main_namespace_admits_only_natural_xero_source(monkeypatch):
    monkeypatch.setenv("XERO_INGESTION_GENERATION", GEN)
    monkeypatch.setenv("XERO_FRESH_START_AT", "2026-10-06T00:00:00Z")
    monkeypatch.setenv("XERO_INGESTION_NAMESPACE", "main")
    active = FreshConfig.from_env()
    assert active.namespace == "main"
    assert admit(active, meta(**{"x-source-system": "xero"}))
    assert not admit(active, meta())


@pytest.mark.parametrize("change", [
    {"x-source-system": "v1"}, {"x-ingestion-generation": "other"},
    {"x-synced-at": "2026-10-05T23:59:59Z"},
])
def test_legacy_wrong_generation_and_old_source_are_not_admitted(change):
    assert not admit(config(), meta(**change))


@pytest.mark.parametrize("change", [
    {"x-data-generation": None}, {"x-data-generation": "0"},
    {"x-data-generation": "not-a-generation"}, {"x-data-crc32c": None},
    {"x-data-crc32c": "not-a-checksum"},
])
def test_incomplete_legacy_sidecar_is_not_admitted(change):
    assert not admit(config(), meta(**change))


def test_invalid_source_time_fails_closed():
    with pytest.raises(ValueError, match="timestamp"):
        admit(config(), meta(**{"x-synced-at": "bad"}))
    with pytest.raises(ValueError, match="timezone"):
        admit(config(), meta(**{"x-synced-at": "2026-10-06T12:00:00"}))
    assert admit(config(), meta())


def test_reader_requires_positive_pinned_meta_generation():
    for generation in (None, "", "0", "-1", "1;DROP"):
        with pytest.raises(ValueError, match="generation"):
            PinnedReader("bucket", "project", "file.meta.json", generation)


def test_reader_downloads_pinned_sidecar_and_data_and_checks_crc(monkeypatch):
    from unittest.mock import MagicMock
    import base64
    import google_crc32c
    body = b'{"Invoices": []}'
    checksum = base64.b64encode(google_crc32c.Checksum(body).digest()).decode()
    sidecar = {**meta(), "x-data-generation": "123", "x-data-crc32c": checksum}
    bucket = MagicMock()
    blob = bucket.blob.return_value
    blob.download_as_text.return_value = __import__("json").dumps(sidecar)
    blob.download_as_bytes.return_value = body
    bucket.get_blob.return_value.generation = 123
    bucket.get_blob.return_value.crc32c = checksum
    client = MagicMock()
    client.bucket.return_value = bucket
    monkeypatch.setattr("etl.common.gcs_reader.storage.Client", lambda **_: client)
    reader = PinnedReader("bucket", "project", "file.meta.json", "456")
    assert reader._read_meta("file.meta.json") == sidecar
    reader.verify_data_identity(sidecar)
    assert reader._read_data("file") == {"Invoices": []}
    assert bucket.blob.call_args_list[0].kwargs["generation"] == 456
    assert bucket.blob.call_args_list[-1].kwargs["generation"] == 123
    blob.download_as_bytes.return_value = b'{}'
    with pytest.raises(ValueError, match="checksum"):
        reader._read_data("file")


def test_reader_rejects_mismatched_data_generation_or_missing_checksum(monkeypatch):
    from unittest.mock import MagicMock
    client = MagicMock()
    monkeypatch.setattr("etl.common.gcs_reader.storage.Client", lambda **_: client)
    reader = PinnedReader("bucket", "project", "file.meta.json", "456")
    with pytest.raises(ValueError, match="checksum"):
        reader.verify_data_identity({"x-data-generation": "123"})
    client.bucket.return_value.get_blob.return_value.generation = 999
    with pytest.raises(ValueError, match="identity"):
        reader.verify_data_identity({"x-data-generation": "123", "x-data-crc32c": "AAAAAA=="})


def test_fresh_namespace_does_not_read_legacy_payloads(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    reader = MagicMock()
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", reader)
    process_fresh(config(), "bucket", "raw/xero/old/invoices/file.meta.json", "123", "project")
    reader.assert_not_called()


def test_active_reader_maps_logical_paths_to_pinned_namespace(monkeypatch):
    from unittest.mock import MagicMock
    client = MagicMock()
    monkeypatch.setattr("etl.common.gcs_reader.storage.Client", lambda **_: client)
    prefix = f"v2/{GEN}/"
    logical = "raw/xero/tenant/2.0/invoices/2026-10-06/file.json.meta.json"
    reader = PinnedReader("bucket", "project", prefix + logical, "456")
    client.bucket.return_value.blob.return_value.download_as_text.return_value = __import__("json").dumps(meta())
    reader._read_meta(logical)
    assert client.bucket.return_value.blob.call_args.args[0] == prefix + logical


def test_fresh_response_requires_declared_collection_not_silent_zero(monkeypatch):
    from unittest.mock import MagicMock
    monkeypatch.setattr("etl.common.gcs_reader.storage.Client", lambda **_: MagicMock())
    reader = PinnedReader("bucket", "project", "file.meta.json", "456")
    with pytest.raises(ValueError, match="declared collection"):
        list(reader._extract_records({}, "invoices", "tenant", config().started_at))


def test_cloud_handler_fresh_route_never_constructs_legacy_writer(monkeypatch):
    from types import SimpleNamespace
    from unittest.mock import MagicMock
    from etl.cloud_function import main
    monkeypatch.setenv("XERO_INGESTION_GENERATION", GEN)
    monkeypatch.setenv("XERO_FRESH_START_AT", "2026-10-06T00:00:00Z")
    fresh_handler, legacy_writer = MagicMock(), MagicMock()
    monkeypatch.setattr(main, "process_fresh", fresh_handler)
    monkeypatch.setattr(main, "BQWriter", legacy_writer)
    main.process_gcs_upload(SimpleNamespace(data={"message": {"attributes": {
        "bucketId": main.GCS_BUCKET, "objectId": f"v2/{GEN}/file.meta.json",
        "objectGeneration": "123", "eventType": "OBJECT_FINALIZE",
    }}}))
    assert fresh_handler.call_args.args[3] == "123"
    legacy_writer.assert_not_called()


def test_pinned_reader_reuses_an_explicit_storage_client():
    from unittest.mock import MagicMock

    client = MagicMock()
    reader = PinnedReader("bucket", "project", "file.meta.json", "456", client)

    assert reader._client is client
    client.bucket.assert_called_once_with("bucket")


def test_admitted_main_raw_path_must_bind_tenant_endpoint_and_api_version(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import prepare_fresh_observation

    monkeypatch.setenv("GCS_BUCKET", "bucket")
    monkeypatch.setenv("XERO_STAGING_LAYOUT", "warehouse")
    reader = MagicMock()
    reader._read_meta.return_value = meta(**{
        "x-source-system": "xero",
        "x-tenant-id": "wrong-tenant",
        "x-endpoint": "payments",
        "x-api-version": "2.0",
    })
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *_: reader)
    main_config = FreshConfig(GEN, config().started_at, "main")
    path = f"xero/{GEN}/raw/xero/tenant/2.0/payments/2026-10-06/file.json.meta.json"

    with pytest.raises(ValueError, match="tenant"):
        prepare_fresh_observation(main_config, "bucket", path, "123", "project")

    reader.verify_data_identity.assert_not_called()


def test_admitted_page_publishes_only_fresh_batches(monkeypatch):
    monkeypatch.setenv("GCS_BUCKET", "bucket")
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    import etl.common.fresh_writer as writer_module
    import etl.xero.fresh_staging as staging_module
    reader = MagicMock()
    reader._read_meta.return_value = meta()
    reader.iter_records_from_meta.return_value = [{"payload": {"PaymentID": "id"}}]
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    batches = {"v2_record_versions": [{"record_id": "id"}], "v2_child_versions": []}
    builder = MagicMock(return_value=batches)
    monkeypatch.setattr(staging_module, "build_observation", builder)
    writer = MagicMock()
    monkeypatch.setattr(writer_module, "FreshBQWriter", lambda *args: writer)
    path = f"v2/{GEN}/raw/xero/tenant/2.0/payments/2026-10-06/file.json.meta.json"
    process_fresh(config(), "bucket", path, "123", "project")
    reader.verify_data_identity.assert_called_once()
    writer.publish.assert_called_once_with(batches)
    assert builder.call_args.args[-1] == "123"


def test_endpoint_layout_routes_fresh_page_to_natural_staging_tables(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    import etl.common.fresh_writer as writer_module
    import etl.xero.fresh_staging as staging_module

    monkeypatch.setenv("GCS_BUCKET", "bucket")
    monkeypatch.setenv("XERO_STAGING_DATASET", "dw_staging_xero_main")
    monkeypatch.setenv("XERO_STAGING_LAYOUT", "endpoint")
    reader = MagicMock()
    reader._read_meta.return_value = meta(**{"x-source-system": "xero"})
    reader.iter_records_from_meta.return_value = [{"payload": {"PaymentID": "id"}}]
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    batches = {"v2_record_versions": [{"endpoint": "payments"}], "v2_child_versions": []}
    monkeypatch.setattr(staging_module, "build_observation", lambda *args: batches)
    endpoint_writer, old_writer = MagicMock(), MagicMock()
    monkeypatch.setattr(writer_module, "EndpointBQWriter", lambda *args: endpoint_writer)
    monkeypatch.setattr(writer_module, "FreshBQWriter", lambda *args: old_writer)

    path = f"xero/{GEN}/raw/xero/tenant/2.0/payments/2026-10-06/file.json.meta.json"
    main_config = FreshConfig(GEN, config().started_at, "main")
    process_fresh(main_config, "bucket", path, "123", "project")

    endpoint_writer.publish_observation.assert_called_once_with(batches)
    old_writer.publish.assert_not_called()


def test_warehouse_layout_routes_main_generation_to_current_raw_history_contract(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    import etl.common.fresh_writer as writer_module
    import etl.xero.fresh_staging as staging_module

    monkeypatch.setenv("GCS_BUCKET", "bucket")
    monkeypatch.setenv("XERO_STAGING_DATASET", "dw_staging_xero")
    monkeypatch.setenv("XERO_STAGING_LAYOUT", "warehouse")
    reader = MagicMock()
    reader._read_meta.return_value = meta(**{"x-source-system": "xero"})
    reader.iter_records_from_meta.return_value = [{"payload": {"PaymentID": "id"}}]
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    batches = {"v2_record_versions": [{"endpoint": "payments"}], "v2_child_versions": []}
    monkeypatch.setattr(staging_module, "build_observation", lambda *args: batches)
    raw_writer, endpoint_writer, old_writer = MagicMock(), MagicMock(), MagicMock()
    monkeypatch.setattr(writer_module, "WarehouseRawBQWriter", lambda *args: raw_writer)
    monkeypatch.setattr(writer_module, "EndpointBQWriter", lambda *args: endpoint_writer)
    monkeypatch.setattr(writer_module, "FreshBQWriter", lambda *args: old_writer)

    path = f"xero/{GEN}/raw/xero/tenant/2.0/payments/2026-10-06/file.json.meta.json"
    main_config = FreshConfig(GEN, config().started_at, "main")
    process_fresh(main_config, "bucket", path, "123", "project")

    raw_writer.publish_observation.assert_called_once_with(batches)
    endpoint_writer.publish_observation.assert_not_called()
    old_writer.publish.assert_not_called()


def test_warehouse_raw_response_batch_preserves_every_individual_receipt(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import PreparedFreshObservation, publish_prepared_observations
    import etl.common.fresh_writer as writer_module

    writer = MagicMock()
    monkeypatch.setattr(writer_module, "WarehouseRawBQWriter", lambda *args: writer)
    observations = tuple(
        PreparedFreshObservation(
            layout="warehouse",
            target_project="project",
            dataset="dw_staging_xero",
            batches={"v2_ingest_receipts": [{"receipt_id": f"receipt-{number}"}]},
            is_manifest=False,
        )
        for number in (1, 2)
    )

    publish_prepared_observations(observations)

    writer.publish_observations_many.assert_called_once_with([
        observations[0].batches,
        observations[1].batches,
    ])


def test_wrong_provenance_reads_no_raw_data(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    reader = MagicMock()
    reader._read_meta.return_value = meta(**{"x-source-system": "legacy"})
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    process_fresh(config(), "bucket", f"v2/{GEN}/raw/xero/file.meta.json", "123", "project")
    reader.verify_data_identity.assert_not_called()
    reader.iter_records_from_meta.assert_not_called()


def test_admitted_manifest_has_atomic_receipt_and_immutable_run_history(monkeypatch):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    import etl.common.fresh_writer as writer_module
    import etl.xero.fresh_staging as staging_module
    import etl.xero.run_history as history
    reader = MagicMock()
    reader._read_meta.return_value = meta()
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    monkeypatch.setattr(history, "manifest_rows", lambda *args: (
        {"run_id": "run", "started_at": config().started_at}, [{"endpoint": "payments"}],
    ))
    monkeypatch.setattr(history, "target_dataset", lambda *args: ("project", "dw_staging_xero"))
    monkeypatch.setattr(staging_module, "build_observation", lambda *args: {
        "v2_record_versions": [], "v2_child_versions": [], "v2_ingest_receipts": [{"receipt_id": "receipt"}],
    })
    writer = MagicMock()
    monkeypatch.setattr(writer_module, "FreshBQWriter", lambda *args: writer)
    process_fresh(config(), "bucket", f"v2/{GEN}/_manifests/xero/2026-10-06/run.json.meta.json", "123", "project")
    written = writer.publish.call_args.args[0]
    assert written["v2_sync_runs"][0]["generation"] == GEN
    assert written["v2_sync_run_entities"][0]["generation"] == GEN
    assert written["v2_ingest_receipts"][0]["receipt_id"] == "receipt"


def test_fresh_only_entrypoint_never_falls_back_with_missing_config(monkeypatch):
    from types import SimpleNamespace
    from unittest.mock import MagicMock
    from etl.cloud_function import main
    monkeypatch.delenv("XERO_INGESTION_GENERATION", raising=False)
    monkeypatch.delenv("XERO_FRESH_START_AT", raising=False)
    legacy = MagicMock()
    monkeypatch.setattr(main, "process_gcs_upload", legacy)
    with pytest.raises(ValueError, match="fresh-only"):
        main.process_fresh_gcs_upload(SimpleNamespace(data={}))
    legacy.assert_not_called()


@pytest.mark.parametrize("bucket", ["documents", "sensitive", "other"])
def test_ordinary_records_cannot_cross_bucket_boundary(monkeypatch, bucket):
    from unittest.mock import MagicMock
    from etl.xero.fresh_ingestion import process_fresh
    monkeypatch.setenv("GCS_BUCKET", "ordinary")
    monkeypatch.setenv("XERO_DOCUMENT_GCS_BUCKET", "documents")
    monkeypatch.setenv("XERO_SENSITIVE_GCS_BUCKET", "sensitive")
    reader = MagicMock()
    reader._read_meta.return_value = meta()
    monkeypatch.setattr("etl.xero.fresh_ingestion.PinnedReader", lambda *args: reader)
    path = f"v2/{GEN}/raw/xero/tenant/2.0/payments/2026-10-06/file.meta.json"
    with pytest.raises(ValueError, match="ordinary raw bucket"):
        process_fresh(config(), bucket, path, "123", "project")
    reader.verify_data_identity.assert_not_called()
