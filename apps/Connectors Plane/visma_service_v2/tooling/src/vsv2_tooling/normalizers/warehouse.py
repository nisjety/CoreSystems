"""Warehouse normalizer (`/v1/warehouse` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "active",
    "address",
    "avgDefaultCost",
    "branch",
    "contact",
    "description",
    "dropShipLocation",
    "fifoDefaultCost",
    "locationEntry",
    "locations",
    "lockSitePICountEntry",
    "metadata",
    "receiptLocation",
    "replenishmentClass",
    "returnLocation",
    "shipLocation",
    "timestamp",
    "warehouseID",
]


def normalize_warehouse(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart Warehouse row to the ERP REST bronze contract.

    DataMart identifies branch and locations by *internal* numbers where the ERP holds
    codes, so those ERP fields stay null and the numbers go to ``metadata``; putting an
    internal id in a code field would be wrong data. ``replenishmentClassId`` is the same code
    the ERP uses, so it is mapped (without the description, which is a join).
    """
    replenishment = dm_record.get("replenishmentClassId")
    normalized = {
        "warehouseID": stringify_scalar(dm_record.get("warehouseCd")),
        "description": dm_record.get("description"),
        "active": dm_record.get("active"),
        "lockSitePICountEntry": dm_record.get("lockSitePICountEntry"),
        "locationEntry": dm_record.get("locationEntry"),
        "avgDefaultCost": dm_record.get("avgDefaultCost"),
        "fifoDefaultCost": dm_record.get("fifoDefaultCost"),
        "replenishmentClass": compact_json({"id": replenishment}) if replenishment else None,
        "metadata": compact_json(
            {
                "addressId": dm_record.get("addressId"),
                "branchId": dm_record.get("branchId"),
                "companyId": dm_record.get("companyId"),
                "contactId": dm_record.get("contactId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "dropShipLocationId": dm_record.get("dropShipLocationId"),
                "lastModifiedDateTime": dm_record.get("lastModifiedDateTime"),
                "receiptLocationId": dm_record.get("receiptLocationId"),
                "returnLocationId": dm_record.get("returnLocationId"),
                "shipLocationId": dm_record.get("shipLocationId"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
                "warehouseId": dm_record.get("warehouseId"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
