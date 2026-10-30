import sys
from datetime import datetime, timezone

import pytest

from etl.xero.run_history import manifest_rows, target_dataset, write_manifest_rows
from etl.backfill_manifests import backfill_manifest_history


RUN_ID = "5f3c9a2e-0000-0000-0000-000000000000"
META_PATH = f"_manifests/xero/2026-10-01/{RUN_ID}.json.meta.json"


def _manifest(entities=None):
    return {
        "run_id": RUN_ID,
        "started_at": "2026-10-01T10:00:00+00:00",
        "finished_at": "2026-10-01T10:01:00+00:00",
        "mode": "extended",
        "window_days": None,
        "entities": entities if entities is not None else [
            {"tenant": "tenant-1", "endpoint": "assets_assets", "pages": 1,
             "records": 2, "termination": "ended_on_empty_page", "error": None},
            {"tenant": "tenant-1", "endpoint": "assets_asset_details", "pages": 0,
             "records": 0, "termination": "error", "error": "private payload"},
        ],
    }


def test_manifest_rows_preserve_grain_without_exposing_error_text():
    run, outcomes = manifest_rows(META_PATH, _manifest())
    assert run["run_id"] == RUN_ID
    assert run["status"] == "failed"
    assert run["entity_count"] == 2
    assert run["error_count"] == 1
    assert run["record_count"] == 2
    assert run["finished_at"] == datetime(2026, 10, 1, 10, 1, tzinfo=timezone.utc)
    assert [row["outcome_index"] for row in outcomes] == [0, 1]
    assert outcomes[1]["error_present"] is True
    assert "private payload" not in str((run, outcomes))


def test_manifest_rows_persist_only_sanitized_source_failure_metadata():
    entity = {
        "tenant": "tenant-1",
        "endpoint": "accounting_get_getonlineinvoice",
        "pages": 0,
        "records": 0,
        "termination": "error",
        "outcome_class": "not_applicable",
        "http_status": 404,
        "error": "404 getOnlineInvoice /Invoices/private-invoice-id/OnlineInvoice",
    }

    run, outcomes = manifest_rows(META_PATH, _manifest([entity]))

    assert run["status"] == "succeeded"
    assert run["error_count"] == 0
    assert run["unavailable_count"] == 1
    assert outcomes == [{
        "run_id": RUN_ID,
        "outcome_index": 0,
        "tenant_id": "tenant-1",
        "endpoint": "accounting_get_getonlineinvoice",
        "request_key": None,
        "pages": 0,
        "records": 0,
        "termination": "error",
        "error_present": True,
        "outcome_class": "not_applicable",
        "http_status": 404,
        "synced_at": datetime(2026, 10, 1, 10, 1, tzinfo=timezone.utc),
    }]
    assert "private-invoice-id" not in str((run, outcomes))


def test_manifest_rows_accept_not_applicable_without_an_error_message():
    entity = {
        "tenant": "tenant-1", "endpoint": "accounting_get_getinvoice",
        "pages": 0, "records": 0, "termination": "not_applicable_no_parent_id",
        "outcome_class": "not_applicable", "error": None,
    }

    run, outcomes = manifest_rows(META_PATH, _manifest([entity]))

    assert run["status"] == "succeeded"
    assert run["unavailable_count"] == 1
    assert outcomes[0]["error_present"] is False


def test_manifest_rows_marks_access_denied_as_an_actionable_run_failure():
    entity = {
        "tenant": "tenant-1", "endpoint": "projects_projects",
        "pages": 0, "records": 0, "termination": "error",
        "outcome_class": "access_denied", "http_status": 403,
        "error": "Xero source request failed (HTTP 403)",
    }

    run, outcomes = manifest_rows(META_PATH, _manifest([entity]))

    assert run["status"] == "failed"
    assert run["error_count"] == 1
    assert run["unavailable_count"] == 1
    assert outcomes[0]["outcome_class"] == "access_denied"


def test_manifest_rows_preserve_an_optional_request_key_for_scoped_completeness():
    entity = {
        "tenant": "tenant-1",
        "endpoint": "report_aged_receivables_by_contact",
        "request_key": "contact:8e702420-b203-48ac-99d5-ccdbb308f8d5",
        "pages": 1,
        "records": 1,
        "termination": "report-snapshot",
        "error": None,
    }

    _, outcomes = manifest_rows(META_PATH, _manifest([entity]))

    assert outcomes[0]["request_key"] == entity["request_key"]


