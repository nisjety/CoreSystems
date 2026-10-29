"""Unit tests — normalizers preserve bronze legacy fields."""

from __future__ import annotations

import json

import pytest

from vsv2_tooling.normalizers import (
    REGISTRY,
    account,
    branch,
    budget,
    contact,
    credit_term,
    customer,
    customer_document,
    customer_payment,
    customer_sales_price,
    general_ledger_balance,
    journal_transaction,
    location,
    sales_order,
    sales_person,
    subaccount,
)


@pytest.mark.unit
def test_registry_lookup() -> None:
    assert "normalize_customer_with_locations" in REGISTRY
    assert "normalize_location" in REGISTRY
    assert "normalize_account" in REGISTRY
    assert "normalize_budget" in REGISTRY
    assert "normalize_branch" in REGISTRY
    assert "normalize_contact" in REGISTRY
    assert "normalize_credit_term" in REGISTRY
    assert "normalize_customer_document" in REGISTRY
    assert "normalize_customer_payment" in REGISTRY
    assert "normalize_customer_sales_price" in REGISTRY
    assert "normalize_general_ledger_balance" in REGISTRY
    assert "normalize_journal_transaction" in REGISTRY
    assert "normalize_sales_order" in REGISTRY
    assert "normalize_sales_person" in REGISTRY
    assert "normalize_subaccount" in REGISTRY


@pytest.mark.unit
def test_normalize_location_minimal() -> None:
    out = location.normalize_location(
        {
            "id": "LOC|1",
            "companyId": 99,
            "locationId": "L1",
            "locationName": "Aquatiq HQ",
            "active": True,
            "priceClassId": "PC-A",
            "bAccountId": "BA-99",
            "updatedInDataMartDateTime": "2026-04-28T12:00:00Z",
        }
    )
    assert out["id"] == "LOC|1"
    assert out["companyId"] == 99
    assert out["priceClassId"] == "PC-A"
    assert out["_source"] == "datamart"
    # DataMart's locationId / bAccountId are internal numbers (all 20,290 live rows are ints and
    # none equals an ERP code such as "HOVED"), so they must not sit under the ERP key names.
    assert out["locationId"] is None
    assert out["locationInternalId"] == "L1"
    assert out["bAccountInternalId"] == "BA-99"


