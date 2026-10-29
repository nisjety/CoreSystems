"""VAT zone / VAT category normalizers keep the ERP REST bronze shape."""

from __future__ import annotations

import json

import pytest

from vsv2_tooling.normalizers import REGISTRY, vat_category, vat_zone

DM_VAT_ZONE = {
    "id": "6f1c0e2a9b",
    "companyId": 4839725,
    "vatZoneId": "01",
    "defaultVatCategoryId": "3",
    "description": "Salg innenlands, avgiftspliktig",
    "manualVatEntry": False,
    "lastModifiedDateTime": "2026-02-19T13:27:34.987Z",
    "createdInDataMartDateTime": "2026-01-01T00:00:00Z",
    "updatedInDataMartDateTime": "2026-02-19T13:30:00Z",
    "deletedDatabaseRecord": False,
}

DM_VAT_CATEGORY = {
    "id": "aa11",
    "companyId": 4839725,
    "vatCategoryId": "0",
    "description": "0% - Ingen mva-behandling (anskaffelser)",
    "excludeListedTaxes": False,
    "active": True,
    "standardTaxCode": "0",
    "lastModifiedDateTime": "2026-09-29T16:59:39.630Z",
    "createdInDataMartDateTime": "2026-01-01T00:00:00Z",
    "updatedInDataMartDateTime": "2026-09-29T17:00:00Z",
    "deletedDatabaseRecord": False,
}


@pytest.mark.unit
def test_registered() -> None:
    assert REGISTRY["normalize_vat_zone"] is vat_zone.normalize_vat_zone
    assert REGISTRY["normalize_vat_category"] is vat_category.normalize_vat_category


@pytest.mark.unit
def test_vat_zone_renames_key_and_nests_the_category_number() -> None:
    out = vat_zone.normalize_vat_zone(DM_VAT_ZONE)

    assert out["id"] == "01"
    assert out["description"] == "Salg innenlands, avgiftspliktig"
    # ERP shape is {"number", "description"}; DataMart only knows the category id, so the
    # description (a join) stays absent instead of being invented.
    assert json.loads(out["defaultTaxCategory"]) == {"number": "3"}
    assert out["_source"] == "datamart"


@pytest.mark.unit
def test_vat_zone_keeps_datamart_only_fields_in_metadata() -> None:
    meta = json.loads(vat_zone.normalize_vat_zone(DM_VAT_ZONE)["metadata"])

    assert meta["dataMartId"] == "6f1c0e2a9b"
    assert meta["manualVatEntry"] is False
    assert meta["companyId"] == 4839725


@pytest.mark.unit
def test_vat_zone_without_default_category_leaves_it_null() -> None:
    out = vat_zone.normalize_vat_zone({**DM_VAT_ZONE, "defaultVatCategoryId": None})

    assert out["defaultTaxCategory"] is None


@pytest.mark.unit
def test_vat_category_maps_erp_fields() -> None:
    out = vat_category.normalize_vat_category(DM_VAT_CATEGORY)

    assert out["vatCategoryId"] == "0"
    assert out["active"] is True
    assert out["excludeListedTaxes"] is False
    # ERP wire format: no mislabelled Z, no trailing zeros
    assert out["lastModifiedDateTime"] == "2026-09-29T16:59:39.63"
    assert json.loads(out["metadata"])["standardTaxCode"] == "0"


@pytest.mark.unit
def test_vat_category_lines_are_null_not_an_invented_empty_list() -> None:
    # The ERP nests per-VAT lines here. DataMart's VatCategory root has none, so claiming
    # `[]` would tell downstream "this category has no VAT lines", which is untrue.
    assert vat_category.normalize_vat_category(DM_VAT_CATEGORY)["vatCategoryLines"] is None


@pytest.mark.unit
@pytest.mark.parametrize("fn", [vat_zone.normalize_vat_zone, vat_category.normalize_vat_category])
def test_every_legacy_field_is_present(fn) -> None:
    module = vat_zone if fn is vat_zone.normalize_vat_zone else vat_category
    out = fn({})
    assert set(module.LEGACY_FIELDS) <= set(out)


# --- shared ERP timestamp helper -------------------------------------------------------


@pytest.mark.unit
@pytest.mark.parametrize(
    ("dm_value", "erp_value"),
    [
        # DataMart appends a 'Z', but the clock is Europe/Oslo wall time (verified against
        # updatedInDataMartDateTime: +1h in winter, +2h in summer), so 'Z' is a mislabel.
        ("2026-09-29T16:59:39.630Z", "2026-09-29T16:59:39.63"),
        ("2026-02-19T13:27:34.987Z", "2026-02-19T13:27:34.987"),
        ("2026-05-08T00:00:00Z", "2026-05-08T00:00:00"),
        ("2026-05-08T00:00:00.000Z", "2026-05-08T00:00:00"),
        ("2026-05-08T00:00:00", "2026-05-08T00:00:00"),
        (None, None),
        ("", None),
        ("not a timestamp", "not a timestamp"),
    ],
)
def test_erp_local_timestamp_matches_the_erp_wire_format(dm_value, erp_value) -> None:
    from vsv2_tooling.normalizers._shared import erp_local_timestamp

    assert erp_local_timestamp(dm_value) == erp_value
