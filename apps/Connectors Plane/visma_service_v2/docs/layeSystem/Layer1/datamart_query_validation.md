# DataMart Query Validation

- query directory: `config/queries/datamart`
- total query files: 53
- schema-valid files: 53
- paginated-compatible with current runtime: 43
- point-lookup only: 5
- require page-wrapper adapter: 3
- require extra runtime params: 2

| file | root | schema_valid | compatibility | required_extra_args | optional_extra_args |
|---|---|---:|---|---|---|
| `account_by_id.graphql` | `accountById` | yes | `point_lookup` | `id` | `-` |
| `accounts.graphql` | `accounts` | yes | `paginated_compatible` | `-` | `publicCode1, publicCode2, externalCode1, externalCode2, active, updatedInDataMartTo` |
| `addresses.graphql` | `addresses` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `aggregated_sales.graphql` | `aggregatedSales` | yes | `requires_runtime_params` | `inventoryItemId, siteId` | `-` |
| `ap_invoice_lines.graphql` | `apInvoiceLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `ap_invoices.graphql` | `apInvoices` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `ap_payment_lines.graphql` | `apPaymentLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `ap_payments.graphql` | `apPayments` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `ar_historical_data.graphql` | `arHistoricalData` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `branches.graphql` | `branches` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `budget_by_id.graphql` | `budgetById` | yes | `point_lookup` | `id` | `-` |
| `budgets.graphql` | `budgets` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo, includeUnreleased` |
| `contacts.graphql` | `contacts` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `countries.graphql` | `countries` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `customer_document_lines.graphql` | `customerDocumentLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `customer_documents.graphql` | `customerDocuments` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `customer_payment_lines.graphql` | `customerPaymentLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo, includeCashDocumentTypes` |
| `customer_payments.graphql` | `customerPayments` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo, includeCashDocumentTypes` |
| `customers.graphql` | `customers` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `employees.graphql` | `employees` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `financial_periods.graphql` | `financialPeriods` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `general_ledger_balance_by_id.graphql` | `generalLedgerBalanceById` | yes | `point_lookup` | `id` | `-` |
| `general_ledger_balances.graphql` | `generalLedgerBalances` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `general_ledger_balances_by_period.graphql` | `generalLedgerBalancesByPeriod` | yes | `requires_runtime_params` | `finPeriodId` | `accountId, subaccountId, updatedInDataMartTo` |
| `general_ledger_balances_by_period_v2.graphql` | `generalLedgerBalancesByPeriodV2` | yes | `requires_page_adapter` | `finPeriodId` | `accountId, subaccountId, lastGroupKey, updatedInDataMartTo` |
| `general_ledger_balances_forward_by_group.graphql` | `generalLedgerBalancesForwardByGroup` | yes | `requires_page_adapter` | `-` | `lastGroupKey, periodsUpTo` |
| `general_ledger_balances_forward_by_group_v2.graphql` | `generalLedgerBalancesForwardByGroupV2` | yes | `requires_page_adapter` | `-` | `lastGroupKey, periodsUpTo` |
| `general_ledger_balances_v2.graphql` | `generalLedgerBalancesV2` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `inventory_items.graphql` | `inventoryItems` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `journal_transaction_by_id.graphql` | `journalTransactionById` | yes | `point_lookup` | `id` | `-` |
| `journal_transactions.graphql` | `journalTransactions` | yes | `paginated_compatible` | `-` | `accountId, fromFinancialPeriod, toFinancialPeriod, updatedInDataMartTo` |
| `locations.graphql` | `locations` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `payment_methods.graphql` | `paymentMethods` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `project_tasks.graphql` | `projectTasks` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `sales_order_line_by_id.graphql` | `salesOrderLineById` | yes | `point_lookup` | `id` | `-` |
| `sales_order_lines.graphql` | `salesOrderLines` | yes | `paginated_compatible` | `-` | `inventoryItemId, siteId, behaviors, updatedInDataMartTo` |
| `sales_order_lines_v2.graphql` | `salesOrderLinesV2` | yes | `paginated_compatible` | `-` | `inventoryItemId, siteId, behaviors, updatedInDataMartTo` |
| `sales_orders.graphql` | `salesOrders` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `sales_persons.graphql` | `salesPersons` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `sales_prices.graphql` | `salesPrices` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `states.graphql` | `states` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `subaccounts.graphql` | `subaccounts` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `supplier_classes.graphql` | `supplierClasses` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `supplier_document_lines.graphql` | `supplierDocumentLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `supplier_documents.graphql` | `supplierDocuments` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `supplier_payment_lines.graphql` | `supplierPaymentLines` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `supplier_payments.graphql` | `supplierPayments` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `suppliers.graphql` | `suppliers` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `terms.graphql` | `terms` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `vat_categories.graphql` | `vatCategories` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `vat_zones.graphql` | `vatZones` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `warehouse_locations.graphql` | `warehouseLocations` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |
| `warehouses.graphql` | `warehouses` | yes | `paginated_compatible` | `-` | `updatedInDataMartTo` |

## Notes

- `paginated_compatible` means the current adapter can execute the query with `pageSize`, `lastId`, optional watermark, and optional `companyId` only.
- `requires_runtime_params` means the schema requires extra non-null arguments before the query can be used for scheduled ingestion.
- `point_lookup` queries are valid, but they are not bulk-ingestion queries.
- `requires_page_adapter` queries return page objects with `items`, not a list at the root.
