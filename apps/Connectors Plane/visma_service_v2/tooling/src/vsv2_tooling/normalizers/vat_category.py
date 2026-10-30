"""VAT category normalizer (`/v1/vatcategory` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "active",
    "description",
    "excludeListedTaxes",
    "lastModifiedDateTime",
    "metadata",
    "vatCategoryId",
    "vatCategoryLines",
]


def normalize_vat_category(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart VatCategory row to the ERP REST bronze contract.

    ``vatCategoryLines`` (per-VAT rates nested under the category in the ERP payload)
    is not exposed by DataMart's VatCategory root, so it stays null. It is deliberately
    *not* ``[]``, which would claim the category has no lines.
    """
    normalized = {
        "vatCategoryId": stringify_scalar(dm_record.get("vatCategoryId")),
        "description": dm_record.get("description"),
        "active": dm_record.get("active"),
        "excludeListedTaxes": dm_record.get("excludeListedTaxes"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "metadata": compact_json(
            {
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "standardTaxCode": dm_record.get("standardTaxCode"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
