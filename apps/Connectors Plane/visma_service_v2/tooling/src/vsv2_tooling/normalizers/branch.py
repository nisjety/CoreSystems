"""Branch normalizer.

DataMart Branch -> v1 visma_branches bronze schema.

Known DM gaps (not derivable from the live DM `Branch` GraphQL type;
they stay None in bronze and are listed in
`config/endpoints/branches.json` under `datamart.compare_excludes` so
the shadow comparator doesn't keep flagging them):

  - name                    DM now exposes nullable branchName, but this
                            is the business-account name and may differ
                            from the ERP legal name.
  - organizationId          ERP returns a legal-entity / organization
                            id (e.g. 1). DM exposes bAccountId (e.g.
                            2458), which is a different concept (the
                            internal business account id, not the legal
                            entity id). Previously bAccountId was
                            written to organizationId; that was wrong.
                            bAccountId is now preserved on a sidecar
                            `_dm_b_account_id` key.
  - corporateId             ERP returns the legal entity / orgnr; DM
                            does not expose it.
  - vatRegistrationId       ERP returns the VAT registration string;
                            DM does not expose it.
  - lastModifiedDateTime    DM only has updatedInDataMartDateTime,
                            which is the DM ingest timestamp, not the
                            ERP last-modified timestamp.
"""

from __future__ import annotations

from typing import Any

from ._shared import first_non_empty, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "branchId",
    "corporateId",
    "isActive",
    "isMainBranch",
    "lastModifiedDateTime",
    "name",
    "number",
    "organizationId",
    "vatRegistrationId",
]


def normalize_branch(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart Branch rows to the legacy bronze contract."""
    branch_number = first_non_empty(
        dm_record,
        "branchCode",
        "branchId",
        "id",
    )
    normalized: dict[str, Any] = {
        "branchId": dm_record.get("branchId"),
        "corporateId": None,
        "isActive": dm_record.get("active"),
        "isMainBranch": dm_record.get("mainBranch"),
        "lastModifiedDateTime": None,
        "name": dm_record.get("branchName"),
        "number": stringify_scalar(branch_number),
        "organizationId": None,
        "vatRegistrationId": None,
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
        "_dm_id": dm_record.get("id"),
        "_dm_company_id": dm_record.get("companyId"),
        "_dm_b_account_id": dm_record.get("bAccountId"),
        "_dm_ledger_id": dm_record.get("ledgerId"),
        "_dm_parent_branch_id": dm_record.get("parentBranchId"),
        "_dm_country_id": dm_record.get("countryId"),
        "_dm_created_at": dm_record.get("createdInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
