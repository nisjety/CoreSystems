"""Customer payment normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "id",
    "companyId",
    "updatedInDataMartDateTime",
    "documentType",
    "referenceNumber",
    "status",
    "hold",
    "applicationDate",
    "applicationPeriod",
    "paymentRef",
    "customerId",
    "locationId",
    "paymentMethodId",
    "cashAccountId",
    "currencyId",
    "paymentAmount",
    "invoiceText",
    "appliedToDocuments",
    "appliedToOrders",
    "availableBalance",
    "writeOffAmount",
    "financeCharges",
    "deductedCharges",
    "branchId",
    "paymentDate",
    "lastModifiedDateTime",
    "postedDateTime",
    "deletedDatabaseRecord",
    "createdInDataMartDateTime",
]


def normalize_customer_payment(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart CustomerPayment rows and preserve legacy aliases."""
    normalized = {field: dm_record.get(field) for field in LEGACY_FIELDS}
    normalized["branch"] = stringify_scalar(dm_record.get("branchId"))
    normalized["cashAccount"] = stringify_scalar(dm_record.get("cashAccountId"))
    normalized["currency"] = dm_record.get("currencyId")
    normalized["customer"] = compact_json(
        {
            "id": stringify_scalar(dm_record.get("customerId")),
            "number": stringify_scalar(dm_record.get("customerId")),
        }
    )
    normalized["location"] = stringify_scalar(dm_record.get("locationId"))
    normalized["paymentAmountInCurrency"] = None
    normalized["availableBalanceInCurrency"] = None
    normalized["paymentMethod"] = dm_record.get("paymentMethodId")
    normalized["refNbr"] = dm_record.get("referenceNumber")
    normalized["type"] = dm_record.get("documentType")
    normalized["_source"] = "datamart"
    normalized["_updated_in_dm"] = dm_record.get("updatedInDataMartDateTime")
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
