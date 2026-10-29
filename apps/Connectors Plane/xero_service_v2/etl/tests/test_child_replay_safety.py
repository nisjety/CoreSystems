"""Typed child rows carry the observation time used by the staging MERGE."""

from datetime import datetime, timezone

import pytest

from etl.common.bq_writer import BQWriter
from etl.xero import (
    bank_transactions, branding_themes, contacts, credit_notes, invoices,
    journals, manual_journals, overpayments, purchase_orders, quotes,
    reports, tracking_categories,
)


CHILD_CASES = (
    (invoices.parse_lines, {"InvoiceID": "i", "LineItems": [{"LineItemID": "l"}]}),
    (invoices.parse_payments, {"InvoiceID": "i", "Payments": [{"PaymentID": "p"}]}),
    (bank_transactions.parse_lines, {"BankTransactionID": "b", "LineItems": [{"LineItemID": "l"}]}),
    (credit_notes.parse_lines, {"CreditNoteID": "c", "LineItems": [{"LineItemID": "l"}]}),
    (credit_notes.parse_allocations, {"CreditNoteID": "c", "Allocations": [{"AllocationID": "a"}]}),
    (contacts.parse_addresses, {"ContactID": "c", "Addresses": [{"AddressType": "STREET"}]}),
    (contacts.parse_phones, {"ContactID": "c", "Phones": [{"PhoneType": "DEFAULT", "PhoneNumber": "123"}]}),
    (contacts.parse_group_memberships, {"ContactID": "c", "ContactGroups": [{"ContactGroupID": "g"}]}),
    (quotes.parse_lines, {"QuoteID": "q", "LineItems": [{"LineItemID": "l"}]}),
    (purchase_orders.parse_lines, {"PurchaseOrderID": "p", "LineItems": [{"LineItemID": "l"}]}),
    (manual_journals.parse_lines, {"ManualJournalID": "m", "JournalLines": [{"AccountID": "a"}]}),
    (journals.parse_lines, {"JournalID": "j", "JournalLines": [{"JournalLineID": "l"}]}),
    (overpayments.parse_lines, {"OverpaymentID": "o", "LineItems": [{"LineItemID": "l"}]}),
    (tracking_categories.parse_options, {"TrackingCategoryID": "t", "Options": [{"TrackingOptionID": "o"}]}),
    (branding_themes.parse_services, {"BrandingThemeID": "b", "PaymentServices": [{"PaymentServiceID": "s"}]}),
    (reports.parse_rows, {"Rows": [{"RowType": "Row", "Cells": [{"Value": "total"}]}]}),
)


@pytest.mark.parametrize("parse_child,payload", CHILD_CASES, ids=lambda value: getattr(value, "__qualname__", None))
def test_child_rows_keep_newest_observation(parse_child, payload):
    older = datetime(2026, 1, 1, tzinfo=timezone.utc)
    newer = datetime(2026, 1, 2, tzinfo=timezone.utc)
    base = {"tenant_id": "tenant", "record_id": "parent", "payload": payload,
            "meta": {"x-report": "profit_and_loss", "x-run-id": "run"}}
    old_row = parse_child({**base, "last_seen_at": older})[0]
    new_row = parse_child({**base, "last_seen_at": newer})[0]

    assert old_row["synced_at"] == older
    assert new_row["synced_at"] == newer
    key_columns = tuple(column for column in new_row if column != "synced_at")
    assert BQWriter._dedup_by_key([new_row, old_row], key_columns) == [new_row]