@pytest.mark.unit
def test_normalize_customer_with_locations_preserves_legacy_keys() -> None:
    out = customer.normalize_customer_with_locations(
        {
            "customerId": "C1",
            "customerNumber": "10042",
            "customerName": "Aquatiq",
            "currencyId": "NOK",
            "customerClassId": "STD",
            "customerClassDescription": "Standard",
            "mainLocationId": "L1",
            "creditTermsId": "NET30",
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    assert out["number"] == "10042"
    assert json.loads(out["defaultLocation"])["id"] == "L1"
    assert json.loads(out["customerClass"])["description"] == "Standard"
    assert json.loads(out["creditTerms"])["id"] == "NET30"


@pytest.mark.unit
def test_normalize_account_includes_dm_only_fields() -> None:
    out = account.normalize_account(
        {
            "accountCD": "1500",
            "accountId": 1500,
            "accountClassId": "ASSET",
            "groupCd": "14",
            "curyId": "NOK",
            "taxCategoryId": "TAX",
            "useDefaultSubaccount": True,
            "isCashAccount": False,
            "controlAccountModule": "AR",
            "publicCode1": "PUB",
            "description": "Trade receivables",
            "type": "Asset",
            "active": True,
            "externalCode1": "EC1",
            "externalCode2": "EC2",
            "analysisCode": "AN-1",
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    assert out["accountID"] == 1500
    assert out["currency"] == "NOK"
    assert out["cashAccount"] is False
    assert out["externalCode1"] == "EC1"
    assert out["analysisCode"] == "AN-1"


@pytest.mark.unit
def test_normalize_budget_shapes_legacy_nested_fields_from_ids() -> None:
    out = budget.normalize_budget(
        {
            "id": "BUDGET|1",
            "companyId": 99,
            "branchId": 13,
            "groupId": "BG-1",
            "ledgerId": 2,
            "financialYear": "2026",
            "released": True,
            "releasedAmount": 1200.0,
            "accountId": 3000,
            "subaccountId": 42,
            "description": "Travel budget",
            "amount": 1500.0,
            "distributedAmount": 1200.0,
            "periods": [{"periodId": "202601", "amount": 100.0}],
            "lastModifiedDateTime": "2026-05-13T09:00:00Z",
            "updatedInDataMartDateTime": "2026-05-13T10:00:00Z",
            "createdInDataMartDateTime": "2026-05-13T08:00:00Z",
        }
    )
    assert out["financialYear"] == "2026"
    assert json.loads(out["account"])["number"] == "3000"
    assert json.loads(out["subaccount"])["id"] == "42"
    assert json.loads(out["branchNumber"])["number"] == "13"
    assert json.loads(out["periods"])[0]["periodId"] == "202601"
    assert "semanticGap" in json.loads(out["metadata"])


@pytest.mark.unit
def test_normalize_branch_uses_branch_code_as_best_available_label() -> None:
    """branchCode populates number; absent branchName leaves name null.
    bAccountId is not the legal-entity organization id and remains a sidecar.
    """
    out = branch.normalize_branch(
        {
            "branchId": 5,
            "branchCode": "05",
            "bAccountId": 1005,
            "active": True,
            "mainBranch": False,
            "updatedInDataMartDateTime": "2026-05-13T09:00:00Z",
        }
    )
    assert out["number"] == "05"
    assert out["name"] is None
    assert out["organizationId"] is None
    assert out["_dm_b_account_id"] == 1005


@pytest.mark.unit
def test_normalize_branch_uses_live_branch_name() -> None:
    out = branch.normalize_branch(
        {
            "branchId": 5,
            "branchCode": "05",
            "branchName": "Aquatiq Oslo",
        }
    )
    assert out["number"] == "05"
    assert out["name"] == "Aquatiq Oslo"


@pytest.mark.unit
def test_normalize_customer_maps_new_live_fields_without_losing_metadata() -> None:
    out = customer.normalize_customer_with_locations(
        {
            "companyId": 99,
            "customerId": 42,
            "customerNumber": "C42",
            "creditVerification": "CreditLimit",
            "statementType": "OpenItem",
            "attributesId": "note-42",
            "note": "Current note",
            "defaultPaymentMethodId": "BANK",
            "excludeDebtCollection": True,
            "invoiceToDefaultLocation": False,
            "printDunningLetters": True,
            "sendDunningLettersViaEmail": True,
            "deletedDatabaseRecord": False,
        }
    )
    assert out["defaultPaymentMethodId"] == "BANK"
    assert out["excludeDebtCollection"] is True
    assert out["invoiceToDefaultLocation"] is False
    assert out["printDunningLetters"] is True
    assert out["sendDunningLettersViaEMail"] is True
    assert out["creditVerification"] == "CreditLimit"
    assert out["statementType"] == "OpenItem"
    assert json.loads(out["metadata"])["deletedDatabaseRecord"] is False
    assert json.loads(out["metadata"])["attributesId"] == "note-42"
    assert json.loads(out["metadata"])["note"] == "Current note"


@pytest.mark.unit
def test_normalize_contact_passes_through_live_fields() -> None:
    out = contact.normalize_contact(
        {
            "id": "CONTACT|1",
            "companyId": 99,
            "updatedInDataMartDateTime": "2026-05-13T09:00:00Z",
            "contactId": 1,
            "displayName": "Ada Lovelace",
            "email": "ada@example.com",
        }
    )
    assert out["contactId"] == 1
    assert out["displayName"] == "Ada Lovelace"
    assert out["email"] == "ada@example.com"


@pytest.mark.unit
def test_normalize_credit_term_does_not_pass_off_datamart_load_time_as_created() -> None:
    out = credit_term.normalize_credit_term(
        {
            "termsId": "NET30",
            "createdInDataMartDateTime": "2026-05-13T09:00:00Z",
            "updatedInDataMartDateTime": "2026-05-13T10:00:00Z",
        }
    )
    assert out["termsId"] == "NET30"
    # createdInDataMartDateTime is when DataMart loaded the row (all 22 live terms: 2026-02-15),
    # not when the term was created (ERP: 2023-09-12). It must not fill createdDateTime.
    assert out["createdDateTime"] is None
    assert out["createdInDataMartDateTime"] == "2026-05-13T09:00:00Z"  # still kept, as DM-only


@pytest.mark.unit
def test_normalize_customer_document_serializes_nested_compatibility_fields() -> None:
    out = customer_document.normalize_customer_document(
        {
            "referenceNumber": "INV0001",
            "customerId": 42,
            "projectId": 77,
            "branchId": 1,
            "locationId": 9,
            "salesPersonId": 12,
            "attachments": {"name": "invoice.pdf", "fileId": "F1"},
            "updatedInDataMartDateTime": "2026-05-13T09:00:00Z",
        }
    )
    assert json.loads(out["project"])["id"] == "77"
    assert json.loads(out["customer"])["id"] == "42"
    assert json.loads(out["attachments"])["fileId"] == "F1"


@pytest.mark.unit
def test_normalize_customer_payment_adds_legacy_key_aliases() -> None:
    out = customer_payment.normalize_customer_payment(
        {
            "referenceNumber": "PAY0001",
            "documentType": "Payment",
            "customerId": 42,
            "branchId": 1,
            "locationId": 9,
            "cashAccountId": 100,
            "paymentMethodId": "BANK",
            "currencyId": "NOK",
            "updatedInDataMartDateTime": "2026-05-13T09:00:00Z",
        }
    )
    assert out["refNbr"] == "PAY0001"
    assert out["type"] == "Payment"
    assert json.loads(out["customer"])["id"] == "42"
    assert out["branch"] == "1"


@pytest.mark.unit
def test_normalize_customer_sales_price_marks_missing_price_type_explicitly() -> None:
    out = customer_sales_price.normalize_customer_sales_price(
        {
            "recordId": 1,
            "priceCode": "4",
            "updatedInDataMartDateTime": "2026-05-13T09:00:00Z",
        }
    )
    assert out["recordId"] == 1
    assert out["priceType"] is None
    assert "semanticGap" in json.loads(out["metadata"])


@pytest.mark.unit
def test_normalize_general_ledger_balance_supports_json_pk_fields() -> None:
    out = general_ledger_balance.normalize_general_ledger_balance(
        {
            "accountId": 3000,
            "accountType": "Income",
            "branchId": 1,
            "ledgerId": 2,
            "subaccountId": 10,
            "finPeriodId": "202605",
            "curyId": "NOK",
            "balanceType": "actual",
            "periodToDateDebit": 12.5,
            "periodToDateCredit": 0.0,
            "periodToDateDebitInCurrency": 12.5,
            "periodToDateCreditInCurrency": 0.0,
            "yearClosed": False,
            "beginningBalance": 5.0,
            "beginningBalanceInCurrency": 5.0,
            "yearToDateBalance": 17.5,
            "yearToDateBalanceInCurrency": 17.5,
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    assert json.loads(out["account"])["number"] == "3000"
    assert json.loads(out["ledger"])["id"] == "2"
    assert out["subAccountCd"] == "10"


@pytest.mark.unit
def test_normalize_general_ledger_balance_preserves_tombstone() -> None:
    out = general_ledger_balance.normalize_general_ledger_balance(
        {
            "companyId": 99,
            "accountId": 3000,
            "ledgerId": 2,
            "finPeriodId": "202605",
            "deletedDatabaseRecord": True,
        }
    )
    assert json.loads(out["metadata"])["deletedDatabaseRecord"] is True


@pytest.mark.unit
def test_normalize_journal_transaction_maps_to_module_batchnumber_natural_key() -> None:
    """A DataMart JournalTransaction record normalizes to the ERP natural key
    [module, batchNumber]: batchNbr -> batchNumber, module passed through, and
    numeric-only DM ids reshaped into the legacy nested business-key objects."""
    out = journal_transaction.normalize_journal_transaction(
        {
            "id": "JT|1",
            "companyId": 99,
            "module": "GL",
            "batchNbr": 100123,
            "tranType": "Normal",
            "branchId": 13,
            "accountId": 3000,
            "subaccountId": 42,
            "ledgerId": 2,
            "finPeriodId": "202601",
            "tranDate": "2026-05-13T00:00:00Z",
            "refNbr": "REF0001",
            "lineNbr": 1,
            "debitAmt": 150.0,
            "creditAmt": 0.0,
            "curyId": "NOK",
            "tranDesc": "Opening entry",
            "lastModifiedDateTime": "2026-05-13T09:00:00Z",
            "updatedInDataMartDateTime": "2026-05-13T10:00:00Z",
        }
    )
    # ERP natural/primary key [module, batchNumber].
    assert out["module"] == "GL"
    assert out["batchNumber"] == "100123"
    assert out["transactionDate"] == "2026-05-13T00:00:00Z"
    assert out["financialPeriod"] == "202601"
    assert out["referenceNumber"] == "REF0001"
    assert json.loads(out["account"])["number"] == "3000"
    assert json.loads(out["subaccount"])["id"] == "42"
    assert json.loads(out["branch"])["number"] == "13"
    assert json.loads(out["ledger"])["id"] == "2"
    assert out["debitAmount"] == 150.0
    assert out["currency"] == "NOK"
    assert out["status"] is None
    assert "semanticGap" in json.loads(out["metadata"])
    assert out["_source"] == "datamart"


@pytest.mark.unit
def test_normalize_sales_person_maps_primary_key_to_legacy_field() -> None:
    out = sales_person.normalize_sales_person(
        {
            "salesPersonId": 7,
            "salesPersonCd": "007",
            "name": "Ada",
            "isActive": True,
            "commissionPct": 3.5,
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    assert out["salespersonID"] == "007"
    assert out["commissionPct"] == 3.5


@pytest.mark.unit
def test_normalize_subaccount_builds_segments_json() -> None:
    out = subaccount.normalize_subaccount(
        {
            "subaccountId": 11,
            "subaccountCd": "010200",
            "active": True,
            "description": "BU 02",
            "cd01": "01",
            "description01": "IC",
            "cd02": "02",
            "description02": "BU",
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    segments = json.loads(out["segments"])
    assert out["subaccountNumber"] == "010200"
    assert segments[0]["segmentValue"] == "01"
    assert segments[1]["segmentValueDescription"] == "BU"


@pytest.mark.unit
def test_normalize_sales_order_supplies_empty_line_arrays() -> None:
    out = sales_order.normalize_sales_order(
        {
            "id": "SO|1",
            "orderNbr": "SO0001",
            "orderType": "SO",
            "customerId": 42,
            "customerNumber": "C00042",
            "branchId": 1,
            "locationId": 5,
            "locationName": "HQ",
            "status": "Open",
            "date": "2026-05-12T00:00:00Z",
            "orderTotal": 100.0,
            "taxTotal": 25.0,
            "vatTaxableTotal": 100.0,
            "vatExemptTotal": 0.0,
            "orderQty": 2.0,
            "updatedInDataMartDateTime": "2026-05-12T09:00:00Z",
        }
    )
    assert out["orderId"] == "SO|1"
    assert out["orderNo"] == "SO0001"
    assert out["orderLines"] == "[]"
    assert out["lines"] == "[]"
