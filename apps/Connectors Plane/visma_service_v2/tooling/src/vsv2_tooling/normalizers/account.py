"""Account normalizer.

DataMart Account -> v1 visma_accounts bronze schema.

Known DM gaps (not derivable from the live DM `Account` GraphQL type;
they stay None in bronze and are listed in
`config/endpoints/accounts.json` under `datamart.compare_excludes` so the
shadow comparator doesn't keep flagging them):

  - allowManualEntry        DM does not expose this field
  - postOption              DM does not expose this field
  - lastModifiedDateTime    DM only has updatedInDataMartDateTime, which
                            is the DM ingest timestamp, not the ERP
                            last-modified timestamp - semantically
                            different, so we no longer use it as a
                            stand-in
  - timeStamp               ERP timeStamp is an opaque rowversion / etag;
                            DM has no equivalent
  - metadata                ERP returns an OData envelope per row; DM has
                            no per-row equivalent. The DM-only IDs that
                            used to be packed in here are now carried as
                            sidecar `_dm_*` fields for traceability.

DM exposes `externalCode1`, `externalCode2`, `analysisCode` that v1's
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


def normalize_account(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart Account to v1 visma_accounts bronze schema."""
    normalized: dict[str, Any] = {
        "accountCD": first_non_empty(dm_record, "accountCD", "accountCd"),
        "accountClass": dm_record.get("accountClassId"),
        "accountGroupCD": dm_record.get("groupCd"),
        "accountID": dm_record.get("accountId"),
        "active": dm_record.get("active"),
        "allowManualEntry": None,
        "cashAccount": dm_record.get("isCashAccount"),
        # ERP stores enum codes as CHAR(N) padded with trailing spaces
        # (e.g. `'FA   '`) while DM returns the trimmed value. We keep the
        # DM value as-is; whitespace-insensitive equality in the shadow
        # comparator handles the CHAR vs VARCHAR drift.
        "controlAccountModule": dm_record.get("controlAccountModule"),
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
