import json
import subprocess
from pathlib import Path

import pytest

from etl.common.endpoint_config import ARRAY_KEYS


DATAFORM = Path(__file__).resolve().parents[2] / "Dataform"
CORE_TABLES = {
    "xero_accounts", "xero_contacts", "xero_invoices", "xero_payments",
    "xero_journals", "xero_credit_notes", "xero_bank_transactions",
    "xero_bank_transfers", "xero_manual_journals", "xero_overpayments",
}


@pytest.fixture(scope="module")
def compiled_graph():
    result = subprocess.run(
        ["dataform", "compile", str(DATAFORM), "--json"],
        capture_output=True, text=True, check=True,
    )
    return json.loads(result.stdout)


def test_main_bronze_compiles_without_reusing_legacy_tables(compiled_graph):
    assert not compiled_graph["graphErrors"]
    tables = {
        action["target"]["name"]: action
        for action in compiled_graph["tables"]
        if action["target"]["schema"] == "dw_bronze_xero"
    }

    assert CORE_TABLES <= tables.keys()
    assert {f"xero_{endpoint}" for endpoint in ARRAY_KEYS} == tables.keys()
    assert "xero_payment_services" not in tables
    assert "xero_bills" not in tables
    for name in CORE_TABLES:
        action = tables[name]
        assert action["type"] == "incremental"
        assert action["protected"] is True
        sql = action["query"] + action.get("incrementalQuery", "")
        assert f"dw_staging_xero_main.{name.removeprefix('xero_')}" in sql
        assert "dw_1_bronze_xero" not in sql
        assert "v2_" not in sql
        assert f"'{name.removeprefix('xero_')}'" in sql
        for field in ("generation", "tenant_id", "record_id", "version_id", "payload_json"):
            assert field in sql


def test_main_bronze_has_insert_only_identity_guard(compiled_graph):
    action = next(
        action for action in compiled_graph["tables"]
        if action["target"]["schema"] == "dw_bronze_xero"
        and action["target"]["name"] == "xero_payments"
    )
    assert "NOT EXISTS" in action["incrementalQuery"]
    assert "source.version_id" in action["incrementalQuery"]
    assert "source.tenant_id" in action["incrementalQuery"]


def test_xero_bronze_assertions_are_isolated_from_visma(compiled_graph):
    assertions = [
        action for action in compiled_graph["assertions"]
        if action["target"]["name"].startswith("xero_bronze_")
    ]
    assert len(assertions) == len(ARRAY_KEYS)
    assert all(
        action["target"]["schema"] == "dw_bronze_xero_assertions"
        for action in assertions
    )
    assert not any(
        action["target"]["name"].startswith("dw_bronze_xero_")
        for action in compiled_graph["assertions"]
    )
