"""Schema guards for endpoint configuration fields used by shadow comparison."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft7Validator

ENDPOINTS_DIR = Path(__file__).resolve().parents[2] / "config" / "endpoints"
SCHEMA = json.loads((ENDPOINTS_DIR / "_schema.json").read_text(encoding="utf-8"))
VALIDATOR = Draft7Validator(SCHEMA)


@pytest.mark.unit
@pytest.mark.parametrize("endpoint", ["accounts", "branches"])
def test_shadow_compare_excludes_are_valid_endpoint_config(endpoint: str) -> None:
    config = json.loads((ENDPOINTS_DIR / f"{endpoint}.json").read_text(encoding="utf-8"))
    assert VALIDATOR.is_valid(config)


@pytest.mark.unit
@pytest.mark.parametrize(
    "invalid_excludes",
    ["name", [], [""], [" "], ["name", "name"], ["name", 42]],
)
def test_invalid_shadow_compare_excludes_are_rejected(
    invalid_excludes: object,
) -> None:
    config = json.loads((ENDPOINTS_DIR / "branches.json").read_text(encoding="utf-8"))
    config["datamart"] = {**config["datamart"], "compare_excludes": invalid_excludes}
    assert not VALIDATOR.is_valid(config)


@pytest.mark.unit
def test_unknown_datamart_config_property_is_rejected() -> None:
    config = json.loads((ENDPOINTS_DIR / "branches.json").read_text(encoding="utf-8"))
    config["datamart"] = {**config["datamart"], "unexpected_property": True}
    assert not VALIDATOR.is_valid(config)
