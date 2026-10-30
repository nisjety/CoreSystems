"""Endpoint key parsing and consistency validation.

`primary_key` and `logical_keys` are the v2 way of making the Bug E lesson
explicit in configuration:

* ``primary_key`` is what enters the SQL MERGE ON-clause. Every component must
  be present and non-NULL on every record.
* ``logical_keys`` is the canonical natural-key set, optionally including
  parts that are intentionally NOT in the MERGE ON-clause (e.g. tenant-
  scoping keys, optional sub-identifiers). Every ``primary_key`` component
  must appear in ``logical_keys`` if it is set.

The validator is run by CI (test_endpoint_keys.py) and by the orchestrator at
startup. Mis-configured endpoints are surfaced as
``EndpointKeyValidationError`` with a remediation hint.
"""

from __future__ import annotations

import json
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class EndpointKeyValidationError(ValueError):
    """Raised when an endpoint config's key declarations are inconsistent."""


@dataclass(frozen=True)
class EndpointKeys:
    """Parsed view of an endpoint's key configuration."""

    endpoint_name: str
    primary_key: tuple[str, ...]
    logical_keys: tuple[str, ...] | None

    @property
    def merge_on_columns(self) -> tuple[str, ...]:
        """Columns that enter the MERGE ON clause. Alias for primary_key."""
        return self.primary_key


def _parse_primary_key_value(value: Any) -> tuple[str, ...]:
    if isinstance(value, str):
        keys = tuple(part.strip() for part in value.split(",") if part.strip())
    elif isinstance(value, list):
        keys = tuple(str(item).strip() for item in value if str(item).strip())
    else:
        raise EndpointKeyValidationError(
            f"primary_key must be string or array, got {type(value).__name__}"
        )
    if not keys:
        raise EndpointKeyValidationError("primary_key must contain at least one field")
    return keys


def parse_endpoint_keys(config: dict[str, Any]) -> EndpointKeys:
    """Parse and validate the key fields of an endpoint config.

    Raises:
        EndpointKeyValidationError: any structural problem or inconsistency.
    """
    endpoint_name = config.get("endpoint_name")
    if not isinstance(endpoint_name, str) or not endpoint_name:
        raise EndpointKeyValidationError("endpoint_name is required and must be a non-empty string")

    if "primary_key" not in config:
        raise EndpointKeyValidationError(f"{endpoint_name}: primary_key is required")
    primary_key = _parse_primary_key_value(config["primary_key"])

    logical_keys_raw = config.get("logical_keys")
    logical_keys: tuple[str, ...] | None
    if logical_keys_raw is None:
        logical_keys = None
    else:
        if not isinstance(logical_keys_raw, list):
            raise EndpointKeyValidationError(f"{endpoint_name}: logical_keys must be an array")
        logical_keys = tuple(str(k).strip() for k in logical_keys_raw if str(k).strip())
        if not logical_keys:
            raise EndpointKeyValidationError(
                f"{endpoint_name}: logical_keys, if present, must contain at least one field"
            )
        if len(logical_keys) != len(set(logical_keys)):
            raise EndpointKeyValidationError(
                f"{endpoint_name}: logical_keys contains duplicate entries"
            )
        # Every primary_key component must appear in logical_keys.
        missing = sorted(set(primary_key) - set(logical_keys))
        if missing:
            raise EndpointKeyValidationError(
                f"{endpoint_name}: primary_key components missing from logical_keys: "
                f"{missing}. logical_keys must be a superset of primary_key."
            )

    return EndpointKeys(
        endpoint_name=endpoint_name,
        primary_key=primary_key,
        logical_keys=logical_keys,
    )


def validate_endpoint_config_file(path: Path) -> EndpointKeys:
    """Load a single endpoint JSON file and validate its key configuration."""
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise EndpointKeyValidationError(
            f"{path.name}: invalid JSON: {exc.msg} (line {exc.lineno})"
        ) from exc
    return parse_endpoint_keys(config)


def validate_all_endpoint_configs(
    endpoints_dir: Path,
    *,
    skip: Iterable[str] = ("_schema.json",),
) -> list[EndpointKeys]:
    """Validate every endpoint JSON in ``endpoints_dir`` and return parsed keys.

    Raises ``EndpointKeyValidationError`` on the first failure, with the path
    and reason. Each file is read independently — no cross-file consistency
    checks beyond this scope.
    """
    skip_set = set(skip)
    results: list[EndpointKeys] = []
    for path in sorted(endpoints_dir.glob("*.json")):
        if path.name in skip_set:
            continue
        results.append(validate_endpoint_config_file(path))
    return results
