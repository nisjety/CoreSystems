# `accounts` Shadow Diff & Normalizer Draft

Status: **draft proposal — not yet applied**. Awaiting review before editing
the live normalizer, endpoint config, or shadow comparator.

## Inputs

- Shadow report: `docs/layeSystem/Layer1/datamart_shadow_compare.md`
- Normalizer: `tooling/src/vsv2_tooling/normalizers/account.py`
- Endpoint config: `config/endpoints/accounts.json`
- DM query: `config/queries/datamart/accounts.graphql`
- Bronze schema: `apps/visma_service/Dataform/definitions/bronze/visma/visma_accounts.sqlx`
- Comparator: `tooling/src/vsv2_tooling/discovery/shadow_compare.py`

## Live shadow result for `accounts`

| metric | value |
|---|---|
| status | `field_mismatch` |
| ERP rows fetched | 1210 |
| DataMart rows fetched | 100 |
| overlapping keys | 100 |
| only in ERP sample | 505 |
| only in DataMart sample | 0 |
| mismatched fields | `allowManualEntry`, `controlAccountModule`, `lastModifiedDateTime`, `metadata`, `postOption`, `timeStamp` |

Key alignment is good (100/100 overlap). The 6 mismatches are all
field-shape problems, not sampling problems.

## Per-field root cause

| field | DM source | ERP source | current normalizer | root cause | proposed action |
|---|---|---|---|---|---|
| `allowManualEntry` | **not exposed** by DM `Account` GraphQL | bool | None (no mapping) | DM gap | mark as known DM gap; exclude from compare |
| `controlAccountModule` | DM `controlAccountModule` | ERP `controlAccountModule` | passthrough | likely enum-value drift (DM `null` vs ERP `"None"`, or `"AP"` vs DM int) | coerce DM `None`/`""` → `"None"` to mirror ERP enum sentinel; needs sample-row confirmation |
| `lastModifiedDateTime` | **not exposed** by DM (only `updatedInDataMartDateTime`, which is the DM ingest timestamp, not the ERP last-modified timestamp) | ISO 8601 | currently falls back to `updatedInDataMartDateTime` — wrong | semantically different fields | drop the misleading fallback → leave `None`; mark as known DM gap; exclude from compare |
| `metadata` | DM has no per-row `metadata` blob | OData-style `metadata` object | currently synthesized from DM IDs | synthesized value is not equivalent to ERP envelope metadata | drop the synthesis → leave `None`; preserve the DM IDs under sidecar `_dm_*` keys for traceability |
| `postOption` | **not exposed** by DM `Account` GraphQL | string (`"D"`/`"S"`) | None (no mapping) | DM gap | mark as known DM gap; exclude from compare |
| `timeStamp` | DM has no row-version | ERP rowversion / etag (opaque base64) | currently set to `updatedInDataMartDateTime` — wrong | DM datetime ≠ ERP rowversion | drop the misleading mapping → leave `None`; mark as known DM gap; exclude from compare |

Summary: of 6 mismatches, **5 are unresolvable DM gaps** and 1
(`controlAccountModule`) is a likely enum-coercion fix that needs sample
confirmation.

## Proposed normalizer (account.py)

