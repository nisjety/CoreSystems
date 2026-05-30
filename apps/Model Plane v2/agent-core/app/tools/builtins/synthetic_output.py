"""SyntheticOutputTool — produce Pydantic-validated structured JSON output.

Useful when the LLM is asked to return a typed response (e.g. a form,
a classification result, a structured report) and the caller needs
schema-validated JSON rather than freeform text.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from pydantic import BaseModel, ValidationError, create_model
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)


class SyntheticOutputTool:
    """Emit structured, schema-validated JSON output.

    The LLM provides:
    - A JSON Schema (as a dict) describing the expected structure.
    - A data dict that must validate against that schema.

    The tool validates the data and returns it serialised as JSON.
    If validation fails, the tool returns an error with details so
    the LLM can correct its output.
    """

    name = "synthetic_output"
    description = (
        "Emit validated structured JSON output. "
        "Provide a JSON Schema and matching data — the tool validates and serialises the result. "
        "Use when the response must conform to a strict schema (forms, structured reports, "
        "classifications, API input objects)."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "schema": {
                "type": "object",
                "description": (
                    "JSON Schema (draft-07) describing the output structure. "
                    "Must be a valid schema object with 'type' and 'properties'."
                ),
            },
            "data": {
                "type": "object",
                "description": "The data object to validate and emit.",
            },
            "label": {
                "type": "string",
                "description": "Optional label for this output (e.g. 'user_profile', 'report').",
                "default": "output",
            },
        },
        "required": ["schema", "data"],
    }
    search_hint = "structured output json schema validate typed response"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Emit validated structured JSON. Use when you need to return typed data "
            "that must conform to a strict schema."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        schema = input_data.get("schema")
        if not isinstance(schema, dict):
            raise ValueError("'schema' must be a JSON Schema object (dict)")
        data = input_data.get("data")
        if not isinstance(data, dict):
            raise ValueError("'data' must be a JSON object (dict)")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        schema = input_data["schema"]
        data = input_data["data"]
        label = input_data.get("label", "output")

        errors = _validate_against_schema(schema, data)
        if errors:
            return ToolResult(
                error=f"Schema validation failed for '{label}': {'; '.join(errors)}",
                metadata={"label": label, "errors": errors},
            )

        try:
            serialised = json.dumps(data, ensure_ascii=False, indent=2)
        except (TypeError, ValueError) as exc:
            return ToolResult(error=f"JSON serialisation failed: {exc}")

        logger.debug("synthetic_output_ok", extra={"label": label})
        return ToolResult(
            output=serialised,
            metadata={"label": label, "fields": list(data.keys())},
        )


def _validate_against_schema(schema: dict[str, Any], data: dict[str, Any]) -> list[str]:
    """Lightweight JSON Schema validation — covers required fields and types.

    We deliberately avoid pulling in jsonschema as a dependency here.
    A full JSON Schema validator can replace this if the stack adds it.
    """
    errors: list[str] = []
    schema_type = schema.get("type")
    if schema_type and schema_type != "object":
        return [f"Top-level schema type must be 'object', got '{schema_type}'"]

    # Required fields
    required = schema.get("required", [])
    for field in required:
        if field not in data:
            errors.append(f"Missing required field: '{field}'")

    # Type checks on properties
    properties = schema.get("properties", {})
    for key, prop_schema in properties.items():
        if key not in data:
            continue
        value = data[key]
        expected_type = prop_schema.get("type")
        if expected_type and not _check_type(value, expected_type):
            errors.append(
                f"Field '{key}' expected type '{expected_type}', got '{type(value).__name__}'"
            )

    return errors


_TYPE_MAP: dict[str, type | tuple[type, ...]] = {
    "string": str,
    "integer": int,
    "number": (int, float),
    "boolean": bool,
    "array": list,
    "object": dict,
    "null": type(None),
}


def _check_type(value: Any, expected: str) -> bool:
    expected_py = _TYPE_MAP.get(expected)
    if expected_py is None:
        return True  # unknown type — pass through
    return isinstance(value, expected_py)
