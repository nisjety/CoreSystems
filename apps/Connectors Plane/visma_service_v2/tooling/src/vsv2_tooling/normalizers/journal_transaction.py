"""Journal transaction normalizer."""

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
    "module",
    "batchNumber",
    "status",
    "transactionType",
    "transactionDate",
    "financialPeriod",
    "referenceNumber",
    "lineNumber",
    "account",
    "subaccount",
    "branch",
    "ledger",
    "debitAmount",
    "creditAmount",
    "currency",
    "description",
    "lastModifiedDateTime",
    "metadata",
]


def normalize_journal_transaction(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart JournalTransaction rows to the legacy bronze contract.

    The ERP-REST `/v2/journaltransaction` payload keys each batch on the natural
    PK ``[module, batchNumber]``; DataMart spells the batch field ``batchNbr`` and
    exposes only numeric identifiers for branch/account/subaccount/ledger where the
    legacy payload carries descriptive nested objects. Preserve the contract shape
    using the best available ID fallbacks and record the unresolved DataMart
    identifiers in ``metadata``.

    Note: DataMart paginates JournalTransaction by *line*, so each ``dm_record`` is a
    single line. Batch-level line merging happens downstream in the ERP adapter; this
    normalizer only reshapes one record at a time.
    """
    branch_id = stringify_scalar(dm_record.get("branchId"))
    account_id = stringify_scalar(dm_record.get("accountId"))
    subaccount_id = stringify_scalar(dm_record.get("subaccountId"))
    ledger_id = stringify_scalar(dm_record.get("ledgerId"))

    normalized = {
        "module": dm_record.get("module"),
        # ERP natural PK: batchNbr (DataMart) -> batchNumber (legacy bronze).
        "batchNumber": stringify_scalar(dm_record.get("batchNbr")),
        # DataMart JournalTransaction exposes no batch status; left null.
        "status": None,
        "transactionType": dm_record.get("tranType"),
        "transactionDate": dm_record.get("tranDate"),
        "financialPeriod": dm_record.get("finPeriodId"),
        "referenceNumber": dm_record.get("refNbr"),
        "lineNumber": dm_record.get("lineNbr"),
        "account": compact_json(
            {
                "id": dm_record.get("accountId"),
                "number": account_id,
            }
        ),
        "subaccount": compact_json(
            {
                "id": subaccount_id,
            }
        ),
        "branch": compact_json(
            {
                "id": dm_record.get("branchId"),
                "number": branch_id,
            }
        ),
        "ledger": compact_json(
            {
                "id": ledger_id,
            }
        ),
        "debitAmount": first_non_empty(dm_record, "debitAmt", "curyDebitAmt"),
        "creditAmount": first_non_empty(dm_record, "creditAmt", "curyCreditAmt"),
        "currency": first_non_empty(dm_record, "curyId", "currencyId"),
        "description": first_non_empty(dm_record, "tranDesc", "batchDescription"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "metadata": compact_json(
            {
                "branchId": dm_record.get("branchId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "customerId": dm_record.get("customerId"),
                "dataMartId": dm_record.get("id"),
                "ledgerId": dm_record.get("ledgerId"),
                "postedDateTime": dm_record.get("postedDateTime"),
                "projectId": dm_record.get("projectId"),
                "reclassBatchNbr": dm_record.get("reclassBatchNbr"),
                "semanticGap": (
                    "DataMart JournalTransaction exposes only numeric "
                    "branch/account/subaccount/ledger IDs and no batch status; legacy "
                    "ERP payload uses nested business-key objects and a batch status."
                ),
                "subaccountId": dm_record.get("subaccountId"),
                "supplierId": dm_record.get("supplierId"),
                "taskId": dm_record.get("taskId"),
                "tranId": dm_record.get("tranId"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
