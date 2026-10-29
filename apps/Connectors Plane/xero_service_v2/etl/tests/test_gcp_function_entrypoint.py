import importlib.util
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock


ROOT = Path(__file__).resolve().parents[2]


def test_gcp_function_entrypoint_exposes_xero_loader_handler():
    spec = importlib.util.spec_from_file_location("xero_gcp_entrypoint", ROOT / "main.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    assert callable(module.process_gcs_upload)
    assert callable(module.process_fresh_gcs_upload)


def test_aged_contact_reports_are_registered_for_typed_projection():
    from etl.cloud_function import main
    from etl.xero import reports

    assert main.XERO_PARSERS["report_aged_receivables_by_contact"] is reports
    assert main.XERO_PARSERS["report_aged_payables_by_contact"] is reports


def test_fresh_handler_commits_raw_history_before_current_typed_projection(monkeypatch):
    from etl.cloud_function import main

    fresh = SimpleNamespace(
        namespace="main",
        generation="550e8400-e29b-41d4-a716-446655440000",
    )
    prepared = SimpleNamespace(
        layout="warehouse",
        target_project="project",
        dataset="dw_staging_xero",
        is_manifest=False,
        records=({"record_id": "payment-1"},),
    )
    projection = SimpleNamespace(
        endpoint="payments",
        batch_key=("project", "dw_staging_xero", "xero", "payments"),
    )
    committed = []

    class ImmediateBatcher:
        def submit(self, _key, item, publish):
            publish((item,))

    monkeypatch.setattr(main.FreshConfig, "from_env", lambda: fresh)
    prepare_raw = MagicMock(return_value=prepared)
    prepare_typed = MagicMock(return_value=projection)
    monkeypatch.setattr(main, "prepare_fresh_observation", prepare_raw)
    monkeypatch.setattr(main, "current_projection_from_records", prepare_typed)
    monkeypatch.setattr(main, "_raw_batcher", ImmediateBatcher())
    monkeypatch.setattr(main, "_typed_batcher", ImmediateBatcher())
    monkeypatch.setattr(main, "_publish_prepared_raw", lambda items: committed.append(("raw", items)))
    monkeypatch.setattr(main, "_publish_current_projections",
                        lambda items: committed.append(("typed", items)))

    event = SimpleNamespace(data={
        "message": {"attributes": {
            "bucketId": main.GCS_BUCKET,
            "objectId": (
                "xero/550e8400-e29b-41d4-a716-446655440000/"
                "raw/xero/tenant-1/2.0/payments/2026-10-16/page.json.meta.json"
            ),
            "objectGeneration": "123",
            "eventType": "OBJECT_FINALIZE",
        }},
    })

    main.process_fresh_gcs_upload(event)

    prepare_raw.assert_called_once_with(
        fresh,
        main.GCS_BUCKET,
        event.data["message"]["attributes"]["objectId"],
        "123",
        main.PROJECT,
    )
    prepare_typed.assert_called_once_with(
        main.GCS_BUCKET,
        event.data["message"]["attributes"]["objectId"],
        "raw/xero/tenant-1/2.0/payments/2026-10-16/page.json.meta.json",
        prepared.records,
    )
    assert committed == [("raw", (prepared,)), ("typed", (projection,))]


def test_fresh_handler_ignores_objects_outside_its_active_generation(monkeypatch):
    from etl.cloud_function import main

    fresh = SimpleNamespace(
        namespace="main",
        generation="550e8400-e29b-41d4-a716-446655440000",
    )
    prepare_raw = MagicMock()
    typed_projection = MagicMock()
    monkeypatch.setattr(main.FreshConfig, "from_env", lambda: fresh)
    monkeypatch.setattr(main, "prepare_fresh_observation", prepare_raw)
    monkeypatch.setattr(main, "_prepare_current_projection", typed_projection)

    event = SimpleNamespace(data={
        "message": {"attributes": {
            "bucketId": main.GCS_BUCKET,
            "objectId": "raw/xero/tenant-1/2.0/payments/2026-10-16/page.json.meta.json",
            "objectGeneration": "123",
            "eventType": "OBJECT_FINALIZE",
        }},
    })

    main.process_fresh_gcs_upload(event)

    prepare_raw.assert_not_called()
    typed_projection.assert_not_called()


def test_fresh_handler_keeps_direct_accounting_gets_in_raw_history_only(monkeypatch):
    from etl.cloud_function import main

    fresh = SimpleNamespace(
        namespace="main",
        generation="550e8400-e29b-41d4-a716-446655440000",
    )
    prepared = SimpleNamespace(
        layout="warehouse",
        target_project="project",
        dataset="dw_staging_xero",
        is_manifest=False,
        records=({"record_id": "account-1"},),
    )
    projection = SimpleNamespace(
        endpoint="accounting_get_getaccount",
        batch_key=("project", "dw_staging_xero", "xero", "accounting_get_getaccount"),
    )
    committed = []

    class ImmediateBatcher:
        def submit(self, _key, item, publish):
            publish((item,))

    monkeypatch.setattr(main.FreshConfig, "from_env", lambda: fresh)
    monkeypatch.setattr(main, "prepare_fresh_observation", lambda *_: prepared)
    monkeypatch.setattr(main, "current_projection_from_records", lambda *_: projection)
    monkeypatch.setattr(main, "_raw_batcher", ImmediateBatcher())
    monkeypatch.setattr(main, "_typed_batcher", ImmediateBatcher())
    monkeypatch.setattr(main, "_publish_prepared_raw", lambda items: committed.append(items))
    publish_typed = MagicMock()
    monkeypatch.setattr(main, "_publish_current_projections", publish_typed)

    event = SimpleNamespace(data={"message": {"attributes": {
        "bucketId": main.GCS_BUCKET,
        "objectId": (
            "xero/550e8400-e29b-41d4-a716-446655440000/"
            "raw/xero/tenant-1/2.0/accounting_get_getaccount/2026-10-16/page.json.meta.json"
        ),
        "objectGeneration": "123",
        "eventType": "OBJECT_FINALIZE",
    }}})

    main.process_fresh_gcs_upload(event)

    assert committed == [(prepared,)]
    publish_typed.assert_not_called()


def test_fresh_handler_rejects_a_foreign_bucket_before_reading_raw_data(monkeypatch):
    from etl.cloud_function import main

    fresh = SimpleNamespace(
        namespace="main",
        generation="550e8400-e29b-41d4-a716-446655440000",
    )
    prepare_raw = MagicMock()
    monkeypatch.setattr(main.FreshConfig, "from_env", lambda: fresh)
    monkeypatch.setattr(main, "prepare_fresh_observation", prepare_raw)
    event = SimpleNamespace(data={
        "message": {"attributes": {
            "bucketId": "foreign-bucket",
            "objectId": (
                "xero/550e8400-e29b-41d4-a716-446655440000/"
                "raw/xero/tenant-1/2.0/payments/2026-10-16/page.json.meta.json"
            ),
            "objectGeneration": "123",
            "eventType": "OBJECT_FINALIZE",
        }},
    })

    try:
        main.process_fresh_gcs_upload(event)
    except ValueError as exc:
        assert "unexpected GCS bucket" in str(exc)
    else:
        raise AssertionError("foreign bucket must fail before a raw history write")

    prepare_raw.assert_not_called()


def test_typed_projection_batch_uses_one_parser_and_one_writer(monkeypatch):
    from etl.cloud_function import main

    parser = MagicMock(return_value={"records": 2})
    writer = MagicMock()
    monkeypatch.setitem(main.VENDOR_PARSERS["xero"], "payments", parser)
    monkeypatch.setattr(main, "BQWriter", lambda **_: writer)
    projections = tuple(
        main._TypedProjection(
            project="project",
            dataset="dw_staging_xero",
            vendor="xero",
            endpoint="payments",
            records=({"record_id": f"record-{number}"},),
            source_objects=(f"object-{number}",),
        )
        for number in (1, 2)
    )

    main._publish_current_projections(projections)

    parser.run.assert_called_once()
    reader = parser.run.call_args.args[0]
    assert list(reader.iter_records("payments")) == [
        {"record_id": "record-1"},
        {"record_id": "record-2"},
    ]
    assert parser.run.call_args.args[1] is writer


def test_gcloudignore_limits_function_upload_to_runtime_sources():
    rules = (ROOT / ".gcloudignore").read_text().splitlines()

    assert ".env*" in rules
    assert "data/" in rules
    assert "target/" in rules
    assert "Dataform/" in rules
    assert "etl/tests/" in rules
    assert "etl/**/__pycache__/" in rules
