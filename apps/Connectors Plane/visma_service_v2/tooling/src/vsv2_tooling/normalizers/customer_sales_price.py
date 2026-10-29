"""Customer sales price normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, keep_legacy_fields

LEGACY_FIELDS = [
    "id",
    "companyId",
    "updatedInDataMartDateTime",
    "recordId",
    "inventoryId",
    "uom",
    "promotion",
    "breakQty",
    "price",
    "currencyId",
    "vatId",
    "effectiveDate",
    "expirationDate",
    "lastModifiedDateTime",
    "priceCode",
    "description",
    "deletedDatabaseRecord",
    "createdInDataMartDateTime",
]


def normalize_customer_sales_price(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Pass DataMart SalesPrice rows through and expose semantic-gap metadata.

    The live SalesPrice type does not expose the legacy `priceType` field that
    downstream models use to distinguish `Customer` from `CustomerPriceClass`.
    Keep that explicit rather than inventing a classification.
    """
    normalized = {field: dm_record.get(field) for field in LEGACY_FIELDS}
    normalized["priceType"] = None
    # ERP inventoryId is the item code ("FB4050"); DataMart holds its internal number, which is
    # the join key for the wrong table. ERP description is the item's; DataMart's is the price
    # record's own text. Both are wrong on every live row if passed through, so they go to
    # metadata and the ERP fields stay null.
    normalized["inventoryId"] = None
    normalized["description"] = None
    for field in ("effectiveDate", "expirationDate", "lastModifiedDateTime"):
        normalized[field] = erp_local_timestamp(dm_record.get(field))
    normalized["metadata"] = compact_json(
        {
            "inventoryInternalId": dm_record.get("inventoryId"),
            "priceDescription": dm_record.get("description"),
            "semanticGap": "priceType is not exposed by the live DataMart SalesPrice type",
        }
    )
    normalized["_source"] = "datamart"
    normalized["_updated_in_dm"] = dm_record.get("updatedInDataMartDateTime")
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
