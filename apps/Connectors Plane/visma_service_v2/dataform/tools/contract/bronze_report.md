# Bronze generation report

72 tables written.

| Table | filled | NULL (v2 lacks) | renamed | added by v2 |
|---|---|---|---|---|
| `visma_accounts` | 18 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_attributes` | 7 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_branches` | 9 | 2 businessUnitId, businessUnitName |  | 9 |
| `visma_carriers` | 4 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_cash_accounts` | 7 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_cash_transactions` | 25 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_contacts` | 26 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_countries` | 2 | 0  |  | 0 |
| `visma_credit_terms` | 22 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_currencies` | 6 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_currency_rates` | 9 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_customer_classes` | 7 | 0  |  | 0 |
| `visma_customer_credit_notes` | 60 | 4 dunningLetterDate, dunningLetterLevel, businessUnitId, businessUnitName |  | 3 |
| `visma_customer_debit_notes` | 58 | 4 businessUnitId, businessUnitName, dunningLetterDate, dunningLetterLevel |  | 3 |
| `visma_customer_documents` | 31 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_customer_invoices` | 71 | 3 businessUnitId, businessUnitName, errorInfo |  | 0 |
| `visma_customer_locations` | 27 | 1 exportType |  | 0 |
| `visma_customer_payment_methods` | 2 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_customer_payments` | 29 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_customer_sales_prices` | 15 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_customers` | 53 | 2 businessUnitId, businessUnitName |  | 4 |
| `visma_deferral_codes` | 8 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_departments` | 6 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_dimensions` | 1 | 0  |  | 0 |
| `visma_discount_codes` | 12 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_earning_types` | 6 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_employees` | 16 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_expense_claims` | 19 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_financial_periods` | 12 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_fixed_asset_classes` | 17 | 0  |  | 0 |
| `visma_fixed_asset_transactions` | 16 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_fixed_assets` | 14 | 2 businessUnitId, businessUnitName |  | 1 |
| `visma_general_ledger_balances_v2` | 20 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_general_ledger_transactions` | 22 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_inventory_adjustments` | 18 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_inventory_issues` | 18 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_inventory_items` | 39 | 2 businessUnitId, businessUnitName | attributes | 5 |
| `visma_inventory_receipts` | 18 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_inventory_transfers` | 16 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_journal_transactions` | 29 | 2 businessUnitId, businessUnitName | batchNbr | 0 |
| `visma_kit_assemblies` | 18 | 2 businessUnitId, businessUnitName |  | 3 |
| `visma_kit_specifications` | 12 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_landed_cost_codes` | 13 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_ledgers` | 9 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_locations` | 14 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_lot_serial_classes` | 12 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_number_sequences` | 5 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_organizations` | 16 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_packaging_types` | 10 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_payment_methods` | 6 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_project_account_groups` | 4 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_project_budgets` | 22 | 0  |  | 0 |
| `visma_project_transactions` | 9 | 2 businessUnitId, businessUnitName |  | 1 |
| `visma_projects` | 34 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_purchase_orders` | 52 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_purchase_receipts` | 25 | 2 businessUnitId, businessUnitName |  | 1 |
| `visma_sales_order_types` | 7 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_sales_orders_v3` | 47 | 4 lines, shipments, businessUnitId, businessUnitName | branchNumber, canceled, cashAccountId, cashDiscountDate, discountTotal, discountTotalInBaseCurrency, dueDate, fobPoint, hold, insurance, invoiceDate, lastModifiedDateTime, orderNo, orderTotalInBaseCurrency, orderType, paymentMethod, paymentReference, postPeriod, preferredWarehouse, priority, residentialDelivery, saturdayDelivery, shipSeparately, shipVia, shippingTerms, shippingZone, soBillingAddress, soBillingContact, soShippingAddress, soShippingContact, taxTotalInBaseCurrency, terms, timeStamp, vatExemptTotal, vatExemptTotalInBaseCurrency, vatTaxableTotal, vatTaxableTotalInBaseCurrency, costTotal, costTotalInBaseCurrency, exciseDutyTotal, exciseDutyTotalInBaseCurrency, freightCost, freightCostInBaseCurrency, freightTotal, freightTotalInBaseCurrency, intrastatTransactionType, invoiceId, invoiceSeparately, paymentTotal, paymentTotalInBaseCurrency, scheduledDate, shippingRule, unbilledAmount, unbilledAmountInBaseCurrency, unshippedAmount, unshippedAmountInBaseCurrency | 3 |
| `visma_sales_persons` | 10 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_shipments` | 42 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_stocktakes_v2` | 12 | 3 referenceNumber, businessUnitId, businessUnitName |  | 0 |
| `visma_subaccounts` | 8 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_supplier_documents` | 34 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_supplier_invoices` | 62 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_supplier_locations` | 13 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_supplier_payments` | 24 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_suppliers` | 38 | 2 businessUnitId, businessUnitName |  | 1 |
| `visma_unit_of_measures` | 5 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_vat_categories` | 6 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_vat_zones` | 4 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_vats` | 24 | 2 businessUnitId, businessUnitName |  | 0 |
| `visma_warehouses` | 18 | 2 businessUnitId, businessUnitName |  | 0 |

## Not generated

* `visma_price_classes`: derived from customers and customer locations (hand-written file)
* `visma_sales_orders_v2`: the v2 sales order API is retired; v2 only has sales_orders_v3
