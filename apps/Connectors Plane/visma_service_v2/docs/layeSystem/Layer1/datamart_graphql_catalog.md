# DataMart GraphQL Catalog

Generated from the live DataMart introspection snapshot.

- live query roots: 53
- endpoint configs with DataMart blocks: 71
- direct config mappings: 22
- alias-based mappings: 0
- unmapped config placeholders: 49
- live query roots without local endpoint mapping: 31
- query output directory: `config/queries/datamart`

## Live Query Roots

| query_root | return_type | args | file |
|---|---|---|---|
| `accountById` | `Account` | `id` | `config/queries/datamart/account_by_id.graphql` |
| `accounts` | `Account` | `lastId, publicCode1, publicCode2, externalCode1, externalCode2, active, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/accounts.graphql` |
| `addresses` | `Address` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/addresses.graphql` |
| `aggregatedSales` | `AggregatedSales` | `inventoryItemId, siteId` | `config/queries/datamart/aggregated_sales.graphql` |
| `apInvoiceLines` | `APInvoiceLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/ap_invoice_lines.graphql` |
| `apInvoices` | `APInvoice` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/ap_invoices.graphql` |
| `apPaymentLines` | `APPaymentLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/ap_payment_lines.graphql` |
| `apPayments` | `APPayment` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/ap_payments.graphql` |
| `arHistoricalData` | `ARHistoricalData` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize` | `config/queries/datamart/ar_historical_data.graphql` |
| `branches` | `Branch` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/branches.graphql` |
| `budgetById` | `Budget` | `id` | `config/queries/datamart/budget_by_id.graphql` |
| `budgets` | `Budget` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted, includeUnreleased` | `config/queries/datamart/budgets.graphql` |
| `contacts` | `Contact` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/contacts.graphql` |
| `countries` | `Country` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/countries.graphql` |
| `customerDocumentLines` | `CustomerDocumentLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/customer_document_lines.graphql` |
| `customerDocuments` | `CustomerDocument` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/customer_documents.graphql` |
| `customerPaymentLines` | `CustomerPaymentLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted, includeCashDocumentTypes` | `config/queries/datamart/customer_payment_lines.graphql` |
| `customerPayments` | `CustomerPayment` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted, includeCashDocumentTypes` | `config/queries/datamart/customer_payments.graphql` |
| `customers` | `Customer` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/customers.graphql` |
| `employees` | `Employee` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/employees.graphql` |
| `financialPeriods` | `FinancialPeriod` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/financial_periods.graphql` |
| `generalLedgerBalanceById` | `GeneralLedgerBalance` | `id` | `config/queries/datamart/general_ledger_balance_by_id.graphql` |
| `generalLedgerBalances` | `GeneralLedgerBalance` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/general_ledger_balances.graphql` |
| `generalLedgerBalancesByPeriod` | `GeneralLedgerBalance` | `finPeriodId, accountId, subaccountId, lastId, updatedInDataMart, updatedInDataMartTo, pageSize, toggleBalanceSign, includeDeleted` | `config/queries/datamart/general_ledger_balances_by_period.graphql` |
| `generalLedgerBalancesByPeriodV2` | `GeneralLedgerBalanceGroupPage` | `finPeriodId, accountId, subaccountId, lastGroupKey, updatedInDataMart, updatedInDataMartTo, pageSize, toggleBalanceSign, includeDeleted` | `config/queries/datamart/general_ledger_balances_by_period_v2.graphql` |
| `generalLedgerBalancesForwardByGroup` | `GeneralLedgerBalanceForwardPage` | `lastGroupKey, updatedInDataMart, pageSize, periodsUpTo, toggleBalanceSign, includeDeleted` | `config/queries/datamart/general_ledger_balances_forward_by_group.graphql` |
| `generalLedgerBalancesForwardByGroupV2` | `GeneralLedgerBalanceGroupPage` | `lastGroupKey, updatedInDataMart, pageSize, periodsUpTo, toggleBalanceSign, includeDeleted` | `config/queries/datamart/general_ledger_balances_forward_by_group_v2.graphql` |
| `generalLedgerBalancesV2` | `GeneralLedgerBalance` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, toggleBalanceSign, includeDeleted` | `config/queries/datamart/general_ledger_balances_v2.graphql` |
| `inventoryItems` | `InventoryItem` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/inventory_items.graphql` |
| `journalTransactionById` | `JournalTransaction` | `id` | `config/queries/datamart/journal_transaction_by_id.graphql` |
| `journalTransactions` | `JournalTransaction` | `lastId, accountId, fromFinancialPeriod, toFinancialPeriod, updatedInDataMart, updatedInDataMartTo, pageSize` | `config/queries/datamart/journal_transactions.graphql` |
| `locations` | `Location` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/locations.graphql` |
| `paymentMethods` | `PaymentMethod` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/payment_methods.graphql` |
| `projectTasks` | `ProjectTask` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/project_tasks.graphql` |
| `salesOrderLineById` | `SOLine` | `id` | `config/queries/datamart/sales_order_line_by_id.graphql` |
| `salesOrderLines` | `SOLine` | `lastId, inventoryItemId, siteId, behaviors, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/sales_order_lines.graphql` |
| `salesOrderLinesV2` | `SalesOrderLine` | `lastId, inventoryItemId, siteId, behaviors, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/sales_order_lines_v2.graphql` |
| `salesOrders` | `SalesOrder` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/sales_orders.graphql` |
| `salesPersons` | `SalesPerson` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/sales_persons.graphql` |
| `salesPrices` | `SalesPrice` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/sales_prices.graphql` |
| `states` | `State` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/states.graphql` |
| `subaccounts` | `Subaccount` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/subaccounts.graphql` |
| `supplierClasses` | `SupplierClass` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/supplier_classes.graphql` |
| `supplierDocumentLines` | `SupplierDocumentLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/supplier_document_lines.graphql` |
| `supplierDocuments` | `SupplierDocument` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/supplier_documents.graphql` |
| `supplierPaymentLines` | `SupplierPaymentLine` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/supplier_payment_lines.graphql` |
| `supplierPayments` | `SupplierPayment` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/supplier_payments.graphql` |
| `suppliers` | `Supplier` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/suppliers.graphql` |
| `terms` | `Terms` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/terms.graphql` |
| `vatCategories` | `VatCategory` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/vat_categories.graphql` |
| `vatZones` | `VatZone` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/vat_zones.graphql` |
| `warehouseLocations` | `WarehouseLocation` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/warehouse_locations.graphql` |
| `warehouses` | `Warehouse` | `lastId, updatedInDataMart, updatedInDataMartTo, pageSize, includeDeleted` | `config/queries/datamart/warehouses.graphql` |

## Endpoint Config Mapping

| endpoint | type_name | mapped_query_root | status | generated_query |
|---|---|---|---|---|
| `accounts` | `Account` | `accounts` | direct | `config/queries/datamart/accounts.graphql` |
| `attributes` | `Attribute` | `` | unmapped |  |
| `branches` | `Branch` | `branches` | direct | `config/queries/datamart/branches.graphql` |
| `budgets` | `Budget` | `budgets` | direct | `config/queries/datamart/budgets.graphql` |
| `carriers` | `Carrier` | `` | unmapped |  |
| `cash_accounts` | `CashAccount` | `` | unmapped |  |
| `cash_sales` | `CashSale` | `` | unmapped |  |
| `cash_transactions` | `CashTransaction` | `` | unmapped |  |
| `contacts` | `Contact` | `contacts` | direct | `config/queries/datamart/contacts.graphql` |
| `credit_terms` | `Terms` | `terms` | direct | `config/queries/datamart/terms.graphql` |
| `currencies` | `Currency` | `` | unmapped |  |
| `currency_rates` | `CurrencyRate` | `` | unmapped |  |
| `customer_classes` | `CustomerClass` | `` | unmapped |  |
| `customer_contracts` | `CustomerContract` | `` | unmapped |  |
| `customer_credit_notes` | `CustomerCreditNote` | `` | unmapped |  |
| `customer_debit_notes` | `CustomerDebitNote` | `` | unmapped |  |
| `customer_documents` | `CustomerDocument` | `customerDocuments` | direct | `config/queries/datamart/customer_documents.graphql` |
| `customer_invoices` | `` | `` | unmapped |  |
| `customer_locations` | `CustomerLocation` | `` | unmapped |  |
| `customer_overdue_charges` | `CustomerOverdueCharge` | `` | unmapped |  |
| `customer_payment_methods` | `CustomerPaymentMethod` | `` | unmapped |  |
| `customer_payments` | `CustomerPayment` | `customerPayments` | direct | `config/queries/datamart/customer_payments.graphql` |
| `customer_sales_prices` | `SalesPrice` | `salesPrices` | direct | `config/queries/datamart/sales_prices.graphql` |
| `customers` | `Customer` | `customers` | direct | `config/queries/datamart/customers.graphql` |
| `deferral_codes` | `DeferralCode` | `` | unmapped |  |
| `departments` | `Department` | `` | unmapped |  |
| `dimensions` | `Dimension` | `` | unmapped |  |
| `discount_codes` | `DiscountCode` | `` | unmapped |  |
| `earning_types` | `EarningType` | `` | unmapped |  |
| `employees` | `Employee` | `employees` | direct | `config/queries/datamart/employees.graphql` |
| `expense_claims` | `ExpenseClaim` | `` | unmapped |  |
| `financial_periods` | `FinancialPeriod` | `financialPeriods` | direct | `config/queries/datamart/financial_periods.graphql` |
| `fixed_asset_classes` | `FixedAssetClass` | `` | unmapped |  |
| `fixed_asset_property_tax_groups` | `FixedAssetPropertyTaxGroup` | `` | unmapped |  |
| `fixed_asset_transactions` | `FixedAssetTransaction` | `` | unmapped |  |
| `fixed_assets` | `FixedAsset` | `` | unmapped |  |
| `general_ledger_balances_v2` | `GeneralLedgerBalance` | `generalLedgerBalancesV2` | direct | `config/queries/datamart/general_ledger_balances_v2.graphql` |
| `general_ledger_transactions` | `GeneralLedgerTransaction` | `` | unmapped |  |
| `inventory_adjustments` | `InventoryAdjustment` | `` | unmapped |  |
| `inventory_issues` | `InventoryIssue` | `` | unmapped |  |
| `inventory_items` | `InventoryItem` | `inventoryItems` | direct | `config/queries/datamart/inventory_items.graphql` |
| `inventory_receipts` | `InventoryReceipt` | `` | unmapped |  |
| `inventory_transfers` | `InventoryTransfer` | `` | unmapped |  |
| `journal_transactions` | `JournalTransaction` | `journalTransactions` | direct | `config/queries/datamart/journal_transactions.graphql` |
| `kit_assemblies` | `` | `` | unmapped |  |
| `kit_specifications` | `` | `` | unmapped |  |
| `landed_cost_codes` | `` | `` | unmapped |  |
| `ledgers` | `` | `` | unmapped |  |
| `locations` | `Location` | `locations` | direct | `config/queries/datamart/locations.graphql` |
| `lot_serial_classes` | `` | `` | unmapped |  |
| `number_sequences` | `` | `` | unmapped |  |
| `organizations` | `` | `` | unmapped |  |
| `packaging_types` | `` | `` | unmapped |  |
| `payment_methods` | `PaymentMethod` | `paymentMethods` | direct | `config/queries/datamart/payment_methods.graphql` |
| `price_classes` | `` | `` | unmapped |  |
| `project_account_groups` | `` | `` | unmapped |  |
| `project_budgets` | `` | `` | unmapped |  |
| `project_transactions` | `` | `` | unmapped |  |
| `projects` | `` | `` | unmapped |  |
| `purchase_orders` | `` | `` | unmapped |  |
| `purchase_receipts` | `` | `` | unmapped |  |
| `sales_categories` | `` | `` | unmapped |  |
| `sales_order_types` | `` | `` | unmapped |  |
| `sales_orders_v3` | `SalesOrder` | `salesOrders` | direct | `config/queries/datamart/sales_orders.graphql` |
| `sales_persons` | `SalesPerson` | `salesPersons` | direct | `config/queries/datamart/sales_persons.graphql` |
| `shipments` | `` | `` | unmapped |  |
| `stocktakes_v2` | `` | `` | unmapped |  |
| `subaccounts` | `Subaccount` | `subaccounts` | direct | `config/queries/datamart/subaccounts.graphql` |
| `supplier_classes` | `SupplierClass` | `supplierClasses` | direct | `config/queries/datamart/supplier_classes.graphql` |
| `supplier_documents` | `SupplierDocument` | `supplierDocuments` | direct | `config/queries/datamart/supplier_documents.graphql` |
| `supplier_invoices` | `APInvoice` | `apInvoices` | direct | `config/queries/datamart/ap_invoices.graphql` |

## Live Roots Without Local Endpoint Mapping

- `accountById`
- `addresses`
- `aggregatedSales`
- `apInvoiceLines`
- `apPaymentLines`
- `apPayments`
- `arHistoricalData`
- `budgetById`
- `countries`
- `customerDocumentLines`
- `customerPaymentLines`
- `generalLedgerBalanceById`
- `generalLedgerBalances`
- `generalLedgerBalancesByPeriod`
- `generalLedgerBalancesByPeriodV2`
- `generalLedgerBalancesForwardByGroup`
- `generalLedgerBalancesForwardByGroupV2`
- `journalTransactionById`
- `projectTasks`
- `salesOrderLineById`
- `salesOrderLines`
- `salesOrderLinesV2`
- `states`
- `supplierDocumentLines`
- `supplierPaymentLines`
- `supplierPayments`
- `suppliers`
- `vatCategories`
- `vatZones`
- `warehouseLocations`
- `warehouses`

## Notes

- This catalog follows the live schema exposed by `https://datamart.visma.net/graphql`.
- It uses the current Visma pattern documented in the DataMart docs: `lastId`, `updatedInDataMart`, and `pageSize` for bulk queries.
- Nested selections are intentionally shallow. They are a schema catalog, not yet a guaranteed bronze-parity DTO set.
- Unmapped endpoint configs need either an explicit query-root alias or a decision that the ERP endpoint has no DataMart equivalent.
