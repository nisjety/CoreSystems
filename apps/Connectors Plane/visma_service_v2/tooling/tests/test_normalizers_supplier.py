"""Supplier, supplier payment and project task normalizers keep the ERP REST bronze shape."""

from __future__ import annotations

import json

import pytest

from vsv2_tooling.normalizers import REGISTRY, project_task, supplier, supplier_payment

DM_SUPPLIER = {
    "id": "5f0a",
    "companyId": 4839725,
    "supplierCd": "10042",
    "supplierId": 77012,
    "supplierName": "Testleverandør AS",
    "status": "Active",
    "currencyId": "NOK",
    "documentLanguage": "nb-NO",
    "accountRef": None,
    "termsId": "30D",
    "supplierClassId": "DEFAULT",
    "mainAddressId": 5001,
    "mainContactId": 6001,
    "mainLocationId": 7001,
    "parentAccountId": None,
    "parentBAccountId": None,
    "noteId": "n-1",
    "lastModifiedDateTime": "2026-09-29T13:44:20.210Z",
    "createdInDataMartDateTime": "2026-01-01T00:00:00Z",
    "updatedInDataMartDateTime": "2026-09-29T11:50:00Z",
    "deletedDatabaseRecord": False,
}

DM_PAYMENT = {
    "id": "9c1d",
    "companyId": 4839725,
    "documentType": "Prepayment",
    "refNbr": "300035",
    "status": "Voided",
    "hold": False,
    "released": True,
    "description": "GIRO",
    "paymentRef": None,
    "paymentMethod": "1",
    "cashAccountId": 14675,
    "currencyId": "NOK",
    "supplierId": 206886,
    "locationId": 1,
    "paymentAmount": 1250.5,
    "appliedAmount": 0.0,
    "unappliedBalance": 1250.5,
    "financeCharges": 0.0,
    "applicationPeriod": "202605",
    "applicationDate": "2026-05-08T00:00:00Z",
    "postedDateTime": "2026-05-08T12:00:00Z",
    "lastModifiedDateTime": "2026-05-08T14:23:58.953Z",
    "createdInDataMartDateTime": "2026-05-08T12:27:17.564Z",
    "updatedInDataMartDateTime": "2026-05-08T12:30:00Z",
    "deletedDatabaseRecord": False,
}

DM_TASK = {
    "id": "t-1",
    "companyId": 4839725,
    "projectId": 4711,
    "taskId": 90210,
    "taskCd": "3",
    "description": "Montering",
    "status": "Active",
    "plannedStartDate": "2026-01-05T00:00:00Z",
    "plannedEndDate": "2026-03-01T00:00:00Z",
    "startDate": "2026-01-06T00:00:00Z",
    "endDate": None,
    "rateTableId": "STD",
    "rateTableDescription": "Standard",
    "branchId": 1,
    "lastModifiedDateTime": "2026-02-19T13:27:34.987Z",
    "createdInDataMartDateTime": "2026-01-01T00:00:00Z",
    "updatedInDataMartDateTime": "2026-02-19T12:30:00Z",
    "deletedDatabaseRecord": False,
}


@pytest.mark.unit
def test_registered() -> None:
    assert REGISTRY["normalize_supplier"] is supplier.normalize_supplier
    assert REGISTRY["normalize_supplier_payment"] is supplier_payment.normalize_supplier_payment
    assert REGISTRY["normalize_project_task"] is project_task.normalize_project_task


# --- suppliers -----------------------------------------------------------------------


@pytest.mark.unit
def test_supplier_maps_identity_and_scalars() -> None:
    out = supplier.normalize_supplier(DM_SUPPLIER)

    assert out["number"] == "10042"
    assert out["name"] == "Testleverandør AS"
    assert out["status"] == "Active"
    assert out["currencyId"] == "NOK"
    assert out["internalId"] == 77012
    assert out["lastModifiedDateTime"] == "2026-09-29T13:44:20.21"  # no mislabelled Z
    assert out["_source"] == "datamart"


@pytest.mark.unit
def test_supplier_nests_only_the_ids_datamart_actually_has() -> None:
    out = supplier.normalize_supplier(DM_SUPPLIER)

    assert json.loads(out["creditTerms"]) == {"id": "30D"}
    assert json.loads(out["supplierClass"]) == {"id": "DEFAULT"}
    assert json.loads(out["mainAddress"]) == {"addressId": 5001}
    assert json.loads(out["mainContact"]) == {"contactId": 6001}


@pytest.mark.unit
def test_supplier_location_is_null_because_datamart_only_has_the_internal_id() -> None:
    out = supplier.normalize_supplier(DM_SUPPLIER)

    # ERP location.id is a code ("HOVED"); DataMart's mainLocationId is an internal number.
    # Putting the number in the code field would be wrong data, so it goes to metadata.
    assert out["location"] is None
    assert json.loads(out["metadata"])["mainLocationId"] == 7001


