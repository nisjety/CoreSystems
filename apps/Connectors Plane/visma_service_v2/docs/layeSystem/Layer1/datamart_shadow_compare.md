# DataMart Shadow Comparison

Live sample shadow comparison between ERP REST and normalized DataMart
candidate endpoints for the current tenant.

- endpoints compared: 18
- clean_sample: 1
- probe_error: 1
- key_gap: 1
- field_mismatch: 8
- no_key_overlap: 7
- no_live_rows: 0
- no_datamart_rows: 0
- no_erp_rows: 0

| endpoint | status | erp_rows | datamart_rows | overlap | only_in_erp | only_in_dm | mismatch_fields |
|---|---|---:|---:|---:|---:|---:|---|
| `accounts` | `key_gap` | 1228 | 100 | 100 | 514 | 0 | - |
| `branches` | `clean_sample` | 1 | 1 | 1 | 0 | 0 | - |
| `contacts` | `no_key_overlap` | 23448 | 100 | 0 | 11724 | 100 | - |
| `credit_terms` | `field_mismatch` | 22 | 22 | 22 | 0 | 0 | `calculationType`, `dayFrom1`, `dayFrom2`, `dayTo1`, `dayTo2`, `discountType`, `dueDateType`, `id` |
| `customer_documents` | `no_key_overlap` | 100 | 100 | 0 | 100 | 100 | - |
| `customer_payments` | `field_mismatch` | 100 | 100 | 17 | 83 | 83 | `applicationDate`, `branchId`, `currencyId`, `customerId`, `documentType`, `id`, `lastModifiedDateTime`, `locationId` |
| `customer_sales_prices` | `field_mismatch` | 100 | 100 | 9 | 91 | 91 | `currencyId`, `description`, `effectiveDate`, `expirationDate`, `id`, `inventoryId`, `lastModifiedDateTime`, `uom` |
| `customers` | `field_mismatch` | 100 | 100 | 100 | 0 | 0 | `attributes`, `consolidateInvoices`, `corporateId`, `createdDateTime`, `creditTerms`, `customerClassId`, `defaultLocation`, `deliveryAddress` |
| `general_ledger_balances_v2` | `no_key_overlap` | 100 | 100 | 0 | 100 | 100 | - |
| `journal_transactions` | `probe_error` | 0 | 100 | 0 | 0 | 0 | - |
| `locations` | `no_key_overlap` | 100 | 100 | 0 | 100 | 0 | - |
| `payment_methods` | `no_key_overlap` | 8 | 13 | 0 | 8 | 0 | - |
| `sales_orders_v3` | `no_key_overlap` | 100 | 100 | 0 | 100 | 100 | - |
| `sales_persons` | `field_mismatch` | 100 | 100 | 100 | 0 | 0 | `commissionHistory`, `createdDateTime`, `customers`, `lastModifiedDateTime`, `metadata`, `salesSub` |
| `subaccounts` | `field_mismatch` | 100 | 100 | 100 | 0 | 0 | `lastModifiedDateTime`, `metadata`, `segments`, `timeStamp` |
| `supplier_classes` | `no_key_overlap` | 16 | 16 | 0 | 16 | 16 | - |
| `supplier_documents` | `field_mismatch` | 100 | 100 | 2 | 98 | 98 | `accountId`, `appStatus`, `batchNumber`, `branchId`, `cashAccountId`, `createdDateTime`, `detailTotal`, `detailTotalInCurrency` |
| `supplier_invoices` | `field_mismatch` | 100 | 100 | 2 | 98 | 98 | `accountId`, `amount`, `amountInCurrency`, `appStatus`, `batchNumber`, `branchId`, `cashAccountId`, `createdDateTime` |

## `accounts`

- status: `key_gap`
- ERP rows fetched: 1228
- DataMart rows fetched: 100
- overlapping keys: 100
- only in ERP sample: 514
- only in DataMart sample: 0
- ERP probe query: `{}`

## `branches`

- status: `clean_sample`
- ERP rows fetched: 1
- DataMart rows fetched: 1
- overlapping keys: 1
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`

## `contacts`

- status: `no_key_overlap`
- ERP rows fetched: 23448
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 11724
- only in DataMart sample: 100
- ERP probe query: `{}`

## `credit_terms`

- status: `field_mismatch`
- ERP rows fetched: 22
- DataMart rows fetched: 22
- overlapping keys: 22
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`
- mismatched fields: `calculationType`, `dayFrom1`, `dayFrom2`, `dayTo1`, `dayTo2`, `discountType`, `dueDateType`, `id`, `instalmentFrequency`, `instalmentMethod`, `instalmentType`, `lastModifiedDateTime`, `moveDueDateIfOnTheWeekend`, `visibleTo`

