"""Warehouse normalizer keeps the ERP REST bronze shape without inventing codes."""

from __future__ import annotations

import json

import pytest

from vsv2_tooling.normalizers import REGISTRY, warehouse

DM_WAREHOUSE = {
    "id": "wh-1",
    "companyId": 4839725,
    "warehouseId": 133,
    "warehouseCd": "11",
    "branchId": 13,
    "replenishmentClassId": "01 TROND",
    "active": True,
    "lockSitePICountEntry": True,
    "description": "Trondheim",
    "locationEntry": "NoValidate",
    "avgDefaultCost": "LastCost",
    "fifoDefaultCost": "LastCost",
    "receiptLocationId": 134,
    "shipLocationId": 134,
    "returnLocationId": 135,
    "dropShipLocationId": 136,
    "contactId": 900,
    "addressId": 901,
    "lastModifiedDateTime": "2026-02-19T13:27:34.987Z",
    "createdInDataMartDateTime": "2026-01-01T00:00:00Z",
    "updatedInDataMartDateTime": "2026-02-19T12:30:00Z",
    "deletedDatabaseRecord": False,
}


@pytest.mark.unit
def test_registered() -> None:
    assert REGISTRY["normalize_warehouse"] is warehouse.normalize_warehouse


@pytest.mark.unit
def test_warehouse_maps_the_fields_datamart_has_verbatim() -> None:
    out = warehouse.normalize_warehouse(DM_WAREHOUSE)

    assert out["warehouseID"] == "11"
    assert out["description"] == "Trondheim"
    assert out["active"] is True
    assert out["lockSitePICountEntry"] is True
    assert out["locationEntry"] == "NoValidate"
    assert out["avgDefaultCost"] == "LastCost"
    assert out["fifoDefaultCost"] == "LastCost"
    assert json.loads(out["replenishmentClass"]) == {"id": "01 TROND"}
    assert out["_source"] == "datamart"


@pytest.mark.unit
def test_internal_ids_are_not_passed_off_as_erp_codes() -> None:
    out = warehouse.normalize_warehouse(DM_WAREHOUSE)

    # ERP holds codes ("1") where DataMart holds internal numbers (134, 13), measured on 15 live
    # warehouses: 0 of 15 matched. They stay out of the ERP fields and go to metadata.
    for field in (
        "branch",
        "receiptLocation",
        "shipLocation",
        "returnLocation",
        "dropShipLocation",
    ):
        assert out[field] is None, field
    meta = json.loads(out["metadata"])
    assert meta["branchId"] == 13
    assert meta["receiptLocationId"] == 134
    assert meta["warehouseId"] == 133


@pytest.mark.unit
def test_fields_datamart_lacks_stay_null() -> None:
    out = warehouse.normalize_warehouse(DM_WAREHOUSE)

    for field in ("contact", "address", "locations", "timestamp"):
        assert out[field] is None, field


@pytest.mark.unit
def test_every_legacy_field_is_present_on_empty_input() -> None:
    assert set(warehouse.LEGACY_FIELDS) <= set(warehouse.normalize_warehouse({}))
