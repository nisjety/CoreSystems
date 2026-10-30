from __future__ import annotations

from pathlib import Path

import pytest

from vsv2_tooling.discovery import review_candidates


@pytest.mark.unit
def test_extract_query_output_fields_uses_alias_names() -> None:
    query = """
    query Accounts($pageSize: Int!) {
      accounts(pageSize: $pageSize) {
        id
        accountCD: accountCd
        description
      }
    }
    """

    fields = review_candidates._extract_query_output_fields(query)

    assert fields == ["accountCD", "description", "id"]


@pytest.mark.unit
def test_load_bronze_schema_fields_reads_sqlx_declaration(tmp_path: Path) -> None:
    path = tmp_path / "visma_accounts.sqlx"
    path.write_text(
        'config { type: "declaration" }\n\n//schema\n[\n'
        '  {"name": "syncId"},\n'
        '  {"name": "tenantId"},\n'
        '  {"name": "accountCD"},\n'
        '  {"name": "description"}\n'
        "]\n"
    )

    fields = review_candidates._load_bronze_schema_fields(path)
    business = review_candidates._business_bronze_fields(fields)

    assert fields == ["syncId", "tenantId", "accountCD", "description"]
    assert business == ["accountCD", "description"]


@pytest.mark.unit
def test_classify_status_prioritizes_blockers() -> None:
    assert (
        review_candidates._classify_status(
            sample_error="boom",
            bronze_field_count=1,
            normalizer_name=None,
            normalizer_registered=False,
            normalizer_aux_params=[],
            missing_fields=[],
        )
        == "blocked_live_sample"
    )
    assert (
        review_candidates._classify_status(
            sample_error=None,
            bronze_field_count=1,
            normalizer_name="normalize_branch",
            normalizer_registered=False,
            normalizer_aux_params=[],
            missing_fields=["branchID"],
        )
        == "blocked_missing_normalizer"
    )
    assert (
        review_candidates._classify_status(
            sample_error=None,
            bronze_field_count=1,
            normalizer_name="normalize_customer_with_locations",
            normalizer_registered=True,
            normalizer_aux_params=["location_lookup"],
            missing_fields=[],
        )
        == "blocked_join_runtime_gap"
    )
    assert (
        review_candidates._classify_status(
            sample_error=None,
            bronze_field_count=1,
            normalizer_name=None,
            normalizer_registered=False,
            normalizer_aux_params=[],
            missing_fields=["fieldA"],
        )
        == "blocked_no_normalizer_and_parity_gap"
    )
    assert (
        review_candidates._classify_status(
            sample_error=None,
            bronze_field_count=0,
            normalizer_name=None,
            normalizer_registered=False,
            normalizer_aux_params=[],
            missing_fields=[],
        )
        == "blocked_bronze_schema_missing"
    )
