"""Budget normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import (
    compact_json,
    erp_local_timestamp,
    keep_legacy_fields,
    stringify_scalar,
)

LEGACY_FIELDS = [
    "financialYear",
    "released",
    "releasedAmount",
    "account",
    "subaccount",
    "description",
    "amount",
    "distributedAmount",
    "periods",
    "lastModifiedDateTime",
    "branchNumber",
    "timeStamp",
]


def normalize_budget(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart Budget rows to the legacy bronze contract.

    The live DataMart Budget type only exposes numeric identifiers for branch,
    account, and subaccount. The legacy ERP payload carries descriptive nested
    objects instead. Preserve the contract shape using the best available ID
    fallbacks and record the unresolved DataMart identifiers in `metadata`.
    """
    description = dm_record.get("description")
    account_number = stringify_scalar(dm_record.get("accountId"))
    branch_number = stringify_scalar(dm_record.get("branchId"))
    subaccount_id = stringify_scalar(dm_record.get("subaccountId"))

    normalized = {
        "financialYear": dm_record.get("financialYear"),
        "released": dm_record.get("released"),
        "releasedAmount": dm_record.get("releasedAmount"),
        "account": compact_json(
            {
                "number": account_number,
                "description": description,
            }
        ),
        "subaccount": compact_json(
            {
                "id": subaccount_id,
            }
        ),
        "description": description,
        "amount": dm_record.get("amount"),
        "distributedAmount": dm_record.get("distributedAmount"),
        "periods": compact_json(dm_record.get("periods")),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "branchNumber": compact_json(
            {
                "number": branch_number,
                "name": branch_number,
            }
        ),
        "timeStamp": None,
        "metadata": compact_json(
            {
                "branchId": dm_record.get("branchId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "groupId": dm_record.get("groupId"),
                "ledgerId": dm_record.get("ledgerId"),
                "semanticGap": (
                    "DataMart Budget exposes only numeric branch/account/subaccount IDs; "
                    "legacy ERP budget payload uses nested business-key objects."
                ),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
