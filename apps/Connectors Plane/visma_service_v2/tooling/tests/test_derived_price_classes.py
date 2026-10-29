"""price_classes is a derived table: v1 builds it, v2 must reproduce it exactly."""

from __future__ import annotations

import pytest

from vsv2_tooling.derived import price_classes


@pytest.mark.unit
def test_a_price_class_seen_only_on_a_location_has_an_empty_description() -> None:
    rows = price_classes(customer_locations=[{"priceClassId": "49"}], customers=[])

    assert rows == [{"id": "49", "description": ""}]


@pytest.mark.unit
def test_a_customer_price_class_supplies_the_description() -> None:
    rows = price_classes(
        customer_locations=[{"priceClassId": "49"}],
        customers=[{"priceClass": {"id": "49", "description": "1814Salmon"}}],
    )

    assert rows == [{"id": "49", "description": "1814Salmon"}]


@pytest.mark.unit
def test_the_first_non_empty_description_wins() -> None:
    rows = price_classes(
        customer_locations=[],
        customers=[
            {"priceClass": {"id": "7", "description": ""}},
            {"priceClass": {"id": "7", "description": "First"}},
            {"priceClass": {"id": "7", "description": "Second"}},
        ],
    )

    assert rows == [{"id": "7", "description": "First"}]


@pytest.mark.unit
def test_rows_without_a_usable_id_are_skipped() -> None:
    rows = price_classes(
        customer_locations=[
            {"priceClassId": ""},
            {"priceClassId": None},
            {"name": "x"},
            {"priceClassId": 5},
        ],
        customers=[
            {"priceClass": None},
            {"priceClass": "49"},
            {"priceClass": {"id": None, "description": "x"}},
            {"priceClass": {"description": "no id"}},
        ],
    )

    assert rows == []


@pytest.mark.unit
def test_a_missing_or_non_text_description_becomes_empty() -> None:
    rows = price_classes(
        customer_locations=[],
        customers=[{"priceClass": {"id": "1"}}, {"priceClass": {"id": "2", "description": 9}}],
    )

    assert rows == [{"id": "1", "description": ""}, {"id": "2", "description": ""}]


@pytest.mark.unit
def test_each_price_class_appears_once() -> None:
    rows = price_classes(
        customer_locations=[{"priceClassId": "1"}, {"priceClassId": "1"}, {"priceClassId": "2"}],
        customers=[{"priceClass": {"id": "1", "description": "One"}}],
    )

    assert sorted(r["id"] for r in rows) == ["1", "2"]
    assert next(r for r in rows if r["id"] == "1")["description"] == "One"
