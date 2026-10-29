"""The `open_set` flag of the endpoint configs: what the schema accepts, what the validator adds
(the rules of the Rust registry), and who carries the flag."""

from __future__ import annotations

import copy
import inspect
import json
import re
import shutil
from pathlib import Path
from typing import Any

import pytest
from jsonschema import Draft7Validator

from vsv2_tooling.config_validate import open_set_problem, validate_configs

REPO = Path(__file__).resolve().parents[2]
ENDPOINTS_DIR = REPO / "config" / "endpoints"
VALIDATOR = Draft7Validator(json.loads((ENDPOINTS_DIR / "_schema.json").read_text("utf-8")))
FLAGGED = ["customer_invoices", "supplier_invoices"]
# Read by the registry tests in core/crates/vsv2-orchestrator/src/registry.rs as well.
SHARED_CASES = json.loads(
    (Path(__file__).resolve().parent / "fixtures" / "open_set_cases.json").read_text("utf-8")
)


def load(name: str) -> dict[str, Any]:
    return json.loads((ENDPOINTS_DIR / f"{name}.json").read_text(encoding="utf-8"))


def errors(config: dict[str, Any]) -> list[str]:
    return [error.message for error in VALIDATOR.iter_errors(config)]


@pytest.mark.unit
@pytest.mark.parametrize("name", FLAGGED)
def test_the_invoice_endpoints_are_flagged_and_valid(name: str) -> None:
    config = load(name)

    assert config["open_set"] is True
    assert errors(config) == []


@pytest.mark.unit
def test_no_other_endpoint_is_flagged() -> None:
    flagged = sorted(
        path.stem
        for path in ENDPOINTS_DIR.glob("*.json")
        if not path.stem.startswith("_") and load(path.stem).get("open_set")
    )

    assert flagged == FLAGGED


@pytest.mark.unit
def test_the_flag_is_optional_and_off_by_default() -> None:
    config = load("accounts")

    assert "open_set" not in config
    assert errors(config) == []


@pytest.mark.unit
@pytest.mark.parametrize("value", ["true", 1, None, [], {}])
def test_the_flag_must_be_a_boolean(value: object) -> None:
    config = load("customer_invoices")
    config["open_set"] = value

    assert errors(config)


@pytest.mark.unit
def test_an_unflagged_or_false_flag_puts_no_extra_demands_on_the_endpoint() -> None:
    config = load("accounts")
    config["open_set"] = False
    config["tier"] = 3

    assert errors(config) == []


@pytest.mark.unit
@pytest.mark.parametrize(
    ("change", "complaint"),
    [
        ({"tier": 2}, "1 was expected"),
        ({"source": "datamart_graphql"}, "'erp_rest' was expected"),
        ({"pagination_profile": "salesorder_page_index"}, "'finance_page_number' was expected"),
        (
            {"period_scan": {"first_period": "202301", "query_params": ["periodId"]}},
            "is not of type 'null'",
        ),
        (
            {
                "partitioning": {
                    "dimensions": [
                        {
                            "query_param": "ledger",
                            "source_endpoint": "ledgers",
                            "source_path": "ledgerId",
                        }
                    ]
                }
            },
            "is not of type 'null'",
        ),
    ],
)
def test_a_flagged_endpoint_must_be_one_the_reconciliation_can_serve(
    change: dict[str, Any], complaint: str
) -> None:
    flagged = {**copy.deepcopy(load("customer_invoices")), **change}
    unflagged = {key: value for key, value in flagged.items() if key != "open_set"}

    assert errors(unflagged) == [], "valid on its own: it is the flag that is refused"
    found = errors(flagged)
    assert found, f"{change} should be refused"
    assert any(complaint in message for message in found), found


@pytest.mark.unit
@pytest.mark.parametrize("missing", ["tier", "api_endpoint"])
def test_a_flagged_endpoint_needs_its_tier_and_its_path(missing: str) -> None:
    config = load("customer_invoices")
    del config[missing]

    found = errors(config)

    assert any(missing in message for message in found), found


