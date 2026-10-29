"""CI assertion: every normalizer's output schema matches the live bronze schema.

Used in CI and pre-deploy gates to block merges that drift from the existing v1
bronze contract. Phase 8b unlocks additive columns; pre-8b, any new field
emitted by a normalizer that is not already in the bronze sqlx declaration is
treated as a contract break.

Background
----------
Visma v1's "Bug E" was driven, in part, by silent column drift between source
data and bronze schema — `businessUnitId` was on `BLOCKED_SOURCE_FIELDS` so
source-side values were always NULL, and the MERGE join silently fell through.
This module is the contract-side companion to that fix: every normalizer's
output is checked against the bronze schema declaration so drift cannot land
without a deliberate Phase 8b change.

Bronze sqlx format
------------------
Each bronze table is a Dataform `type: "declaration"` file containing a
``//schema`` comment block followed by a JSON array of column descriptors:

    config { type: "declaration", schema: "...", name: "..." }
    //schema
    [ { "name": "field1", "type": "STRING", "mode": "", ... }, ... ]
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

# Fields added by the sync/orchestrator/load layer post-normalizer — these are
# never produced by the normalizer itself, so they are excluded from the
# "bronze has field, normalizer doesn't" check.
SYNC_INFRASTRUCTURE_FIELDS: frozenset[str] = frozenset(
    {
        "syncId",
        "syncedAt",
        "firstSyncedAt",
        "refreshedAt",
        "seenInFullScopeRuns",
        "consecutiveMissingFullRuns",
        "isDeleted",
        "deletedAt",
        "tenantId",
        "syncBatch",
        "organizationName",
    }
)

# Metadata injected by Rust's enrich() step into the normalizer's input record.
# Loader.py preserves these across reshape so they always appear in the final
# output, but they are not part of the bronze legacy contract — they live
# alongside it via _ensure_tenant_id().
ORCHESTRATOR_FIELDS: frozenset[str] = frozenset({"tenant_id", "_source", "_synced_at"})

# DataMart-lineage metadata emitted by normalizers themselves. These leading-
# underscore keys carry provenance from the DM source (when the record was
# last updated in DM, etc.) and are not part of the bronze legacy contract.
# Listed explicitly rather than wildcarded so new `_foo` fields cannot slip
# through silently without a deliberate addition to this set.
DM_LINEAGE_FIELDS: frozenset[str] = frozenset({"_updated_in_dm"})

# Combined "never-in-bronze" set used by the comparator.
_NON_BRONZE_FIELDS: frozenset[str] = ORCHESTRATOR_FIELDS | DM_LINEAGE_FIELDS


_SCHEMA_BLOCK_RE = re.compile(r"//\s*schema\s*(\[.*\])\s*\Z", re.DOTALL)


def _parse_bronze_sqlx_schema(path: Path) -> list[dict[str, Any]]:
    """Extract the schema declaration JSON array from a bronze sqlx file.

    Raises:
        FileNotFoundError: bronze sqlx not found at path.
        ValueError: no ``//schema`` block, or block is not valid JSON.
    """
    text = path.read_text(encoding="utf-8")
    match = _SCHEMA_BLOCK_RE.search(text)
    if not match:
        raise ValueError(
            f"no '//schema [...]' block found in {path}; bronze sqlx must declare its schema inline"
        )
    try:
        return json.loads(match.group(1))
    except json.JSONDecodeError as exc:
        raise ValueError(
            f"invalid JSON in '//schema' block at {path}: {exc.msg} "
            f"(line {exc.lineno}, col {exc.colno})"
        ) from exc


def _bronze_field_names(path: Path) -> set[str]:
    return {f["name"] for f in _parse_bronze_sqlx_schema(path)}


def assert_bronze_compat(
    *,
    normalizer_name: str,
    sample_dm_record: dict[str, Any],
    bronze_sqlx_path: Path,
    allow_additive: bool = False,
) -> list[str]:
    """Return a list of mismatched-field messages. Empty list = pass.

    Compares the keys emitted by ``REGISTRY[normalizer_name](sample_dm_record)``
    against the field names declared in ``bronze_sqlx_path``.

    Excluded from the comparison (never expected from a normalizer):
      * ``SYNC_INFRASTRUCTURE_FIELDS`` — written by the sync/orchestrator layer
      * ``ORCHESTRATOR_FIELDS`` — preserved through reshape; live alongside bronze

    Parameters
    ----------
    normalizer_name:
        Key into ``vsv2_tooling.normalizers.REGISTRY``.
    sample_dm_record:
        DataMart sample record to run through the normalizer. Should be
        representative of the entity (use a real captured payload).
    bronze_sqlx_path:
        Path to the bronze ``visma_<entity>.sqlx`` declaration file.
    allow_additive:
        Phase 8b flag — when True, normalizer-only fields are tolerated. Pre-8b
        and the safe default is False.

    Returns
    -------
    list[str]
        One message per mismatch. Empty list means the normalizer output is
        contract-compatible with the bronze schema declaration.
    """
    from vsv2_tooling.normalizers import REGISTRY

    fn = REGISTRY.get(normalizer_name)
    if fn is None:
        raise ValueError(f"Unknown normalizer: {normalizer_name!r}. Available: {sorted(REGISTRY)}")

    output = fn(sample_dm_record)
    if not isinstance(output, dict):
        return [f"{normalizer_name} returned {type(output).__name__}, expected dict"]

    output_fields = set(output.keys()) - _NON_BRONZE_FIELDS
    bronze_fields = _bronze_field_names(bronze_sqlx_path)
    bronze_business_fields = bronze_fields - SYNC_INFRASTRUCTURE_FIELDS

    messages: list[str] = []
    missing_from_output = sorted(bronze_business_fields - output_fields)
    for field in missing_from_output:
        messages.append(f"missing: bronze field {field!r} not produced by {normalizer_name}")

    if not allow_additive:
        extras_in_output = sorted(output_fields - bronze_business_fields)
        for field in extras_in_output:
            messages.append(
                f"extra: {normalizer_name} produced field {field!r} not in "
                f"{bronze_sqlx_path.name} (Phase 8b additive — gate behind allow_additive=True)"
            )

    return messages


# Backwards-compat alias for the original signature mentioned in audit notes.
def assert_bronze_schema_compatibility(
    *,
    normalizer_name: str,
    sample_dm_record: dict[str, Any],
    bronze_sqlx_path: Path,
    allow_additive: bool = False,
) -> list[str]:
    """Alias for :func:`assert_bronze_compat`."""
    return assert_bronze_compat(
        normalizer_name=normalizer_name,
        sample_dm_record=sample_dm_record,
        bronze_sqlx_path=bronze_sqlx_path,
        allow_additive=allow_additive,
    )
