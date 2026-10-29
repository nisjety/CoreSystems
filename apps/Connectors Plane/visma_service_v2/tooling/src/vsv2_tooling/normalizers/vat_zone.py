"""VAT zone normalizer (`/v1/vatzone` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "defaultTaxCategory",
    "description",
    "id",
    "metadata",
]


def normalize_vat_zone(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart VatZone row to the ERP REST bronze contract.

    The ERP nests ``defaultTaxCategory`` as ``{number, description}``. DataMart only
    carries the category id, so ``description`` (a join against VatCategory) is left
    out rather than invented.
    """
    category_id = stringify_scalar(dm_record.get("defaultVatCategoryId"))
    normalized = {
        "id": stringify_scalar(dm_record.get("vatZoneId")),
        "description": dm_record.get("description"),
        "defaultTaxCategory": compact_json({"number": category_id}),
        "metadata": compact_json(
            {
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "lastModifiedDateTime": dm_record.get("lastModifiedDateTime"),
                "manualVatEntry": dm_record.get("manualVatEntry"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