## `customer_documents`

- status: `no_key_overlap`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 100
- only in DataMart sample: 100
- ERP probe query: `{}`

## `customer_payments`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 17
- only in ERP sample: 83
- only in DataMart sample: 83
- ERP probe query: `{}`
- mismatched fields: `applicationDate`, `branchId`, `currencyId`, `customerId`, `documentType`, `id`, `lastModifiedDateTime`, `locationId`, `paymentDate`, `postedDateTime`, `referenceNumber`

## `customer_sales_prices`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 9
- only in ERP sample: 91
- only in DataMart sample: 91
- ERP probe query: `{}`
- mismatched fields: `currencyId`, `description`, `effectiveDate`, `expirationDate`, `id`, `inventoryId`, `lastModifiedDateTime`, `uom`

## `customers`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 100
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`
- mismatched fields: `attributes`, `consolidateInvoices`, `corporateId`, `createdDateTime`, `creditTerms`, `customerClassId`, `defaultLocation`, `deliveryAddress`, `deliveryContact`, `id`, `invoiceAddress`, `invoiceContact`, `lastModifiedDateTime`, `location`, `mainAddress`, `mainContact`, `metadata`, `paymentMethods`, `priceClass`, `timeStamp`, `vatRegistrationId`, `vatZone`

## `general_ledger_balances_v2`

- status: `no_key_overlap`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 100
- only in DataMart sample: 100
- ERP probe query: `{"periodId": "202609"}`

## `journal_transactions`

- status: `probe_error`
- ERP rows fetched: 0
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`
- note: ERP probe failed: ERP 400: {"message":"'LastModifiedDateTime' is not allowed to be more than 6 month back in time if 'FinePeriodId' is not defined"}

## `locations`

- status: `no_key_overlap`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 100
- only in DataMart sample: 0
- ERP probe query: `{}`

## `payment_methods`

- status: `no_key_overlap`
- ERP rows fetched: 8
- DataMart rows fetched: 13
- overlapping keys: 0
- only in ERP sample: 8
- only in DataMart sample: 0
- ERP probe query: `{}`

## `sales_orders_v3`

- status: `no_key_overlap`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 0
- only in ERP sample: 100
- only in DataMart sample: 100
- ERP probe query: `{"expand": ["Customer", "FinancialInformation", "LinesExcludingAllocations"]}`

## `sales_persons`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 100
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`
- mismatched fields: `commissionHistory`, `createdDateTime`, `customers`, `lastModifiedDateTime`, `metadata`, `salesSub`

## `subaccounts`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 100
- only in ERP sample: 0
- only in DataMart sample: 0
- ERP probe query: `{}`
- mismatched fields: `lastModifiedDateTime`, `metadata`, `segments`, `timeStamp`

## `supplier_classes`

- status: `no_key_overlap`
- ERP rows fetched: 16
- DataMart rows fetched: 16
- overlapping keys: 0
- only in ERP sample: 16
- only in DataMart sample: 16
- ERP probe query: `{}`

## `supplier_documents`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 2
- only in ERP sample: 98
- only in DataMart sample: 98
- ERP probe query: `{}`
- mismatched fields: `accountId`, `appStatus`, `batchNumber`, `branchId`, `cashAccountId`, `createdDateTime`, `detailTotal`, `detailTotalInCurrency`, `docDate`, `exchangeRate`, `hold`, `id`, `lastModifiedDateTime`, `locationId`, `origInvoiceDate`, `payDate`, `paySelected`, `paymentMethodId`, `paymentRefNo`, `releasedDateTime`, `subaccountId`, `supplierId`, `vatExemptTotal`, `vatExemptTotalInCurrency`, `vatTaxableTotal`, `vatTaxableTotalInCurrency`, `voucherId`, `withHoldingTax`, `withHoldingTaxInCurrency`, `withholdingTax`, `withholdingTaxInCurrency`

## `supplier_invoices`

- status: `field_mismatch`
- ERP rows fetched: 100
- DataMart rows fetched: 100
- overlapping keys: 2
- only in ERP sample: 98
- only in DataMart sample: 98
- ERP probe query: `{}`
- mismatched fields: `accountId`, `amount`, `amountInCurrency`, `appStatus`, `batchNumber`, `branchId`, `cashAccountId`, `createdDateTime`, `docDate`, `id`, `lastModifiedDateTime`, `locationId`, `origInvoiceDate`, `payDate`, `paymentMethodId`, `releasedDateTime`, `subaccountId`, `supplierId`, `voucherId`, `withHoldingTax`, `withHoldingTaxInCurrency`