@pytest.mark.unit
@pytest.mark.parametrize(
    ("dm_status", "erp_status"),
    [
        ("Active", "Active"),
        ("InActive", "Inactive"),  # the only spelling difference measured on live data
        ("ONHOLD", "OnHold"),
        ("holdpayments", "HoldPayments"),
        ("SomethingNew", "SomethingNew"),  # unknown values pass through untouched
        (None, None),
    ],
)
def test_supplier_status_uses_the_erp_spelling(dm_status, erp_status) -> None:
    assert supplier.normalize_supplier({**DM_SUPPLIER, "status": dm_status})["status"] == erp_status


@pytest.mark.unit
def test_supplier_fields_datamart_lacks_stay_null_not_defaulted() -> None:
    out = supplier.normalize_supplier(DM_SUPPLIER)

    # VAT ids are legitimately absent for many suppliers, so a default would be a lie.
    for field in ("vatRegistrationId", "corporateId", "vatZone", "paymentMethod", "retainagePct"):
        assert out[field] is None, field


@pytest.mark.unit
def test_supplier_keeps_datamart_only_ids_in_metadata() -> None:
    meta = json.loads(supplier.normalize_supplier(DM_SUPPLIER)["metadata"])

    assert meta["dataMartId"] == "5f0a"
    assert meta["noteId"] == "n-1"


# --- supplier payments ---------------------------------------------------------------


@pytest.mark.unit
def test_supplier_payment_key_is_type_and_refnbr() -> None:
    out = supplier_payment.normalize_supplier_payment(DM_PAYMENT)

    # (type, refNbr) is unique; refNbr alone is not (a Prepayment and its VoidPayment share it).
    assert out["type"] == "Prepayment"
    assert out["refNbr"] == "300035"


@pytest.mark.unit
def test_supplier_payment_maps_amounts_and_dates() -> None:
    out = supplier_payment.normalize_supplier_payment(DM_PAYMENT)

    assert out["paymentAmount"] == 1250.5
    assert out["unappliedBalance"] == 1250.5
    assert out["released"] is True and out["hold"] is False
    assert out["applicationDate"] == "2026-05-08T00:00:00"
    assert out["lastModifiedDateTime"] == "2026-05-08T14:23:58.953"
    assert json.loads(out["currency"]) == {"id": "NOK"}
    assert json.loads(out["supplier"]) == {"internalId": 206886}


@pytest.mark.unit
def test_supplier_payment_does_not_pass_off_internal_ids_as_erp_codes() -> None:
    out = supplier_payment.normalize_supplier_payment(DM_PAYMENT)

    # DataMart has the cash account's internal id (14675); the ERP field holds its code.
    assert out["cashAccount"] is None
    assert json.loads(out["metadata"])["cashAccountId"] == 14675


# --- project tasks -------------------------------------------------------------------


@pytest.mark.unit
def test_project_task_key_is_project_and_task_code() -> None:
    out = project_task.normalize_project_task(DM_TASK)

    # The task code alone repeats across projects (66 codes over 3,108 tasks).
    assert out["taskId"] == "3"
    assert out["projectInternalId"] == 4711
    assert out["internalId"] == 90210


@pytest.mark.unit
def test_project_task_maps_dates_and_nests_rate_table() -> None:
    out = project_task.normalize_project_task(DM_TASK)

    assert out["plannedStart"] == "2026-01-05T00:00:00"
    assert out["plannedEnd"] == "2026-03-01T00:00:00"
    assert out["startDate"] == "2026-01-06T00:00:00"
    assert out["endDate"] is None
    assert out["lastModifiedDateTime"] == "2026-02-19T13:27:34.987"
    assert json.loads(out["rateTable"]) == {"id": "STD"}


@pytest.mark.unit
def test_project_task_created_date_is_null_not_the_datamart_load_time() -> None:
    out = project_task.normalize_project_task(DM_TASK)

    assert out["createdDateTime"] is None
    assert json.loads(out["metadata"])["createdInDataMartDateTime"] == "2026-01-01T00:00:00Z"


@pytest.mark.unit
@pytest.mark.parametrize(
    ("fn", "module"),
    [
        (supplier.normalize_supplier, supplier),
        (supplier_payment.normalize_supplier_payment, supplier_payment),
        (project_task.normalize_project_task, project_task),
    ],
)
def test_every_legacy_field_is_present_on_empty_input(fn, module) -> None:
    assert set(module.LEGACY_FIELDS) <= set(fn({}))
