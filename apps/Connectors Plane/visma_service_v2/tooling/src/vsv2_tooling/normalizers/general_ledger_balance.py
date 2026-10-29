"""General ledger balance normalizer."""

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
    "account",
    "balanceType",
    "beginningBalance",
    "beginningBalanceInCurrency",
    "branch",
    "currencyId",
    "financialPeriod",
    "lastModifiedDateTime",
    "ledger",
    "metadata",
    "periodToDateCredit",
    "periodToDateCreditInCurrency",
    "periodToDateDebit",
    "periodToDateDebitInCurrency",
    "subAccountCd",
    "subaccountId",
    "yearClosed",
    "yearToDateBalance",
    "yearToDateBalanceInCurrency",
]


def normalize_general_ledger_balance(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart GL balances to the legacy bronze contract."""
    normalized = {
        "account": compact_json(
            {
                "id": dm_record.get("accountId"),
                "number": stringify_scalar(dm_record.get("accountId")),
                "type": dm_record.get("accountType"),
            }
        ),
        "balanceType": dm_record.get("balanceType"),
        "beginningBalance": dm_record.get("beginningBalance"),
        "beginningBalanceInCurrency": dm_record.get("beginningBalanceInCurrency"),
        "branch": compact_json(
            {
                "branchId": dm_record.get("branchId"),
                "id": dm_record.get("branchId"),
                "number": stringify_scalar(dm_record.get("branchId")),
            }
        ),
        "currencyId": first_non_empty(dm_record, "curyId", "baseCuryId"),
        "financialPeriod": dm_record.get("finPeriodId"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "ledger": compact_json(
            {
                "id": stringify_scalar(dm_record.get("ledgerId")),
            }
        ),
        "metadata": compact_json(
            {
                "baseCurrencyId": dm_record.get("baseCuryId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "deletedDatabaseRecord": dm_record.get("deletedDatabaseRecord"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "periodToDateCredit": dm_record.get("periodToDateCredit"),
        "periodToDateCreditInCurrency": dm_record.get("periodToDateCreditInCurrency"),
        "periodToDateDebit": dm_record.get("periodToDateDebit"),
        "periodToDateDebitInCurrency": dm_record.get("periodToDateDebitInCurrency"),
        "subAccountCd": stringify_scalar(
            first_non_empty(dm_record, "subAccountCd", "subaccountId")
        ),
        "subaccountId": stringify_scalar(dm_record.get("subaccountId")),
        "yearClosed": dm_record.get("yearClosed"),
        "yearToDateBalance": dm_record.get("yearToDateBalance"),
        "yearToDateBalanceInCurrency": dm_record.get("yearToDateBalanceInCurrency"),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
