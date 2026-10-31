"""Endpoint-scoped lifecycle regressions for raw observation staging."""

import json

import pytest

from etl.tests.test_fresh_staging import RUN_ID, _build, _meta, _record


@pytest.mark.parametrize(
    "endpoint,field,status,deleted",
    [
        (endpoint, "Status", "DELETED", True)
        for endpoint in (
            "accounts bank_transactions bank_transfers batch_payments contact_groups credit_notes "
            "expense_claims invoices manual_journals payments purchase_orders quotes repeating_invoices "
            "tax_rates tracking_categories"
        ).split()
    ]
    + [
        ("invoices", "Status", "VOIDED", False),
        ("invoices", "Status", "AUTHORISED", False),
        ("invoices", "Status", "deleted", False),
        ("accounts", "Status", "ARCHIVED", False),
        ("contacts", "ContactStatus", "ARCHIVED", False),
        ("contacts", "ContactStatus", "DELETED", False),
        ("assets_assets", "assetStatus", "Disposed", False),
        ("assets_assets", "assetStatus", "DISPOSED", False),
        ("assets_assets", "assetStatus", "DELETED", False),
        ("projects_projects", "status", "CLOSED", False),
        ("projects_tasks", "status", "LOCKED", False),
        ("projects_projects", "status", "DELETED", False),
        ("organisations", "OrganisationStatus", "ACTIVE", False),
        ("prepayments", "Status", "DELETED", False),
        ("overpayments", "Status", "DELETED", False),
        ("receipts", "Status", "DELETED", False),
        ("linked_transactions", "Status", "DELETED", False),
    ],
)
def test_source_deleted_only_means_documented_endpoint_lifecycle(endpoint, field, status, deleted):
    row = _build([_record({field: status})], _meta(**{"x-endpoint": endpoint}))[
        "v2_record_versions"
    ][0]
    assert row["source_status"] == status
    assert row["source_deleted"] is deleted


@pytest.mark.parametrize(
    "endpoint,field",
    [
        ("invoices", "DeliveryStatus"),
        ("invoices", "ContactStatus"),
        ("invoices", "AccountStatus"),
        ("contacts", "Status"),
        ("assets_assets", "Status"),
        ("projects_projects", "ProjectStatus"),
        ("unknown_endpoint", "Status"),
        ("attachment_metadata", "Status"),
        ("accounting_get_getInvoice", "Status"),
    ],
)
@pytest.mark.parametrize("status", ["DELETED", "OK"])
def test_unregistered_or_wrong_family_status_fields_never_assert_lifecycle(endpoint, field, status):
    payload = {field: status}
    record_id = RUN_ID if endpoint.startswith("accounting_get_") else "invoice-1"
    row = _build([_record(payload, record_id=record_id)], _meta(**{"x-endpoint": endpoint}))[
        "v2_record_versions"
    ][0]
    assert row["source_status"] is None and row["source_deleted"] is False
    assert json.loads(row["payload_json"]) == payload


def test_auxiliary_deleted_status_cannot_override_known_lifecycle_status():
    payload = {"Status": "AUTHORISED", "DeliveryStatus": "DELETED", "ContactStatus": "DELETED"}
    row = _build([_record(payload)])["v2_record_versions"][0]
    assert row["source_status"] == "AUTHORISED" and row["source_deleted"] is False
    assert json.loads(row["payload_json"]) == payload


def test_deleted_boolean_and_absent_collections_do_not_invent_source_deletion():
    result = _build([_record({"Deleted": True, "IsDeleted": True, "Lines": None})])
    assert result["v2_record_versions"][0]["source_deleted"] is False
    assert result["v2_collection_versions"] == []