# ─── the rules of the Rust registry, in `vsv2-validate-config` ─────────────────────────────────
#
# The registry refuses a flagged config that the reconciliation cannot serve when it loads, which
# stops every process that reads config/endpoints. The validator has to refuse it first.


def case_config(case: dict[str, Any]) -> dict[str, Any]:
    """The real config the shared cases start from, changed the way `case` says."""
    config = copy.deepcopy(load(SHARED_CASES["base"]))
    config.update(copy.deepcopy(case.get("set", {})))
    for key in case.get("remove", []):
        config.pop(key, None)
    return config


def case_ids(cases: list[dict[str, Any]]) -> list[str]:
    return [case["name"] for case in cases]


@pytest.fixture
def validated(tmp_path: Path):
    """Run `validate_configs` over one config, in a throwaway config/endpoints with the schema."""

    def run(config: dict[str, Any]) -> list[tuple[str, str]]:
        root = tmp_path / "repo"
        directory = root / "config" / "endpoints"
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copy(ENDPOINTS_DIR / "_schema.json", directory / "_schema.json")
        (directory / f"{config['endpoint_name']}.json").write_text(json.dumps(config))
        failures, _ = validate_configs(directory, root)
        return failures

    return run


@pytest.mark.unit
@pytest.mark.parametrize("case", SHARED_CASES["cases"], ids=case_ids(SHARED_CASES["cases"]))
def test_the_validator_answers_what_the_shared_cases_say(case: dict[str, Any]) -> None:
    assert open_set_problem(case_config(case)) == case["problem"]


@pytest.mark.unit
@pytest.mark.parametrize("case", SHARED_CASES["cases"], ids=case_ids(SHARED_CASES["cases"]))
def test_make_validate_config_reports_the_problem_and_only_for_a_config_that_has_one(
    case: dict[str, Any], validated: Any
) -> None:
    config = case_config(case)

    failures = validated(config)

    if case["problem"] is None:
        assert failures == []
    else:
        assert (config["endpoint_name"] + ".json", case["problem"]) in failures, failures


@pytest.mark.unit
@pytest.mark.parametrize("name", FLAGGED)
def test_the_real_flagged_configs_have_no_open_set_problem(name: str) -> None:
    assert open_set_problem(load(name)) is None


@pytest.mark.unit
def test_every_reason_the_validator_can_give_has_a_shared_case() -> None:
    reasons = set(re.findall(r'fail\("([^"]+)"\)', inspect.getsource(open_set_problem)))
    covered = {
        case["problem"].removeprefix("open_set needs ")
        for case in SHARED_CASES["cases"]
        if case["problem"] is not None
    }

    assert reasons, "the pattern no longer finds the rules in open_set_problem"
    assert reasons <= covered, f"no shared case for: {sorted(reasons - covered)}"


@pytest.mark.unit
def test_a_tier_that_is_true_is_not_tier_1() -> None:
    config = {**load("customer_invoices"), "tier": True}

    assert open_set_problem(config) == "open_set needs tier 1: only the hourly pass runs it"


@pytest.mark.unit
@pytest.mark.parametrize("flag", [False, None, "true", 1, 0, [], {}])
def test_only_a_flag_that_is_exactly_true_is_checked(flag: object) -> None:
    config = {**load("customer_invoices"), "open_set": flag, "tier": 4, "source": "expense_api"}

    assert open_set_problem(config) is None


@pytest.mark.unit
@pytest.mark.parametrize("data", [None, [], "text", 7])
def test_a_config_that_is_not_an_object_has_no_open_set_problem(data: object) -> None:
    assert open_set_problem(data) is None


@pytest.mark.unit
def test_a_flagged_config_that_breaks_a_rule_is_a_failure_of_the_file_not_a_crash(
    validated: Any,
) -> None:
    config = {**load("customer_invoices"), "api_endpoint": "/v1/customerinvoice/{id}"}

    failures = validated(config)

    assert (
        "customer_invoices.json",
        "open_set needs an api_endpoint without placeholders or a query string",
    ) in failures
