"""Customer normalizer.

Solves the DataMart customer shape mismatch without requiring runtime joins.
Where DataMart does not expose the full REST object graph, the normalizer
emits the legacy bronze keys with partial JSON payloads or nulls.
"""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, first_non_empty, keep_legacy_fields

LEGACY_FIELDS = [
    "acceptAutoInvoices",
    "attributes",
    "consolidateInvoices",
    "corporateId",
    "createdDateTime",
    "creditDaysPastDue",
    "creditLimit",
    "creditTerms",
    "creditVerification",
    "currencyId",
    "customerClass",
    "defaultPaymentMethodId",
    "deliveryAddress",
    "deliveryContact",
    "excludeDebtCollection",
    "internalId",
    "invoiceAddress",
    "invoiceContact",
    "invoiceToDefaultLocation",
    "lastModifiedDateTime",
    "location",
    "mainAddress",
    "mainContact",
    "metadata",
    "name",
    "number",
    "paymentMethods",
    "printDunningLetters",
    "printInvoices",
    "printMultiCurrencyStatements",
    "printStatements",
    "sendDunningLettersViaEMail",
    "sendInvoicesByEmail",
    "sendStatementsByEmail",
    "statementType",
    "status",
    "timeStamp",
    "vatRegistrationId",
    "vatZone",
    "priceClass",
    "addressLine1",
    "addressLine2",
    "country",
    "customerClassId",
    "defaultLocation",
    "exportType",
    "id",
    "postalCode",
    "salesAccountId",
    "salesExportAccountId",
    "salesNonTaxableAccountId",
    "salutation",
    "salesEuAccountId",
]


def normalize_customer_with_locations(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart Customer record to v1 visma_customers bronze schema."""
    default_location = compact_json(
        {
            "id": dm_record.get("mainLocationId"),
            "locationId": dm_record.get("mainLocationId"),
        }
    )
    normalized = {
        "acceptAutoInvoices": dm_record.get("acceptAutoInvoices"),
        "corporateId": dm_record.get("corporateId"),
        # DataMart carries the ERP's own createdDateTime on every row; createdInDataMartDateTime
        # is when it loaded the row, so it is never used as a stand-in.
        "createdDateTime": erp_local_timestamp(dm_record.get("createdDateTime")),
        "creditDaysPastDue": dm_record.get("creditDaysPastDue"),
        "creditLimit": dm_record.get("creditLimit"),
        "creditTerms": compact_json({"id": dm_record.get("creditTermsId")}),
        "creditVerification": dm_record.get("creditVerification"),
        "currencyId": dm_record.get("currencyId"),
        "customerClass": compact_json(
            {
                "id": dm_record.get("customerClassId"),
                "description": dm_record.get("customerClassDescription"),
            }
        ),
        "customerClassId": dm_record.get("customerClassId"),
        "defaultPaymentMethodId": dm_record.get("defaultPaymentMethodId"),
        "defaultLocation": default_location,
        "excludeDebtCollection": dm_record.get("excludeDebtCollection"),
        "id": dm_record.get("customerId"),
        "internalId": dm_record.get("customerId"),
        "invoiceAddress": compact_json({"addressId": dm_record.get("billAddressId")}),
        "invoiceContact": compact_json({"contactId": dm_record.get("billContactId")}),
        "invoiceToDefaultLocation": dm_record.get("invoiceToDefaultLocation"),
        # updatedInDataMartDateTime is true UTC while this field is Oslo wall-clock time, so it is
        # not a fallback.
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        # ERP location.id is a code ("HOVED"); DataMart only has the internal number (kept in
        # metadata), and an internal id in a code field is a wrong value on every row.
        "location": None,
        "mainAddress": compact_json({"addressId": dm_record.get("mainAddressId")}),
        "mainContact": compact_json({"contactId": dm_record.get("mainContactId")}),
        "metadata": compact_json(
            {
                "accountReference": dm_record.get("accountReference"),
                "attributesId": dm_record.get("attributesId"),
                "billAddressId": dm_record.get("billAddressId"),
                "billContactId": dm_record.get("billContactId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "deletedDatabaseRecord": dm_record.get("deletedDatabaseRecord"),
                "mainAddressId": dm_record.get("mainAddressId"),
                "mainContactId": dm_record.get("mainContactId"),
                "mainLocationId": dm_record.get("mainLocationId"),
                "note": dm_record.get("note"),
                "parentCustomer": dm_record.get("parentCustomer"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "name": first_non_empty(dm_record, "name", "customerName"),
        "number": first_non_empty(dm_record, "customerNumber", "id"),
        "printInvoices": dm_record.get("printInvoices"),
        "printDunningLetters": dm_record.get("printDunningLetters"),
        "printMultiCurrencyStatements": dm_record.get("printMultiCurrencyStatements"),
        "printStatements": dm_record.get("printStatements"),
        "sendInvoicesByEmail": dm_record.get("sendInvoicesByEmail"),
        "sendDunningLettersViaEMail": dm_record.get("sendDunningLettersViaEmail"),
        "sendStatementsByEmail": dm_record.get("sendStatementsByEmail"),
        "statementType": dm_record.get("statementType"),
        "status": dm_record.get("status"),
        # ERP timeStamp is a row version ("AAAAADE6vFc="); DataMart has no equivalent.
        "timeStamp": None,
        "vatRegistrationId": dm_record.get("vatRegistrationId"),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