```python
"""Account normalizer.

DataMart Account → v1 visma_accounts bronze schema.

Known DM gaps (not derivable from the live DM `Account` GraphQL type and
therefore stay None in bronze; tracked separately as `compare_excludes`
in `config/endpoints/accounts.json`):

  - allowManualEntry        (DM does not expose)
  - postOption              (DM does not expose)
  - lastModifiedDateTime    (DM has updatedInDataMartDateTime, but that is
                             the DM ingest timestamp, NOT the ERP
                             last-modified timestamp)
  - timeStamp               (ERP timeStamp is an opaque rowversion;
                             DM has no equivalent)
  - metadata                (ERP returns an OData envelope per row; DM
                             has no per-row equivalent. The DM-only IDs
                             that used to be packed in here are now
                             carried as sidecar _dm_* fields.)

DM exposes externalCode1, externalCode2, analysisCode that v1's
`v1/account` REST endpoint does not. These live in the bronze record but
silver/gold should not consume them until Phase 8b.
"""
from __future__ import annotations

from typing import Any

from ._shared import first_non_empty, keep_legacy_fields

LEGACY_FIELDS = [
    "accountCD",
    "accountClass",
    "accountGroupCD",
    "accountID",
    "active",
    "allowManualEntry",
    "cashAccount",
    "controlAccountModule",
    "currency",
    "description",
    "lastModifiedDateTime",
    "metadata",
    "postOption",
    "publicCode1",
    "taxCategory",
    "timeStamp",
    "type",
    "useDefaultSub",
]


def _coerce_control_account_module(value: Any) -> Any:
    """ERP returns the string sentinel "None" for unset controlAccountModule.
    DM returns null. Mirror the ERP sentinel so the two sides compare equal."""
    if value in (None, ""):
        return "None"
    return value


def normalize_account(dm_record: dict[str, Any]) -> dict[str, Any]:
    normalized = {
        "accountCD": first_non_empty(dm_record, "accountCD", "accountCd"),
        "accountClass": dm_record.get("accountClassId"),
        "accountGroupCD": dm_record.get("groupCd"),
        "accountID": dm_record.get("accountId"),
        "active": dm_record.get("active"),
        "allowManualEntry": None,
        "cashAccount": dm_record.get("isCashAccount"),
        "controlAccountModule": _coerce_control_account_module(
            dm_record.get("controlAccountModule")
        ),
        "currency": dm_record.get("curyId"),
        "description": dm_record.get("description"),
        "lastModifiedDateTime": None,
        "metadata": None,
        "postOption": None,
        "publicCode1": dm_record.get("publicCode1"),
        "taxCategory": dm_record.get("taxCategoryId"),
        "timeStamp": None,
        "type": dm_record.get("type"),
        "useDefaultSub": dm_record.get("useDefaultSubaccount"),
        "externalCode1": dm_record.get("externalCode1"),
        "externalCode2": dm_record.get("externalCode2"),
        "analysisCode": dm_record.get("analysisCode"),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
        "_dm_company_id": dm_record.get("companyId"),
        "_dm_created_at": dm_record.get("createdInDataMartDateTime"),
        "_dm_id": dm_record.get("id"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
```

Net deltas vs current `account.py`:

1. Drops the `compact_json` import (no longer synthesizing `metadata`).
2. `lastModifiedDateTime` → `None` (no DM source).
3. `metadata` → `None`; DM IDs moved to `_dm_*` sidecar keys.
4. `timeStamp` → `None` (no DM source).
5. Adds `_coerce_control_account_module` to align with ERP `"None"` sentinel.
6. Adds `allowManualEntry: None` and `postOption: None` explicitly (was
   relying on `keep_legacy_fields` to backfill — explicit is clearer).
7. Docstring enumerates the 5 known DM gaps.

## Proposed `accounts.json` change

Add a `compare_excludes` list inside the `datamart` block so the shadow
comparator stops flagging the 5 known-gap fields as mismatches:

```json
"datamart": {
  ...
  "compare_excludes": [
    "allowManualEntry",
    "postOption",
    "lastModifiedDateTime",
    "metadata",
    "timeStamp"
  ]
}
```

`controlAccountModule` is **not** in the exclude list — we want the
comparator to confirm the enum coercion fix on the next run.

## Proposed `shadow_compare.py` change

Two small edits inside `_load_candidate_configs` to honor
`compare_excludes`:

```python
dm = data["datamart"]
...
exclude = set(dm.get("compare_excludes") or [])
bronze_fields = _business_bronze_fields(_load_bronze_schema_fields(bronze_schema_path))
...
compare_fields=[
    field for field in bronze_fields
    if field not in COMMON_SYSTEM_FIELDS and field not in exclude
],
```

Optional: also surface the excluded set in the report (a separate column
`known_dm_gaps`) so we don't lose visibility into the gap.

## Expected post-fix shadow result for `accounts`

- If `controlAccountModule` was the only real value mismatch: status
  becomes `clean_sample` with `mismatch_fields = []`.
- If `controlAccountModule` still mismatches after the coercion: it
  means DM and ERP genuinely return different enum strings — at that
  point we need one sample row pair to design the mapping.

## Open items for follow-up

- [ ] Confirm `controlAccountModule` enum values from a single
      sample-row pair (`vsv2-shadow-diff` per-row dump or one ad-hoc
      `gh api`/REST call).
- [ ] Decide whether the bronze schema should keep `metadata` /
      `timeStamp` / `postOption` / `allowManualEntry` columns once
      Layer 1 is fully DM-sourced, or drop them in a follow-up Dataform
      migration.
- [ ] Apply the same pattern to the next mismatch endpoint (`branches`)
      once this one lands.
