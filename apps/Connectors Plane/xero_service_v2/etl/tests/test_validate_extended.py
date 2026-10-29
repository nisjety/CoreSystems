import json
import sys

import pytest

from etl.validate_extended import main, validate_tree


def _pair(root, endpoint, body, *, tenant="tenant-1", version="1.0", count=1,
          overrides=None):
    path = root / "raw" / "xero" / tenant / version / endpoint / "2026-09-30" / "page.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(body), encoding="utf-8")
    meta = {
        "x-tenant-id": tenant,
        "x-endpoint": endpoint,
        "x-api-version": version,
        "x-http-status": "200",
        "x-record-count": str(count),
        "x-synced-at": "2026-09-30T10:00:00Z",
    } | (overrides or {})
    path.with_name(path.name + ".meta.json").write_text(json.dumps(meta), encoding="utf-8")
    return path


def test_validates_extended_dry_run_without_payload_in_report(tmp_path):
    _pair(tmp_path, "assets_assets", {"items": [{"assetId": "private-asset-id"}]})
    _pair(tmp_path, "projects_projects", {"items": []}, version="2.0", count=0)
    report = validate_tree(tmp_path)
    assert report == {
        "files": 2,
        "records": 1,
        "endpoints": {"assets_assets": 1, "projects_projects": 0},
        "errors": [],
    }
    assert "private-asset-id" not in json.dumps(report)


def test_validates_an_isolated_generation_prefix(tmp_path):
    generation_root = tmp_path / "xero" / "a3e6e7e5-04c1-4574-a63f-e0863ea9cef1"
    _pair(generation_root, "projects_projects", {"items": []}, version="2.0", count=0)

    assert validate_tree(tmp_path) == {
        "files": 1,
        "records": 0,
        "endpoints": {"projects_projects": 0},
        "errors": [],
    }


def test_rejects_sidecar_mismatch_and_wrong_version(tmp_path):
    _pair(tmp_path, "assets_assets", {"items": [{"assetId": "id"}]},
          overrides={"x-tenant-id": "another-tenant"})
    report = validate_tree(tmp_path)
    assert report["records"] == 0
    assert len(report["errors"]) == 1
    assert "tenant" in report["errors"][0]


def test_rejects_record_count_drift_and_missing_snapshot_key(tmp_path):
    _pair(tmp_path, "assets_assets", {"items": [{"assetId": "id"}]}, count=2)
    _pair(tmp_path, "finance_balance_sheet", {"amount": 12}, tenant="tenant-2")
    report = validate_tree(tmp_path)
    assert report["files"] == 2
    assert report["records"] == 0
    assert len(report["errors"]) == 2


def test_rejects_missing_data_file_and_empty_tree(tmp_path):
    assert validate_tree(tmp_path)["errors"] == ["no extended sidecars found"]
    path = _pair(tmp_path, "identity_connections", [{"id": "id"}],
                 tenant="_application", version="identity")
    path.unlink()
    report = validate_tree(tmp_path)
    assert report["files"] == 1
    assert len(report["errors"]) == 1
    assert "missing data file" in report["errors"][0]


@pytest.mark.parametrize(
    ("version", "overrides", "reason"),
    [
        ("2.0", None, "API version"),
        ("1.0", {"x-api-version": "2.0"}, "API version"),
        ("1.0", {"x-endpoint": "projects_projects"}, "endpoint"),
        ("1.0", {"x-api-family": "finance"}, "family"),
        ("1.0", {"x-http-status": "304"}, "HTTP status"),
        ("1.0", {"x-record-count": "not-a-number"}, "record count"),
        ("1.0", {"x-record-count": "-1"}, "record count"),
    ],
)
def test_rejects_invalid_contract_fields(tmp_path, version, overrides, reason):
    _pair(tmp_path, "assets_assets", {"items": [{"assetId": "id"}]},
          version=version, overrides=overrides)
    assert reason in validate_tree(tmp_path)["errors"][0]


def test_rejects_malformed_response_and_sidecar(tmp_path):
    path = _pair(tmp_path, "assets_assets", {"items": [{"assetId": "id"}]})
    path.write_text("not json", encoding="utf-8")
    assert "invalid JSON" in validate_tree(tmp_path)["errors"][0]
    path.write_text(json.dumps({"items": [{"assetId": "id"}]}), encoding="utf-8")
    path.with_name(path.name + ".meta.json").write_text("[]", encoding="utf-8")
    assert "sidecar is not a JSON object" in validate_tree(tmp_path)["errors"][0]


def test_cli_returns_nonzero_on_failure_and_never_prints_payload(tmp_path, monkeypatch, capsys):
    path = _pair(tmp_path, "assets_assets", {"items": [{"assetId": "secret-value"}]})
    monkeypatch.setattr(sys, "argv", ["validate_extended", str(tmp_path)])
    assert main() == 0
    assert "secret-value" not in capsys.readouterr().out
    path.unlink()
    assert main() == 1
    assert "missing data file" in capsys.readouterr().out
