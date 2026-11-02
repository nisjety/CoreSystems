"""Pure classification for the Xero warehouse evidence matrix."""

from datetime import datetime, timedelta, timezone

from etl.xero.quality_report import assess, safe_dataset, warehouse_evidence


NOW = datetime(2026, 10, 1, 12, tzinfo=timezone.utc)


def _outcome(endpoint="assets_assets", records=2, **overrides):
    return {
        "tenant_id": "tenant-1", "endpoint": endpoint, "run_id": "run-1",
        "mode": "extended", "records": records, "pages": 1,
        "observed_at": NOW - timedelta(hours=1), "failed": False,
    } | overrides


def test_exact_collection_requires_same_run_and_count():
    current = {("tenant-1", "assets_assets"): {"rows": 2, "run_id": "run-1"}}
    rows = assess([_outcome()], {}, current, [], NOW, 36)
    assert rows[0]["check_class"] == "exact"
    assert rows[0]["status"] == "ok"
    assert rows[0]["raw_records"] == rows[0]["silver_rows"] == 2

    pending = assess([_outcome()], {}, {
        ("tenant-1", "assets_assets"): {"rows": 2, "run_id": "old-run"},
    }, [], NOW, 36)
    assert pending[0]["status"] == "pending_current_model"
    gap = assess([_outcome()], {}, {
        ("tenant-1", "assets_assets"): {"rows": 1, "run_id": "run-1"},
    }, [], NOW, 36)
    assert gap[0]["status"] == "exact_gap"


def test_accounting_counts_are_advisory_and_duplicates_are_errors():
    staging = {("tenant-1", "invoices"): {"rows": 20, "distinct_keys": 19}}
    rows = assess([_outcome(endpoint="invoices", records=3, mode="incremental")],
                  staging, {}, [], NOW, 36)
    assert rows[0]["check_class"] == "advisory"
    assert rows[0]["status"] == "duplicate_keys"
    staging[("tenant-1", "invoices")] = {"rows": 20, "distinct_keys": 20}
    rows = assess([_outcome(endpoint="invoices", records=3, mode="incremental")],
                  staging, {}, [], NOW, 36)
    assert rows[0]["status"] == "advisory"


def test_failures_empty_stale_and_expected_missing_are_distinct():
    failed = _outcome(failed=True)
    empty = _outcome(endpoint="contacts", records=0)
    old = _outcome(endpoint="projects_projects", observed_at=NOW - timedelta(days=3))
    rows = assess([failed, empty, old], {}, {}, [("tenant-1", "finance_cash_validation")],
                  NOW, 36)
    status = {row["endpoint"]: row["status"] for row in rows}
    assert status == {
        "assets_assets": "failed_run", "contacts": "no_data",
        "projects_projects": "stale", "finance_cash_validation": "missing_run",
    }


def test_complete_empty_collection_is_valid_no_data():
    rows = assess([_outcome(records=0, requests=3)], {}, {}, [], NOW, 36,
                  memberships={("tenant-1", "assets_assets", "run-1"): {
                      "lists": 3, "records": 0,
                  }})
    assert rows[0]["status"] == "no_data"
    assert rows[0]["silver_rows"] is None

    incomplete = assess([_outcome(records=0, requests=3)], {}, {}, [], NOW, 36,
                        memberships={("tenant-1", "assets_assets", "run-1"): {
                            "lists": 2, "records": 0,
                        }})
    assert incomplete[0]["status"] == "pending_current_model"
    stale_current = assess([_outcome(records=0, requests=3)], {}, {
        ("tenant-1", "assets_assets"): {"rows": 2, "run_id": "old-run"},
    }, [], NOW, 36, memberships={("tenant-1", "assets_assets", "run-1"): {
        "lists": 3, "records": 0,
    }})
    assert stale_current[0]["status"] == "pending_current_model"


def test_dataset_identifier_is_restricted():
    assert safe_dataset("staging_xero") == "staging_xero"
    assert safe_dataset("staging_xero_sensitive") == "staging_xero_sensitive"
    import pytest
    with pytest.raises(ValueError):
        safe_dataset("staging_xero`; DROP TABLE x")


def test_warehouse_evidence_reads_run_scoped_memberships_and_current_rows():
    class Query:
        def __init__(self, rows):
            self.rows = rows

        def result(self):
            return self.rows

    class Client:
        def __init__(self):
            self.sql = []

        def query(self, sql):
            self.sql.append(sql)
            if "staging_xero.INFORMATION_SCHEMA.TABLES" in sql:
                return Query([{"table_name": name} for name in (
                    "sync_runs", "sync_run_entities", "extended_records",
                    "extended_list_snapshots",
                )])
            if "dw_1_silver_xero.INFORMATION_SCHEMA.TABLES" in sql:
                return Query([{"table_name": "current_extended_records"}])
            if "WITH grouped AS" in sql:
                return Query([_outcome(requests=1)])
            if "FROM `dw-prod.staging_xero.extended_records`" in sql:
                return Query([{"tenant_id": "tenant-1", "endpoint": "assets_assets",
                               "rows": 2, "distinct_keys": 2}])
            if "FROM `dw-prod.staging_xero.extended_list_snapshots`" in sql:
                return Query([{"tenant_id": "tenant-1", "source_endpoint": "assets_assets",
                               "run_id": "run-1", "lists": 1, "records": 2}])
            if "FROM `dw-prod.dw_1_silver_xero.current_extended_records`" in sql:
                return Query([{"tenant_id": "tenant-1", "endpoint": "assets_assets",
                               "run_id": "run-1", "rows": 2}])
            raise AssertionError(sql)

    client = Client()
    rows = warehouse_evidence(client, "dw-prod", "staging_xero", [], NOW, 36)
    assert rows[0]["status"] == "ok"
    assert rows[0]["membership_lists"] == 1
    assert rows[0]["staging_distinct_keys"] == 2
    assert all("staging_xero_sensitive" not in sql for sql in client.sql)


def test_quality_cli_reports_exceptions_without_payload(monkeypatch, capsys):
    import json
    from etl.xero import quality_report

    monkeypatch.setattr(quality_report.bigquery, "Client", lambda project: object())
    monkeypatch.setattr(quality_report, "warehouse_evidence", lambda *args: [
        {"tenant_id": "tenant-1", "endpoint": "invoices", "status": "missing_run"},
    ])
    assert quality_report.main([
        "--project", "dw-prod", "--expect", "tenant-1:invoices", "--fail-on-exceptions",
    ]) == 2
    report = json.loads(capsys.readouterr().out)
    assert report["exceptions"] == 1
    assert "payload_json" not in str(report)
