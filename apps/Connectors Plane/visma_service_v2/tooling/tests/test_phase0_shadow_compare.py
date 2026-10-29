from __future__ import annotations

from pathlib import Path

import pytest

from vsv2_tooling.discovery import shadow_compare


@pytest.mark.unit
def test_load_candidate_configs_applies_datamart_meta_default_excludes(
    tmp_path: Path,
) -> None:
    """DM-only operational fields are excluded by default, even without a
    per-endpoint `compare_excludes` list."""
    import json

    endpoint_dir = tmp_path / "endpoints"
    endpoint_dir.mkdir()
    (endpoint_dir / "credit_terms.json").write_text(
        json.dumps(
            {
                "endpoint_name": "credit_terms",
                "bigquery_table": "visma_credit_terms",
                "primary_key": "id",
                "api_endpoint": "/v1/creditTerms",
                "pagination_profile": "finance_page_number",
                "datamart": {
                    "query_file": "config/queries/credit_terms.graphql",
                    "root_field": "creditTerms",
                    "id_field": "id",
                    "join_normalizer": "normalize_credit_term",
                },
            }
        )
    )

    bronze_dir = tmp_path / "bronze"
    bronze_dir.mkdir()
    (bronze_dir / "visma_credit_terms.sqlx").write_text(
        'config { type: "declaration", schema: "x", name: "visma_credit_terms" }\n'
        "//schema\n"
        + json.dumps(
            [
                {"name": "syncId", "type": "STRING"},
                {"name": "id", "type": "STRING"},
                {"name": "description", "type": "STRING"},
                {"name": "companyId", "type": "STRING"},
                {"name": "createdInDataMartDateTime", "type": "STRING"},
                {"name": "updatedInDataMartDateTime", "type": "STRING"},
                {"name": "deletedDatabaseRecord", "type": "BOOLEAN"},
            ]
        )
    )

    configs = shadow_compare._load_candidate_configs(endpoint_dir, bronze_dir, ["credit_terms"])

    excluded = shadow_compare.DATAMART_META_FIELDS
    assert excluded.isdisjoint(configs[0].compare_fields)
    assert "description" in configs[0].compare_fields


@pytest.mark.unit
def test_load_candidate_configs_honors_compare_excludes(tmp_path: Path) -> None:
    import json

    endpoint_dir = tmp_path / "endpoints"
    endpoint_dir.mkdir()
    (endpoint_dir / "accounts.json").write_text(
        json.dumps(
            {
                "endpoint_name": "accounts",
                "bigquery_table": "visma_accounts",
                "primary_key": "accountCD",
                "api_endpoint": "/v1/account",
                "pagination_profile": "finance_page_number",
                "datamart": {
                    "query_file": "config/queries/account.graphql",
                    "root_field": "accounts",
                    "id_field": "id",
                    "join_normalizer": "normalize_account",
                    "compare_excludes": [
                        "metadata",
                        "timeStamp",
                        "postOption",
                        "allowManualEntry",
                        "lastModifiedDateTime",
                    ],
                },
            }
        )
    )

    bronze_dir = tmp_path / "bronze"
    bronze_dir.mkdir()
    (bronze_dir / "visma_accounts.sqlx").write_text(
        'config { type: "declaration", schema: "x", name: "visma_accounts" }\n'
        "//schema\n"
        + json.dumps(
            [
                {"name": "syncId", "type": "STRING"},
                {"name": "accountCD", "type": "STRING"},
                {"name": "description", "type": "STRING"},
                {"name": "allowManualEntry", "type": "BOOLEAN"},
                {"name": "controlAccountModule", "type": "STRING"},
                {"name": "lastModifiedDateTime", "type": "STRING"},
                {"name": "metadata", "type": "STRING"},
                {"name": "postOption", "type": "STRING"},
                {"name": "timeStamp", "type": "STRING"},
            ]
        )
    )

    configs = shadow_compare._load_candidate_configs(endpoint_dir, bronze_dir, ["accounts"])

    assert len(configs) == 1
    excluded = {
        "metadata",
        "timeStamp",
        "postOption",
        "allowManualEntry",
        "lastModifiedDateTime",
    }
    assert excluded.isdisjoint(configs[0].compare_fields)
    assert "controlAccountModule" in configs[0].compare_fields
    assert "accountCD" in configs[0].compare_fields