@pytest.mark.parametrize("request_key", ["", 1, [], {}])
def test_manifest_rows_reject_invalid_optional_request_keys(request_key):
    entity = {
        "tenant": "tenant-1", "endpoint": "accounts", "pages": 1,
        "records": 1, "termination": "ended_on_empty_page", "error": None,
        "request_key": request_key,
    }

    with pytest.raises(ValueError, match="request key"):
        manifest_rows(META_PATH, _manifest([entity]))


def test_manifest_rows_detect_truncation_and_allow_empty_runs():
    entity = {"tenant": "tenant-1", "endpoint": "accounts", "pages": 10,
              "records": 100, "termination": "hit_max_pages_cap", "error": None}
    run, _ = manifest_rows(META_PATH, _manifest([entity]))
    assert run["status"] == "failed"
    assert run["error_count"] == 1
    empty, outcomes = manifest_rows(META_PATH, _manifest([]))
    assert empty["status"] == "succeeded"
    assert outcomes == []


def test_manifest_rows_count_error_termination_without_error_text():
    entity = {"tenant": "tenant-1", "endpoint": "accounts", "pages": 0,
              "records": 0, "termination": "error", "error": ""}
    run, _ = manifest_rows(META_PATH, _manifest([entity]))
    assert run["status"] == "failed"
    assert run["error_count"] == 1


def test_manifest_rows_reject_non_object_body():
    with pytest.raises(ValueError, match="object"):
        manifest_rows(META_PATH, [])


def test_manifest_path_must_match_body_run_id():
    with pytest.raises(ValueError, match="run ID"):
        manifest_rows(META_PATH, _manifest() | {"run_id": "other"})


def test_sensitive_manifest_requires_restricted_bucket_and_dataset(monkeypatch):
    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("XERO_SENSITIVE_GCS_BUCKET", "restricted-raw")
    monkeypatch.setenv("XERO_SENSITIVE_DATASET", "staging_xero_sensitive")
    endpoints = ["finance_balance_sheet"]
    assert target_dataset("restricted-raw", endpoints, "fallback") == (
        "dw-prod", "staging_xero_sensitive")
    with pytest.raises(ValueError, match="restricted bucket"):
        target_dataset("ordinary-raw", endpoints, "fallback")
    assert target_dataset("ordinary-raw", ["accounts"], "fallback") == (
        "fallback", "dw_staging_xero")


def test_writer_merges_run_and_indexed_outcomes():
    run, outcomes = manifest_rows(META_PATH, _manifest())

    class Writer:
        def __init__(self):
            self.calls = []

        def merge(self, table, rows, key_columns, schema):
            self.calls.append((table, rows, key_columns, schema))
            return len(rows)

    writer = Writer()
    assert write_manifest_rows(writer, run, outcomes) == 3
    assert [call[0] for call in writer.calls] == ["sync_runs", "sync_run_entities"]
    assert writer.calls[1][2] == ("run_id", "outcome_index")


@pytest.mark.parametrize(
    ("bucket", "endpoint", "expected_dataset"),
    [
        ("ordinary-raw", "accounts", "dw_staging_xero"),
        ("restricted-raw", "finance_balance_sheet", "staging_xero_sensitive"),
    ],
)
def test_cloud_function_routes_manifest_sidecar_to_run_history(
    monkeypatch, bucket, endpoint, expected_dataset,
):
    from etl.cloud_function import main as cloud_function

    monkeypatch.setenv("GCP_PROJECT", "dw-prod")
    monkeypatch.setenv("XERO_SENSITIVE_GCS_BUCKET", "restricted-raw")
    monkeypatch.setenv("XERO_SENSITIVE_DATASET", "staging_xero_sensitive")
    monkeypatch.setattr(cloud_function, "GCS_BUCKET", bucket)
    monkeypatch.setattr(cloud_function, "PROJECT", "dw-prod")
    manifest = _manifest([{
        "tenant": "tenant-1", "endpoint": endpoint, "pages": 1,
        "records": 2, "termination": "ended_on_empty_page", "error": None,
    }])
    calls = []

    class Reader:
        def __init__(self, bucket, project):
            assert bucket in ("ordinary-raw", "restricted-raw")

        def _read_data(self, path):
            assert path == META_PATH.removesuffix(".meta.json")
            return manifest

    class Writer:
        def __init__(self, project, dataset):
            assert project == "dw-prod"
            assert dataset == expected_dataset

        def merge(self, table, rows, key_columns, schema):
            calls.append(table)
            return len(rows)

    monkeypatch.setattr(cloud_function, "GCSReader", Reader)
    monkeypatch.setattr(cloud_function, "BQWriter", Writer)

    class Event:
        data = {"bucket": bucket, "name": META_PATH}

    cloud_function.process_gcs_upload(Event())
    assert calls == ["sync_runs", "sync_run_entities"]


