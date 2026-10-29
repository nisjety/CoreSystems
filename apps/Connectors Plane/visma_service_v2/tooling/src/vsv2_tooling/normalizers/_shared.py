"""Shared helpers for DM → bronze shape normalizers."""

from __future__ import annotations

import json
import re
from collections.abc import Mapping
from typing import Any


def first_non_empty(record: dict[str, Any], *keys: str) -> Any:
    """Return the first non-empty value among keys (mirrors v1
    `VismaAPIClientBase._first_non_empty_value`)."""
    for key in keys:
        value = record.get(key)
        if value not in (None, "", []):
            return value
    return None


def keep_legacy_fields(target: dict[str, Any], legacy_fields: list[str]) -> None:
    """Ensure every legacy bronze field is present (None when DM doesn't expose it)."""
    for field in legacy_fields:
        target.setdefault(field, None)


def stringify_scalar(value: Any) -> str | None:
    """Convert scalar identifiers to strings while preserving null semantics."""
    if value in (None, ""):
        return None
    return str(value)


def compact_json(value: Any) -> str | None:
    """Serialize JSON-like values using a stable compact representation.

    Empty dicts become ``None``; empty lists stay ``[]`` because bronze contracts
    often use them as explicit "no nested rows" sentinels.
    """
    if isinstance(value, str):
        stripped = value.strip()
        return stripped or None
    cleaned = _clean_jsonish(value)
    if cleaned is None:
        return None
    return json.dumps(cleaned, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _clean_jsonish(value: Any) -> Any:
    if value in (None, ""):
        return None
    if isinstance(value, Mapping):
        cleaned = {
            key: cleaned_value
            for key, raw_value in value.items()
            if (cleaned_value := _clean_jsonish(raw_value)) is not None
        }
        return cleaned or None
    if isinstance(value, list):
        return [
            cleaned_value
            for raw_value in value
            if (cleaned_value := _clean_jsonish(raw_value)) is not None
        ]
    return value


_ISO_LOCAL = re.compile(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d+))?Z?$")


def erp_local_timestamp(value: Any) -> Any:
    """Render a DataMart timestamp in the ERP REST wire format.

    The ERP returns Europe/Oslo wall-clock time with no zone designator and no trailing
    zeros (``2026-09-29T16:59:39.63``). DataMart's ``lastModifiedDateTime`` carries the same
    wall-clock time but appends ``Z`` - a mislabel, measured against the true-UTC
    ``updatedInDataMartDateTime`` (local is 1h ahead in winter, 2h in summer). Stripping the
    ``Z`` keeps bronze byte-compatible with rows written from the ERP.

    Values that are not ISO timestamps are returned unchanged; empty values become None.
    """
    if value in (None, ""):
        return None
    if not isinstance(value, str):
        return value
    match = _ISO_LOCAL.match(value.strip())
    if match is None:
        return value
    head, fraction = match.groups()
    fraction = (fraction or "").rstrip("0")
    return f"{head}.{fraction}" if fraction else head