@pytest.mark.unit
def test_load_shadow_candidate_names_reads_summary_table(tmp_path: Path) -> None:
    report = tmp_path / "report.md"
    report.write_text(
        "\n".join(
            [
                "# Report",
                "",
                "## Summary",
                "",
                (
                    "| endpoint | bronze_table | sample | normalizer | "
                    "delivered/bronze | missing | status |"
                ),
                "|---|---|---:|---|---:|---:|---|",
                (
                    "| `accounts` | `visma_accounts` | ok | `normalize_account` | "
                    "21/18 | 0 | `candidate_for_shadow` |"
                ),
                (
                    "| `budgets` | `visma_budgets` | fail | `normalize_budget` | "
                    "0/12 | 12 | `blocked_live_sample` |"
                ),
                "",
                "## Detailed Review",
            ]
        )
    )

    names = shadow_compare._load_shadow_candidate_names(report)

    assert names == ["accounts"]


@pytest.mark.unit
def test_record_key_reads_dotted_paths_from_json_strings() -> None:
    record = {
        "account": '{"number":"3000"}',
        "branchNumber": '{"number":"1"}',
        "financialYear": "2026",
    }

    key = shadow_compare._record_key(
        record,
        ["financialYear", "branchNumber.number", "account.number"],
    )

    assert key == "2026\x1f1\x1f3000"


@pytest.mark.unit
def test_values_equal_coerces_json_strings() -> None:
    assert shadow_compare._values_equal('{"id":"42"}', {"id": "42"})
    assert not shadow_compare._values_equal('{"id":"42"}', {"id": "43"})


@pytest.mark.unit
def test_values_equal_ignores_trailing_whitespace_on_strings() -> None:
    # ERP returns CHAR(N)-padded enum values ("FA   "); DM returns trimmed.
    assert shadow_compare._values_equal("FA   ", "FA")
    assert shadow_compare._values_equal("FA", "FA   ")
    assert shadow_compare._values_equal("FA   ", "FA   ")
    # Leading whitespace must still differentiate (it's significant).
    assert not shadow_compare._values_equal(" FA", "FA")
    # Different non-whitespace content must still mismatch.
    assert not shadow_compare._values_equal("FA", "TX")


@pytest.mark.unit
def test_normalize_erp_record_adds_journal_batchnumber_alias() -> None:
    out = shadow_compare._normalize_erp_record(
        "journal_transactions",
        {"module": "GL", "batchNbr": "B-1"},
    )

    assert out["batchNbr"] == "B-1"
    assert out["batchNumber"] == "B-1"


@pytest.mark.unit
def test_gl_balance_probe_query_defaults_to_current_period() -> None:
    cfg = shadow_compare.CandidateConfig(
        endpoint="general_ledger_balances_v2",
        bigquery_table="visma_general_ledger_balances_v2",
        api_endpoint="/v2/generalLedgerBalance",
        pagination_profile="finance_page_number",
        default_query={},
        required_query=[],
        partitioning=None,
        primary_key=["account.number"],
        datamart_query_file="config/queries/general_ledger_balance.graphql",
        datamart_root_field="generalLedgerBalancesV2",
        datamart_id_field="id",
        datamart_normalizer="normalize_general_ledger_balance",
        bronze_schema_path=Path("dummy.sqlx"),
        compare_fields=["account"],
    )

    params, note = shadow_compare._resolve_probe_query(None, cfg)  # type: ignore[arg-type]

    assert params is not None
    assert set(params) == {"periodId"}
    assert len(params["periodId"]) == 6
    assert note is None
