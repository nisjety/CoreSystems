"""Unit tests for endpoint key parsing and validation."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from vsv2_tooling.dataform.endpoint_keys import (
    EndpointKeyValidationError,
    parse_endpoint_keys,
    validate_all_endpoint_configs,
    validate_endpoint_config_file,
)

ENDPOINTS_DIR = Path(
    "/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen "
    "/apps/visma_service/visma_service_v2/config/endpoints"
)


@pytest.mark.unit
def test_parse_primary_key_string() -> None:
    keys = parse_endpoint_keys({"endpoint_name": "x", "primary_key": "id"})
    assert keys.primary_key == ("id",)
    assert keys.logical_keys is None


@pytest.mark.unit
def test_parse_primary_key_csv_string() -> None:
    keys = parse_endpoint_keys(
        {
            "endpoint_name": "x",
            "primary_key": "tenantId, accountCD , currencyId",
        }
    )
    assert keys.primary_key == ("tenantId", "accountCD", "currencyId")


@pytest.mark.unit
def test_parse_primary_key_array() -> None:
    keys = parse_endpoint_keys(
        {
            "endpoint_name": "x",
            "primary_key": ["a", "b"],
        }
    )
    assert keys.primary_key == ("a", "b")


@pytest.mark.unit
def test_parse_primary_key_rejects_empty() -> None:
    with pytest.raises(EndpointKeyValidationError, match="primary_key must contain"):
        parse_endpoint_keys({"endpoint_name": "x", "primary_key": ""})


@pytest.mark.unit
def test_parse_primary_key_rejects_wrong_type() -> None:
    with pytest.raises(EndpointKeyValidationError, match="primary_key must be string or array"):
        parse_endpoint_keys({"endpoint_name": "x", "primary_key": 123})


@pytest.mark.unit
def test_parse_missing_primary_key() -> None:
    with pytest.raises(EndpointKeyValidationError, match="primary_key is required"):
        parse_endpoint_keys({"endpoint_name": "x"})


@pytest.mark.unit
def test_parse_missing_endpoint_name() -> None:
    with pytest.raises(EndpointKeyValidationError, match="endpoint_name is required"):
        parse_endpoint_keys({"primary_key": "id"})


@pytest.mark.unit
def test_logical_keys_must_be_superset_of_primary_key() -> None:
    with pytest.raises(EndpointKeyValidationError, match="primary_key components missing"):
        parse_endpoint_keys(
            {
                "endpoint_name": "gl_transactions",
                "primary_key": "tenantId,batchNumber,lineNumber",
                "logical_keys": ["tenantId", "batchNumber"],  # missing lineNumber
            }
        )


@pytest.mark.unit
def test_logical_keys_clean_match() -> None:
    keys = parse_endpoint_keys(
        {
            "endpoint_name": "gl_transactions",
            "primary_key": "tenantId,batchNumber,lineNumber",
            "logical_keys": ["tenantId", "businessUnitId", "batchNumber", "lineNumber"],
        }
    )
    assert keys.primary_key == ("tenantId", "batchNumber", "lineNumber")
    assert keys.logical_keys == ("tenantId", "businessUnitId", "batchNumber", "lineNumber")


@pytest.mark.unit
def test_logical_keys_documents_bug_e_lesson() -> None:
    """The shape we expect for gl_transactions post-Bug-E:
    logical_keys includes businessUnitId; primary_key excludes it because
    businessUnitId can be NULL. This is the explicit form of v1's hard-won
    fix."""
    keys = parse_endpoint_keys(
        {
            "endpoint_name": "general_ledger_transactions",
            "primary_key": ["tenantId", "batchNumber", "lineNumber"],
            "logical_keys": ["tenantId", "businessUnitId", "batchNumber", "lineNumber"],
        }
    )
    excluded_from_merge = set(keys.logical_keys or ()) - set(keys.primary_key)
    assert "businessUnitId" in excluded_from_merge
    assert keys.merge_on_columns == ("tenantId", "batchNumber", "lineNumber")


@pytest.mark.unit
def test_logical_keys_duplicates_rejected() -> None:
    with pytest.raises(EndpointKeyValidationError, match="duplicate"):
        parse_endpoint_keys(
            {
                "endpoint_name": "x",
                "primary_key": "id",
                "logical_keys": ["id", "id", "extra"],
            }
        )


@pytest.mark.unit
def test_logical_keys_empty_rejected() -> None:
    with pytest.raises(EndpointKeyValidationError, match="at least one field"):
        parse_endpoint_keys(
            {
                "endpoint_name": "x",
                "primary_key": "id",
                "logical_keys": [],
            }
        )


@pytest.mark.unit
def test_logical_keys_wrong_type_rejected() -> None:
    with pytest.raises(EndpointKeyValidationError, match="must be an array"):
        parse_endpoint_keys(
            {
                "endpoint_name": "x",
                "primary_key": "id",
                "logical_keys": "id",
            }
        )


@pytest.mark.unit
def test_validate_endpoint_config_file_bad_json(tmp_path: Path) -> None:
    path = tmp_path / "broken.json"
    path.write_text("{not valid json}", encoding="utf-8")
    with pytest.raises(EndpointKeyValidationError, match="invalid JSON"):
        validate_endpoint_config_file(path)


@pytest.mark.unit
def test_validate_endpoint_config_file_real_config(tmp_path: Path) -> None:
    path = tmp_path / "widgets.json"
    path.write_text(
        json.dumps({"endpoint_name": "widgets", "primary_key": "widgetId"}),
        encoding="utf-8",
    )
    keys = validate_endpoint_config_file(path)
    assert keys.endpoint_name == "widgets"
    assert keys.primary_key == ("widgetId",)


@pytest.mark.unit
def test_validate_all_endpoint_configs_skips_schema_file(tmp_path: Path) -> None:
    (tmp_path / "_schema.json").write_text("{}", encoding="utf-8")
    (tmp_path / "widgets.json").write_text(
        json.dumps({"endpoint_name": "widgets", "primary_key": "id"}),
        encoding="utf-8",
    )
    results = validate_all_endpoint_configs(tmp_path)
    assert len(results) == 1
    assert results[0].endpoint_name == "widgets"


@pytest.mark.unit
def test_validate_all_real_endpoint_configs_pass() -> None:
    """Every config under config/endpoints/ should pass key validation today.
    This is a regression guard — drift in any file fails CI."""
    if not ENDPOINTS_DIR.exists():
        pytest.skip(f"endpoints dir not available at {ENDPOINTS_DIR}")
    results = validate_all_endpoint_configs(ENDPOINTS_DIR)
    # Spot-check: there should be a lot of them, all parseable.
    assert len(results) >= 50, f"expected at least 50 configs, got {len(results)}"
    names = {r.endpoint_name for r in results}
    assert "customers" in names
    assert "accounts" in names
