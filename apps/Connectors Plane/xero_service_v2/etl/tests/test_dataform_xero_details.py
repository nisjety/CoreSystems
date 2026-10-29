"""Verify Dataform resolves the detail-derived Xero staging contracts."""

import json
import shutil
import subprocess
from pathlib import Path
from unittest import SkipTest


DATAFORM_DIR = Path(__file__).resolve().parents[2] / "Dataform"


def test_ordinary_xero_models_use_active_staging_dataset():
    definitions = (DATAFORM_DIR / "definitions").glob("**/*.sqlx")
    source = "\n".join(path.read_text() for path in definitions)

    assert 'schema: "staging_xero"' not in source
    assert 'ref("staging_xero"' not in source
    assert source.count('schema: "dw_staging_xero"') == 48


def test_detail_silver_models_use_current_staging_snapshots():
    if shutil.which("dataform") is None:
        raise SkipTest("Dataform CLI is not installed")
    result = subprocess.run(
        ["dataform", "compile", "--json"],
        cwd=DATAFORM_DIR,
        capture_output=True,
        text=True,
        check=True,
    )
    compiled = json.loads(result.stdout)
    assert not compiled["graphErrors"], compiled["graphErrors"]
    tables = {table["target"]["name"]: table for table in compiled["tables"]}
    expected = {
        "fact_budget": {"budgets"},
        "fact_budget_line": {"budgets", "budget_balance_snapshots"},
        "dim_contact_group": {"contact_groups"},
        "dim_contact_group_member": {"contact_groups", "contact_group_member_snapshots"},
        "fact_batch_payment": {"batch_payments"},
        "fact_batch_payment_line": {"batch_payments", "batch_payment_line_snapshots"},
        "fact_linked_transaction": {"linked_transactions"},
        "dim_repeating_invoice": {"repeating_invoices"},
        "dim_repeating_invoice_line": {"repeating_invoices", "repeating_invoice_line_snapshots"},
    }
    for name, staging_names in expected.items():
        dependencies = {
            target["name"] for target in tables[name]["dependencyTargets"]
            if target["schema"] == "dw_staging_xero"
        }
        assert staging_names <= dependencies, (name, dependencies)
    for name in (
        "fact_budget_line", "dim_contact_group_member", "fact_batch_payment_line",
        "dim_repeating_invoice_line",
    ):
        assert "s.synced_at = h.synced_at" in tables[name]["query"]
    batch_lines = tables["fact_batch_payment_line"]["query"]
    for field in ("s.line_index", "s.account_currency_code", "s.bank_amount"):
        assert field in batch_lines


def test_extended_current_models_require_same_run_headers_and_complete_children():
    if shutil.which("dataform") is None:
        raise SkipTest("Dataform CLI is not installed")
    result = subprocess.run(
        ["dataform", "compile", "--json"], cwd=DATAFORM_DIR,
        capture_output=True, text=True, check=True,
    )
    tables = {table["target"]["name"]: table for table in json.loads(result.stdout)["tables"]}
    for name in ("current_extended_records", "current_sensitive_extended_records"):
        query = tables[name]["query"]
        assert "h.run_id = l.run_id" in query
        assert "o.expected_lists = c.observed_lists" in query
        assert "c.observed_lists = c.distinct_lists" in query
    ordinary = tables["current_extended_records"]["query"]
    assert "project_child_scopes" in ordinary
    assert "missing_task_lists = 0" in ordinary
    assert "missing_time_lists = 0" in ordinary
