"""Supplier normalizer (`/v1/supplier` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "accountReference",
    "accountUsedForPayment",
    "cashAccount",
    "chargeBearer",
    "corporateId",
    "creditTerms",
    "currencyId",
    "currencyOverride",
    "currencyRateOverride",
    "documentLanguage",
    "internalId",
    "landedCostSupplier",
    "lastModifiedDateTime",
    "location",
    "mainAddress",
    "mainContact",
    "metadata",
    "name",
    "number",
    "parentRecord",
    "paySeparately",
    "paymentBy",
    "paymentLeadTime",
    "paymentMethod",
    "remitAddress",
    "remitContact",
    "retainageApply",
    "retainagePct",
    "status",
    "supplierAddress",
    "supplierClass",
    "supplierContact",
    "supplierPaymentMethodDetails",
    "timeStamp",
    "vatRegistrationId",
    "vatZone",
]


# ERP spelling of the supplier status enum. DataMart differs in case ("InActive" for the ERP's
# "Inactive", measured on 5 live rows); unknown values are passed through unchanged.
_ERP_STATUS = {s.lower(): s for s in ("Active", "OnHold", "HoldPayments", "Inactive", "OneTime")}


def _erp_status(value: Any) -> Any:
    if not isinstance(value, str):
        return value
    return _ERP_STATUS.get(value.strip().lower(), value)


def _ref(key: str, value: Any) -> str | None:
    """Nested ERP object holding only the id DataMart knows; the rest of it needs a join."""
    return compact_json({key: value}) if value not in (None, "") else None


def normalize_supplier(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart Supplier row to the ERP REST bronze contract.

    DataMart's Supplier root carries 21 fields against the ERP's 36. What it has is mapped;
    everything else (payment settings, VAT ids, addresses/contacts beyond their ids) stays
    null - never defaulted, because VAT and corporate ids are legitimately empty on many
    suppliers and a default would be indistinguishable from real data.
    """
    normalized = {
        "number": stringify_scalar(dm_record.get("supplierCd")),
        "name": dm_record.get("supplierName"),
        "status": _erp_status(dm_record.get("status")),
        "currencyId": dm_record.get("currencyId"),
        "documentLanguage": dm_record.get("documentLanguage"),
        "internalId": dm_record.get("supplierId"),
        "accountReference": dm_record.get("accountRef"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "creditTerms": _ref("id", dm_record.get("termsId")),
        "supplierClass": _ref("id", dm_record.get("supplierClassId")),
        "mainAddress": _ref("addressId", dm_record.get("mainAddressId")),
        "mainContact": _ref("contactId", dm_record.get("mainContactId")),
        "parentRecord": _ref("internalId", dm_record.get("parentBAccountId")),
        "metadata": compact_json(
            {
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "mainLocationId": dm_record.get("mainLocationId"),
                "noteId": dm_record.get("noteId"),
                "parentAccountId": dm_record.get("parentAccountId"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