def test_manifest_backfill_scans_sidecars_and_reuses_warehouse_contract(monkeypatch):
    monkeypatch.delenv("XERO_SENSITIVE_GCS_BUCKET", raising=False)
    names = [META_PATH, META_PATH.removesuffix(".meta.json"), "unrelated.txt"]
    calls = []

    class Blob:
        def __init__(self, name):
            self.name = name

    class Storage:
        def list_blobs(self, bucket, prefix):
            assert bucket == "ordinary-raw"
            assert prefix == "_manifests/xero/"
            return [Blob(name) for name in names]

    class Reader:
        def _read_data(self, path):
            assert path == META_PATH.removesuffix(".meta.json")
            return _manifest([])

    class Writer:
        def merge(self, table, rows, key_columns, schema):
            calls.append(table)
            return len(rows)

    result = backfill_manifest_history(
        Storage(), Reader(), lambda project, dataset: Writer(),
        bucket="ordinary-raw", default_project="dw-prod",
    )
    assert result == {"manifests": 1, "rows": 1}
    assert calls == ["sync_runs"]


def test_manifest_backfill_accepts_main_generation_prefix(monkeypatch):
    monkeypatch.setenv("GCS_BUCKET", "ordinary-raw")
    generation = "550e8400-e29b-41d4-a716-446655440000"
    prefix = f"xero/{generation}/_manifests/xero/"
    generated_path = f"{prefix}2026-10-01/{RUN_ID}.json.meta.json"
    reads = []

    class Blob:
        def __init__(self, name):
            self.name = name

    class Storage:
        def list_blobs(self, bucket, prefix):
            assert bucket == "ordinary-raw"
            assert prefix == f"xero/{generation}/_manifests/xero/"
            return [Blob(generated_path)]

    class Reader:
        def _read_data(self, path):
            reads.append(path)
            return _manifest([])

    class Writer:
        def merge(self, table, rows, key_columns, schema):
            return len(rows)

    result = backfill_manifest_history(
        Storage(), Reader(), lambda project, dataset: Writer(),
        bucket="ordinary-raw", default_project="dw-prod", prefix=prefix,
    )

    assert result == {"manifests": 1, "rows": 1}
    assert reads == [generated_path.removesuffix(".meta.json")]


def test_manifest_backfill_cli_requires_target_and_reports_counts(monkeypatch, capsys):
    from etl import backfill_manifests

    monkeypatch.delenv("GCS_BUCKET", raising=False)
    monkeypatch.delenv("GCP_PROJECT", raising=False)
    monkeypatch.setattr(sys, "argv", ["backfill_manifests"])
    with pytest.raises(SystemExit) as exc:
        backfill_manifests.main()
    assert exc.value.code == 2
    monkeypatch.setattr(sys, "argv", [
        "backfill_manifests", "--bucket", "ordinary-raw", "--project", "dw-prod",
    ])

    class Storage:
        def __init__(self, project):
            assert project == "dw-prod"

        def list_blobs(self, bucket, prefix):
            return []

    monkeypatch.setattr(backfill_manifests.storage, "Client", Storage)
    monkeypatch.setattr(backfill_manifests, "GCSReader", lambda **kwargs: object())
    assert backfill_manifests.main() == 0
    assert "Replayed 0 manifests" in capsys.readouterr().out


@pytest.mark.parametrize(
    ("change", "reason"),
    [
        ({"started_at": "not-a-date"}, "timestamp"),
        ({"finished_at": "2026-09-30T10:00:00+00:00"}, "finished before"),
        ({"entities": "wrong"}, "array"),
        ({"window_days": -1}, "window_days"),
    ],
)
def test_manifest_validation_fails_closed(change, reason):
    with pytest.raises(ValueError, match=reason):
        manifest_rows(META_PATH, _manifest() | change)
