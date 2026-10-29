"""Customer document normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "id",
    "companyId",
    "updatedInDataMartDateTime",
    "paymentMethodId",
    "postPeriod",
    "financialPeriod",
    "customerRefNbr",
    "referenceNumber",
    "paymentReference",
    "voucherId",
    "contractDocRef",
    "childRecordNumber",
    "vatTotal",
    "vatTotalInCurrency",
    "accountId",
    "subAccountId",
    "dueDate",
    "documentType",
    "documentDate",
    "status",
    "currencyId",
    "amount",
    "amountInCurrency",
    "balance",
    "balanceInCurrency",
    "cashDiscount",
    "cashDiscountInCurrency",
    "invoiceText",
    "locationId",
    "lastModifiedDateTime",
    "createdDateTime",
    "customerId",
    "branchId",
    "deletedDatabaseRecord",
    "creditTermsId",
    "cashDiscountDate",
    "externalReference",
    "isRotRutDeductible",
    "exchangeRate",
    "hold",
    "detailTotal",
    "detailTotalInCurrency",
    "vatTaxableTotal",
    "vatTaxableTotalInCurrency",
    "vatExemptTotal",
    "vatExemptTotalInCurrency",
    "salesPersonId",
    "note",
    "projectId",
    "dunningLetterDate",
    "dunningLetterLevel",
    "releasedDateTime",
    "createdInDataMartDateTime",
    "attachments",
]


def normalize_customer_document(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart CustomerDocument rows to the bronze contract."""
    normalized = {field: dm_record.get(field) for field in LEGACY_FIELDS}
    normalized["attachments"] = compact_json(dm_record.get("attachments"))
    normalized["account"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("accountId")),
            "number": stringify_scalar(dm_record.get("accountId")),
        }
    )
    normalized["branch"] = compact_json(
        {
            "branchId": stringify_scalar(dm_record.get("branchId")),
            "id": stringify_scalar(dm_record.get("branchId")),
            "number": stringify_scalar(dm_record.get("branchId")),
        }
    )
    normalized["creditTerms"] = compact_json({"id": dm_record.get("creditTermsId")})
    normalized["customer"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("customerId")),
            "number": stringify_scalar(dm_record.get("customerId")),
        }
    )
    normalized["location"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("locationId")),
        }
    )
    normalized["paymentMethod"] = compact_json({"id": dm_record.get("paymentMethodId")})
    normalized["project"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("projectId")),
            "projectId": stringify_scalar(dm_record.get("projectId")),
        }
    )
    normalized["salesPerson"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("salesPersonId")),
            "salespersonID": stringify_scalar(dm_record.get("salesPersonId")),
        }
    )
    normalized["subAccount"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("subAccountId")),
            "subaccountNumber": stringify_scalar(dm_record.get("subAccountId")),
        }
    )
    normalized["_source"] = "datamart"
    normalized["_updated_in_dm"] = dm_record.get("updatedInDataMartDateTime")
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
