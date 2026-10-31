from __future__ import annotations

import json
from pathlib import Path

import pytest
from graphql import parse, validate

from vsv2_tooling.discovery import validate_queries
from vsv2_tooling.discovery.generate_queries import _load_catalog
from vsv2_tooling.discovery.validate_queries import _build_schema, _validate_one


@pytest.mark.unit
def test_build_schema_ignores_unsupported_directive_locations(tmp_path: Path) -> None:
    source = Path("core/queries/_generated/datamart.introspection.json")
    payload = json.loads(source.read_text())
    schema = payload.get("__schema") or payload["data"]["__schema"]
    directive = next(item for item in schema["directives"] if item["name"] == "deprecated")
    directive["locations"] = [*directive["locations"], "DIRECTIVE_DEFINITION"]
    path = tmp_path / "schema.json"
    path.write_text(json.dumps(payload))

    result = _build_schema(path)

    assert result.query_type is not None
    assert "DIRECTIVE_DEFINITION" in directive["locations"]


@pytest.mark.unit
def test_committed_datamart_queries_match_committed_schema() -> None:
    schema = _build_schema(Path("core/queries/_generated/datamart.introspection.json"))
    query_files = sorted(Path("config/queries").rglob("*.graphql"))
    errors = {
        str(path): [
            str(error).splitlines()[0] for error in validate(schema, parse(path.read_text()))
        ]
        for path in query_files
    }
    invalid = {path: issues for path, issues in errors.items() if issues}
    assert not invalid


@pytest.mark.unit
def test_query_defaults_satisfy_required_schema_arguments() -> None:
    snapshot = Path("core/queries/_generated/datamart.introspection.json")
    schema = _build_schema(snapshot)
    fields, _ = _load_catalog(snapshot)
    by_name = {field.root_name: field for field in fields}

    budgets = _validate_one(
        schema,
        Path("config/queries/datamart/budgets.graphql"),
        by_name["budgets"],
    )
    by_period = _validate_one(
        schema,
        Path("config/queries/datamart/general_ledger_balances_by_period.graphql"),
        by_name["generalLedgerBalancesByPeriod"],
    )

    assert budgets["compatibility"] == "paginated_compatible"
    assert "includeUnreleased" in budgets["optional_extra_args"]
    assert by_period["compatibility"] == "requires_runtime_params"
    assert by_period["required_extra_args"] == ["finPeriodId"]


@pytest.mark.unit
def test_page_wrapper_is_not_classified_as_point_lookup() -> None:
    snapshot = Path("core/queries/_generated/datamart.introspection.json")
    schema = _build_schema(snapshot)
    fields, _ = _load_catalog(snapshot)
    forward = next(
        field for field in fields if field.root_name == "generalLedgerBalancesForwardByGroupV2"
    )

    result = _validate_one(
        schema,
        Path("config/queries/datamart/general_ledger_balances_forward_by_group_v2.graphql"),
        forward,
    )

    assert result["compatibility"] == "requires_page_adapter"


@pytest.mark.unit
def test_validate_queries_reports_invalid_fields(tmp_path: Path) -> None:
    query_dir = tmp_path / "queries"
    query_dir.mkdir()
    (query_dir / "branches.graphql").write_text("query { branches(pageSize: 1) { id } }")
    (query_dir / "customers.graphql").write_text(
        "query { customers(pageSize: 1) { fieldThatDoesNotExist } }"
    )
    report = tmp_path / "report.md"

    with pytest.raises(SystemExit, match="1/2"):
        validate_queries.run(
            introspection_path="core/queries/_generated/datamart.introspection.json",
            query_dir=str(query_dir),
            report_path=str(report),
        )

    content = report.read_text()
    assert "schema-valid files: 1" in content
    assert "Cannot query field 'fieldThatDoesNotExist'" in content
