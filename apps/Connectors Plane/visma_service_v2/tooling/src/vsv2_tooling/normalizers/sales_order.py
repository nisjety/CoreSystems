"""Sales order normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import (
    compact_json,
    erp_local_timestamp,
    first_non_empty,
    keep_legacy_fields,
    stringify_scalar,
)

LEGACY_FIELDS = [
    "attachments",
    "billing",
    "branch",
    "branchNumber",
    "cancelBy",
    "canceled",
    "cashAccountId",
    "cashDiscountDate",
    "createdBy",
    "currency",
    "currencyRate",
    "customFields",
    "customer",
    "customerId",
    "customerName",
    "customerOrder",
    "customerRefNo",
    "date",
    "description",
    "discountTotal",
    "discountTotalInBaseCurrency",
    "dueDate",
    "emailed",
    "financialInformation",
    "fobPoint",
    "freight",
    "hold",
    "insurance",
    "invoiceDate",
    "isRotRutDeductible",
    "lastModified",
    "lastModifiedDateTime",
    "lines",
    "location",
    "orderId",
    "orderLines",
    "orderNo",
    "orderQty",
    "orderTotal",
    "orderTotalInBaseCurrency",
    "orderType",
    "origin",
    "originalOrderId",
    "originalOrderType",
    "owner",
    "parentCustomer",
    "paymentMethod",
    "paymentReference",
    "paymentSettings",
    "postPeriod",
    "preferredWarehouse",
    "print",
    "priority",
    "project",
    "requestOn",
    "residentialDelivery",
    "salesPerson",
    "saturdayDelivery",
    "shipSeparately",
    "shipVia",
    "shipment",
    "shipments",
    "shipping",
    "shippingScheduledDate",
    "shippingTerms",
    "shippingZone",
    "soBillingAddress",
    "soBillingContact",
    "soShippingAddress",
    "soShippingContact",
    "status",
    "statusDetails",
    "taxTotal",
    "taxTotalInBaseCurrency",
    "terms",
    "timeStamp",
    "totals",
    "type",
    "useReplacementCostForMarginAndProfit",
    "vatExemptTotal",
    "vatExemptTotalInBaseCurrency",
    "vatTaxableTotal",
    "vatTaxableTotalInBaseCurrency",
    "version",
    "discounts",
    "tax",
    "costTotal",
    "costTotalInBaseCurrency",
    "exciseDutyTotal",
    "exciseDutyTotalInBaseCurrency",
    "freightCost",
    "freightCostInBaseCurrency",
    "freightTotal",
    "freightTotalInBaseCurrency",
    "intrastatTransactionType",
    "invoiceId",
    "invoiceSeparately",
    "paymentTotal",
    "paymentTotalInBaseCurrency",
    "scheduledDate",
    "shippingRule",
    "unbilledAmount",
    "unbilledAmountInBaseCurrency",
    "unshippedAmount",
    "unshippedAmountInBaseCurrency",
]


def normalize_sales_order(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart SalesOrder rows to the legacy bronze contract."""
    branch_id = stringify_scalar(dm_record.get("branchId"))
    billing_contact = {"id": stringify_scalar(dm_record.get("soBillingContactId"))}
    shipping_contact = {
        "id": stringify_scalar(dm_record.get("soShippingContactId")),
        "name": dm_record.get("soShippingContactName"),
    }
    billing_address = _address_payload(
        address_id=dm_record.get("soBillingAddressId"),
        line1=dm_record.get("soBillingAddressLine1"),
        line2=dm_record.get("soBillingAddressLine2"),
        line3=dm_record.get("soBillingAddressLine3"),
        postal_code=dm_record.get("soBillingAddressPostalCode"),
        city=dm_record.get("soBillingAddressCity"),
        country_id=dm_record.get("soBillingAddressCountryId"),
        country_name=dm_record.get("soBillingAddressCountryName"),
        county_id=dm_record.get("soBillingAddressCountyId"),
        county_name=dm_record.get("soBillingAddressCountyName"),
    )
    shipping_address = _address_payload(
        address_id=dm_record.get("soShippingAddressId"),
        line1=dm_record.get("soShippingAddressLine1"),
        line2=dm_record.get("soShippingAddressLine2"),
        line3=dm_record.get("soShippingAddressLine3"),
        postal_code=dm_record.get("soShippingAddressPostalCode"),
        city=dm_record.get("soShippingAddressCity"),
        country_id=dm_record.get("soShippingAddressCountryId"),
        country_name=dm_record.get("soShippingAddressCountryName"),
        county_id=dm_record.get("soShippingAddressCountyId"),
        county_name=dm_record.get("soShippingAddressCountyName"),
    )
    location_payload = compact_json(
        {
            "countryId": dm_record.get("locationCountryId"),
            "id": stringify_scalar(dm_record.get("locationId")),
            "locationId": stringify_scalar(dm_record.get("locationId")),
            "name": dm_record.get("locationName"),
        }
    )
    terms_payload = {
        "description": dm_record.get("creditTermsDescription"),
        "id": dm_record.get("creditTermsId"),
    }
    shipping_terms_payload = {
        "description": dm_record.get("shippingTermsDescription"),
        "id": dm_record.get("shippingTermsId"),
    }
    shipping_zone_payload = {
        "description": dm_record.get("shippingZoneDescription"),
        "id": dm_record.get("shippingZoneId"),
    }
    sales_person_payload = {
        "description": dm_record.get("salesPersonDescription"),
        "id": stringify_scalar(dm_record.get("salesPersonId")),
        "name": dm_record.get("salesPersonDescription"),
        "salespersonID": stringify_scalar(dm_record.get("salesPersonId")),
    }
    normalized = {
        "attachments": compact_json([]),
        "billing": compact_json(
            {
                "address": billing_address,
                "contact": billing_contact,
                "overrideAddress": dm_record.get("soBillingAddressOverrideAddress"),
            }
        ),
        "branch": compact_json(
            {
                "branchId": branch_id,
                "id": branch_id,
                "number": branch_id,
            }
        ),
        "branchNumber": branch_id,
        "cancelBy": dm_record.get("cancelBy"),
        "canceled": dm_record.get("canceled"),
        "cashAccountId": stringify_scalar(dm_record.get("cashAccountId")),
        "cashDiscountDate": dm_record.get("cashDiscountDate"),
        "currency": dm_record.get("currency"),
        "currencyRate": dm_record.get("exchangeRate"),
        "customer": compact_json(
            {
                "id": stringify_scalar(dm_record.get("customerId")),
                "location": {
                    "countryId": dm_record.get("locationCountryId"),
                    "id": stringify_scalar(dm_record.get("locationId")),
                    "name": dm_record.get("locationName"),
                },
                "number": dm_record.get("customerNumber"),
            }
        ),
        "customerId": stringify_scalar(dm_record.get("customerId")),
        "customerName": dm_record.get("customerNumber"),
        "customerOrder": dm_record.get("customerOrder"),
        "customerRefNo": dm_record.get("customerRefNbr"),
        "date": dm_record.get("date"),
        "description": dm_record.get("description"),
        "dueDate": dm_record.get("dueDate"),
        "financialInformation": compact_json(
            {
                "cashDiscountDate": dm_record.get("cashDiscountDate"),
                "dueDate": dm_record.get("dueDate"),
                "postPeriod": dm_record.get("postPeriod"),
                "terms": terms_payload,
            }
        ),
        "fobPoint": compact_json(
            {
                "description": dm_record.get("fobPointDescription"),
                "id": dm_record.get("fobPointId"),
            }
        ),
        "hold": dm_record.get("hold"),
        "insurance": dm_record.get("insurance"),
        "invoiceDate": dm_record.get("invoiceDate"),
        "lastModified": erp_local_timestamp(
            first_non_empty(dm_record, "lastModified", "lastModifiedDateTime")
        ),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "lines": compact_json([]),
        "location": location_payload,
        "orderId": dm_record.get("id"),
        "orderLines": compact_json([]),
        "orderNo": dm_record.get("orderNbr"),
        "orderQty": dm_record.get("orderQty"),
        "orderTotal": dm_record.get("orderTotal"),
        "orderTotalInBaseCurrency": dm_record.get("orderTotalInBaseCurrency"),
        "orderType": dm_record.get("orderType"),
        "originalOrderId": dm_record.get("origOrderNbr"),
        "originalOrderType": dm_record.get("origOrderType"),
        "owner": compact_json(
            {
                "id": stringify_scalar(dm_record.get("ownerId")),
                "name": dm_record.get("ownerName"),
            }
        ),
        "paymentSettings": compact_json(
            {
                "cashAccountId": stringify_scalar(dm_record.get("cashAccountId")),
            }
        ),
        "postPeriod": dm_record.get("postPeriod"),
        "preferredWarehouse": compact_json(
            {
                "id": stringify_scalar(dm_record.get("preferredWarehouseId")),
            }
        ),
        "print": compact_json(
            {
                "descriptionOnInvoice": dm_record.get("printDescriptionOnInvoice"),
                "noteOnExternalDocuments": dm_record.get("printNoteOnExternalDocuments"),
                "noteOnInternalDocuments": dm_record.get("printNoteOnInternalDocuments"),
            }
        ),
        "priority": dm_record.get("priority"),
        "project": compact_json({"id": stringify_scalar(dm_record.get("projectId"))}),
        "requestOn": dm_record.get("requestOn"),
        "residentialDelivery": dm_record.get("residentialDelivery"),
        "salesPerson": compact_json(sales_person_payload),
        "saturdayDelivery": dm_record.get("saturdayDelivery"),
        "shipSeparately": dm_record.get("shipSeparately"),
        "shipVia": compact_json({"id": dm_record.get("shipViaCarrierId")}),
        "shipment": compact_json(
            {
                "scheduledDate": dm_record.get("scheduledShipmentDate"),
                "shipComplete": dm_record.get("shipComplete"),
                "warehouseId": stringify_scalar(dm_record.get("preferredWarehouseId")),
            }
        ),
        "shipments": compact_json([]),
        "shipping": compact_json(
            {
                "address": shipping_address,
                "contact": shipping_contact,
                "intrastatTransactionType": {
                    "description": dm_record.get("transactionTypeDescription"),
                    "id": stringify_scalar(dm_record.get("transactionTypeId")),
                },
                "rule": None,
                "scheduledDate": dm_record.get("scheduledShipmentDate"),
                "shipVia": {"id": dm_record.get("shipViaCarrierId")},
                "terms": shipping_terms_payload,
                "zone": shipping_zone_payload,
            }
        ),
        "shippingScheduledDate": dm_record.get("scheduledShipmentDate"),
        "shippingTerms": compact_json(shipping_terms_payload),
        "shippingZone": compact_json(shipping_zone_payload),
        "soBillingAddress": compact_json(billing_address),
        "soBillingContact": compact_json(billing_contact),
        "soShippingAddress": compact_json(shipping_address),
        "soShippingContact": compact_json(shipping_contact),
        "status": dm_record.get("status"),
        "statusDetails": compact_json(
            {
                "templateStatus": dm_record.get("templateStatus"),
                "transactionTypeDescription": dm_record.get("transactionTypeDescription"),
                "transactionTypeId": stringify_scalar(dm_record.get("transactionTypeId")),
            }
        ),
        "taxTotal": dm_record.get("taxTotal"),
        "terms": compact_json(terms_payload),
        # ERP timeStamp is a row version; DataMart has none, and its timestamps are not one.
        "timeStamp": None,
        "totals": compact_json(
            {
                "orderTotal": dm_record.get("orderTotal"),
                "orderTotalInBaseCurrency": dm_record.get("orderTotalInBaseCurrency"),
                "taxTotal": dm_record.get("taxTotal"),
                "taxableTotal": dm_record.get("vatTaxableTotal"),
                "taxExemptTotal": dm_record.get("vatExemptTotal"),
            }
        ),
        "type": dm_record.get("orderType"),
        "vatExemptTotal": dm_record.get("vatExemptTotal"),
        "vatTaxableTotal": dm_record.get("vatTaxableTotal"),
        "invoiceId": dm_record.get("invoiceNbr"),
        "invoiceSeparately": dm_record.get("invoiceSeparately"),
        "scheduledDate": dm_record.get("scheduledShipmentDate"),
        "isRotRutDeductible": False,
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized


def _address_payload(
    *,
    address_id: Any,
    line1: Any,
    line2: Any,
    line3: Any,
    postal_code: Any,
    city: Any,
    country_id: Any,
    country_name: Any,
    county_id: Any,
    county_name: Any,
) -> dict[str, Any]:
    return {
        "addressId": stringify_scalar(address_id),
        "addressLine1": line1,
        "addressLine2": line2,
        "addressLine3": line3,
        "city": city,
        "country": country_name,
        "countryId": country_id,
        "county": county_name,
        "countyId": county_id,
        "id": stringify_scalar(address_id),
        "postalCode": postal_code,
    }
