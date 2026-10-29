"""Older normalizers must emit ERP-shaped values or null - never a wrong-domain value.

Each case below was measured against live ERP data with ``scripts/normalizer_parity.py`` and
found to put a value of the wrong kind into an ERP field on every row:

* credit terms: DataMart enum names ("Gross") where the ERP holds codes ("G"), and DataMart's
  load time as ``createdDateTime``
* customers: a datetime in ``timeStamp`` (the ERP holds a row version), internal ids in
  ``location``, ``id`` instead of ``addressId`` / ``contactId``
* sales prices: DataMart's internal inventory number in ``inventoryId`` (the ERP holds the item
  code) and a different ``description``
"""

from __future__ import annotations

import json

import pytest

from vsv2_tooling.normalizers import contact, credit_term, customer, customer_sales_price

# --- credit terms --------------------------------------------------------------------

DM_TERM = {
    "termsId": "30",
    "description": "30 dager netto",
    "calculationType": "Gross",
    "discountType": "DayOfNextMonth",
    "dueDateType": "FixedNumberOfDays",
    "instalmentFrequency": "Weekly",
    "instalmentMethod": "EqualParts",
    "instalmentType": "Single",
    "moveDueDateIfOnTheWeekend": "Keep the exact date",
    "visibleTo": "All",
    "dayFrom1": 1,
    "dayTo1": 15,
    "dayFrom2": 16,
    "dayTo2": 31,
    "lastModifiedDateTime": "2026-02-19T13:27:34.980Z",
    "createdInDataMartDateTime": "2026-02-15T00:42:07.100Z",
}


@pytest.mark.unit
def test_credit_term_enums_use_the_erp_codes() -> None:
    out = credit_term.normalize_credit_term(DM_TERM)

    assert out["calculationType"] == "G"
    assert out["discountType"] == "D"
    assert out["dueDateType"] == "N"
    assert out["instalmentFrequency"] == "W"
    assert out["instalmentMethod"] == "E"
    assert out["instalmentType"] == "S"
    assert out["moveDueDateIfOnTheWeekend"] == "1"
    assert out["visibleTo"] == "AL"


@pytest.mark.unit
@pytest.mark.parametrize(
    ("field", "dm_value", "erp_code"),
    [
        ("dueDateType", "DayOfTheMonth", "T"),
        ("dueDateType", "Custom", "C"),
        ("discountType", "FixedNumberOfDays", "N"),
        ("discountType", "DayOfTheMonth", "T"),
    ],
)
def test_credit_term_enum_translations_seen_on_live_data(field, dm_value, erp_code) -> None:
    assert credit_term.normalize_credit_term({field: dm_value})[field] == erp_code


@pytest.mark.unit
def test_credit_term_enum_value_never_seen_before_passes_through_unchanged() -> None:
    # No code is invented for a value that has not been observed against the ERP.
    assert credit_term.normalize_credit_term({"dueDateType": "EndOfQuarter"})["dueDateType"] == (
        "EndOfQuarter"
    )
    assert credit_term.normalize_credit_term({"dueDateType": None})["dueDateType"] is None


@pytest.mark.unit
def test_credit_term_day_windows_use_the_erp_field_names() -> None:
    out = credit_term.normalize_credit_term(DM_TERM)

    assert (out["dueDay1DayFrom1"], out["dueDay1DayTo1"]) == (1, 15)
    assert (out["dueDay2DayFrom1"], out["dueDay2DayTo1"]) == (16, 31)


@pytest.mark.unit
def test_credit_term_last_modified_uses_the_erp_wire_format() -> None:
    assert credit_term.normalize_credit_term(DM_TERM)["lastModifiedDateTime"] == (
        "2026-02-19T13:27:34.98"
    )


# --- customers -----------------------------------------------------------------------

DM_CUSTOMER = {
    "customerId": 42,
    "customerNumber": "C42",
    "customerName": "Testkunde AS",
    "mainAddressId": 116260,
    "mainContactId": 116019,
    "billAddressId": 116261,
    "billContactId": 116020,
    "mainLocationId": 77545,
    "createdDateTime": "2023-09-12T13:38:23.283Z",
    "lastModifiedDateTime": "2026-09-08T09:16:10.681Z",
    "createdInDataMartDateTime": "2026-02-15T00:00:00Z",
    "updatedInDataMartDateTime": "2026-09-08T07:16:10.681Z",
}


@pytest.mark.unit
def test_customer_time_stamp_is_null_not_a_datetime() -> None:
    # ERP timeStamp is a row version ("AAAAADE6vFc="). DataMart has none; a datetime there would
    # be a wrong value on all 6,586 live rows.
    assert customer.normalize_customer_with_locations(DM_CUSTOMER)["timeStamp"] is None


@pytest.mark.unit
def test_customer_references_use_the_erp_key_names() -> None:
    out = customer.normalize_customer_with_locations(DM_CUSTOMER)

    assert json.loads(out["mainAddress"]) == {"addressId": 116260}
    assert json.loads(out["mainContact"]) == {"contactId": 116019}
    assert json.loads(out["invoiceAddress"]) == {"addressId": 116261}
    assert json.loads(out["invoiceContact"]) == {"contactId": 116020}


@pytest.mark.unit
def test_customer_location_is_null_because_datamart_only_has_an_internal_id() -> None:
    out = customer.normalize_customer_with_locations(DM_CUSTOMER)

    # ERP location.id is a code ("HOVED"); 77545 is an internal number.
    assert out["location"] is None
    assert json.loads(out["metadata"])["mainLocationId"] == 77545


@pytest.mark.unit
def test_customer_dates_come_from_the_erp_fields_in_the_erp_format() -> None:
    out = customer.normalize_customer_with_locations(DM_CUSTOMER)

    assert out["createdDateTime"] == "2023-09-12T13:38:23.283"
    assert out["lastModifiedDateTime"] == "2026-09-08T09:16:10.681"


