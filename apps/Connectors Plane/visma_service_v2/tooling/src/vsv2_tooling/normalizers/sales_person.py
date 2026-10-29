"""Sales person normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import (
    compact_json,
    erp_local_timestamp,
    first_non_empty,
    keep_legacy_fields,
    stringify_scalar,
)

LEGACY_FIELDS = [
    "commissionHistory",
    "commissionPct",
    "createdDateTime",
    "customers",
    "isActive",
    "lastModifiedDateTime",
    "metadata",
    "name",
    "salesSub",
    "salespersonID",
]


def normalize_sales_person(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart SalesPerson rows to the legacy bronze contract."""
    normalized = {
        "commissionPct": dm_record.get("commissionPct"),
        # createdInDataMartDateTime is when DataMart loaded the row, not when it was created
        # (all 114 live sales persons: 2026-05-19 vs the ERP's 2023-09-12), so it is no fallback.
        "createdDateTime": erp_local_timestamp(dm_record.get("createdDateTime")),
        "isActive": dm_record.get("isActive"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "metadata": compact_json(
            {
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "salesPersonId": dm_record.get("salesPersonId"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "name": dm_record.get("name"),
        "salespersonID": first_non_empty(
            dm_record,
            "salesPersonCd",
            "salespersonID",
            "salesPersonId",
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    if normalized["salespersonID"] is not None:
        normalized["salespersonID"] = stringify_scalar(normalized["salespersonID"])
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
