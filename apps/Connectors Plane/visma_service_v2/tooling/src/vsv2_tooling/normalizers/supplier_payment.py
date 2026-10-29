"""Supplier payment normalizer (`/v1/supplierpayment` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "applicationDate",
    "applicationPeriod",
    "appliedAmount",
    "balance",
    "branch",
    "cashAccount",
    "currency",
    "description",
    "financeCharges",
    "hold",
    "lastModifiedDateTime",
    "location",
    "metadata",
    "paymentAmount",
    "paymentLines",
    "paymentMethod",
    "paymentRef",
    "refNbr",
    "released",
    "status",
    "supplier",
    "timeStamp",
    "type",
    "unappliedBalance",
]


def normalize_supplier_payment(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart SupplierPayment row to the ERP REST bronze contract.

    The key is ``(type, refNbr)``: a Prepayment and its VoidPayment share a ``refNbr``.
    DataMart holds internal ids where the ERP holds codes (cash account, location, branch), so
    those ERP fields stay null and the internal ids go to ``metadata`` rather than being
    passed off as codes.
    """
    currency_id = dm_record.get("currencyId")
    supplier_id = dm_record.get("supplierId")
    normalized = {
        "type": dm_record.get("documentType"),
        "refNbr": stringify_scalar(dm_record.get("refNbr")),
        "status": dm_record.get("status"),
        "hold": dm_record.get("hold"),
        "released": dm_record.get("released"),
        "description": dm_record.get("description"),
        "paymentRef": dm_record.get("paymentRef"),
        "paymentMethod": stringify_scalar(dm_record.get("paymentMethod")),
        "paymentAmount": dm_record.get("paymentAmount"),
        "appliedAmount": dm_record.get("appliedAmount"),
        "unappliedBalance": dm_record.get("unappliedBalance"),
        "financeCharges": dm_record.get("financeCharges"),
        "applicationPeriod": dm_record.get("applicationPeriod"),
        "applicationDate": erp_local_timestamp(dm_record.get("applicationDate")),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "currency": compact_json({"id": currency_id}) if currency_id else None,
        "supplier": compact_json({"internalId": supplier_id}) if supplier_id else None,
        "metadata": compact_json(
            {
                "cashAccountId": dm_record.get("cashAccountId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "locationId": dm_record.get("locationId"),
                "postedDateTime": dm_record.get("postedDateTime"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