@pytest.mark.unit
def test_customer_created_date_never_falls_back_to_datamart_load_time() -> None:
    out = customer.normalize_customer_with_locations(
        {**DM_CUSTOMER, "createdDateTime": None, "lastModifiedDateTime": None}
    )

    assert out["createdDateTime"] is None
    assert out["lastModifiedDateTime"] is None  # updatedInDataMart is true UTC, another clock


# --- sales prices and contacts -------------------------------------------------------

DM_PRICE = {
    "recordId": 7,
    "inventoryId": 2581311,
    "description": "Statnett/Punchout",
    "effectiveDate": "2026-01-01T00:00:00Z",
    "expirationDate": None,
    "lastModifiedDateTime": "2026-05-13T09:00:00.500Z",
}


@pytest.mark.unit
def test_sales_price_does_not_put_the_internal_inventory_number_in_the_item_code_field() -> None:
    out = customer_sales_price.normalize_customer_sales_price(DM_PRICE)

    # ERP inventoryId is the item code ("FB4050"); 2581311 is DataMart's internal number.
    assert out["inventoryId"] is None
    assert json.loads(out["metadata"])["inventoryInternalId"] == 2581311


@pytest.mark.unit
def test_sales_price_description_is_not_the_item_description() -> None:
    out = customer_sales_price.normalize_customer_sales_price(DM_PRICE)

    assert out["description"] is None
    assert json.loads(out["metadata"])["priceDescription"] == "Statnett/Punchout"


@pytest.mark.unit
def test_sales_price_dates_use_the_erp_wire_format() -> None:
    out = customer_sales_price.normalize_customer_sales_price(DM_PRICE)

    assert out["effectiveDate"] == "2026-01-01T00:00:00"
    assert out["expirationDate"] is None
    assert out["lastModifiedDateTime"] == "2026-05-13T09:00:00.5"
    assert "semanticGap" in json.loads(out["metadata"])  # existing contract is kept


@pytest.mark.unit
def test_contact_last_modified_uses_the_erp_wire_format() -> None:
    out = contact.normalize_contact(
        {"contactId": 1, "lastModifiedDateTime": "2026-05-13T09:00:00.120Z"}
    )

    assert out["lastModifiedDateTime"] == "2026-05-13T09:00:00.12"


# --- the same two defects, checked for every normalizer that had them ------------------------
#
# 1. ``lastModifiedDateTime`` fell back to ``updatedInDataMartDateTime`` (true UTC) when the
#    Oslo wall-clock value was missing, mixing two clocks in one column, and copied DataMart's
#    misleading ``Z``.
# 2. ``createdDateTime`` / ``timeStamp`` fell back to DataMart's *load* time, which is neither
#    the ERP's creation time nor a row version (sales persons: 2026-05-19 vs 2023-09-12 on all
#    114 live rows).

from vsv2_tooling.normalizers import (  # noqa: E402
    budget,
    general_ledger_balance,
    journal_transaction,
    sales_order,
    sales_person,
    subaccount,
)

LAST_MODIFIED_NORMALIZERS = [
    budget.normalize_budget,
    general_ledger_balance.normalize_general_ledger_balance,
    journal_transaction.normalize_journal_transaction,
    sales_order.normalize_sales_order,
    sales_person.normalize_sales_person,
    subaccount.normalize_subaccount,
    customer_sales_price.normalize_customer_sales_price,
    contact.normalize_contact,
]


@pytest.mark.unit
@pytest.mark.parametrize("normalize", LAST_MODIFIED_NORMALIZERS)
def test_last_modified_is_the_erp_value_in_the_erp_format(normalize) -> None:
    out = normalize(
        {
            "lastModifiedDateTime": "2026-09-08T09:16:10.681Z",
            "updatedInDataMartDateTime": "2026-09-08T07:16:10.681Z",
        }
    )

    assert out["lastModifiedDateTime"] == "2026-09-08T09:16:10.681"


@pytest.mark.unit
@pytest.mark.parametrize("normalize", LAST_MODIFIED_NORMALIZERS)
def test_last_modified_never_falls_back_to_the_datamart_update_time(normalize) -> None:
    out = normalize({"updatedInDataMartDateTime": "2026-09-08T07:16:10.681Z"})

    # A true-UTC value in an Oslo-wall-clock column is a wrong value, so absent means null.
    assert out["lastModifiedDateTime"] is None


@pytest.mark.unit
def test_sales_person_created_is_the_erp_value_or_null_never_the_load_time() -> None:
    with_erp_value = sales_person.normalize_sales_person(
        {"createdDateTime": "2023-09-12T11:11:37.613Z"}
    )
    load_time_only = sales_person.normalize_sales_person(
        {"createdInDataMartDateTime": "2026-05-19T11:07:35.210Z"}
    )

    assert with_erp_value["createdDateTime"] == "2023-09-12T11:11:37.613"
    assert load_time_only["createdDateTime"] is None
    assert json.loads(load_time_only["metadata"])["createdInDataMartDateTime"] == (
        "2026-05-19T11:07:35.210Z"
    )


@pytest.mark.unit
@pytest.mark.parametrize(
    "normalize", [sales_order.normalize_sales_order, subaccount.normalize_subaccount]
)
def test_time_stamp_is_null_because_the_erp_holds_a_row_version(normalize) -> None:
    out = normalize(
        {
            "updatedInDataMartDateTime": "2026-09-08T07:16:10.681Z",
            "lastModifiedDateTime": "2026-09-08T09:16:10.681Z",
            "createdInDataMartDateTime": "2026-02-15T00:00:00Z",
        }
    )

    assert out["timeStamp"] is None
