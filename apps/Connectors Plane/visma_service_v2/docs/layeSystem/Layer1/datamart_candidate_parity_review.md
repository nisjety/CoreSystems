# DataMart Candidate Bronze Parity Review

Live review of the synchronized Layer 1 DataMart mappings against:

- current endpoint query files
- bronze Dataform declaration schemas
- registered normalizers
- one live sample pull per endpoint

- candidates reviewed: 19
- blocked_live_sample: 1
- candidate_for_shadow: 18

## Summary

| endpoint | bronze_table | sample | normalizer | delivered/bronze | missing | status |
|---|---|---:|---|---:|---:|---|
| `accounts` | `visma_accounts` | ok | `normalize_account` | 21/18 | 0 | `candidate_for_shadow` |
| `branches` | `visma_branches` | ok | `normalize_branch` | 9/9 | 0 | `candidate_for_shadow` |
| `budgets` | `visma_budgets` | fail | `normalize_budget` | 13/13 | 1 | `blocked_live_sample` |
| `contacts` | `visma_contacts` | ok | `normalize_contact` | 31/31 | 0 | `candidate_for_shadow` |
| `credit_terms` | `visma_credit_terms` | ok | `normalize_credit_term` | 27/26 | 0 | `candidate_for_shadow` |
| `customer_documents` | `visma_customer_documents` | ok | `normalize_customer_document` | 63/54 | 0 | `candidate_for_shadow` |
| `customer_payments` | `visma_customer_payments` | ok | `normalize_customer_payment` | 39/29 | 0 | `candidate_for_shadow` |
| `customer_sales_prices` | `visma_customer_sales_prices` | ok | `normalize_customer_sales_price` | 20/18 | 0 | `candidate_for_shadow` |
| `customers` | `visma_customers` | ok | `normalize_customer_with_locations` | 53/53 | 0 | `candidate_for_shadow` |
| `general_ledger_balances_v2` | `visma_general_ledger_balances_v2` | ok | `normalize_general_ledger_balance` | 19/19 | 0 | `candidate_for_shadow` |
| `journal_transactions` | `visma_journal_transactions` | ok | `-` | 55/55 | 0 | `candidate_for_shadow` |
| `locations` | `visma_locations` | ok | `normalize_location` | 8/8 | 0 | `candidate_for_shadow` |
| `payment_methods` | `visma_payment_methods` | ok | `-` | 11/11 | 0 | `candidate_for_shadow` |
| `sales_orders_v3` | `visma_sales_orders_v3` | ok | `normalize_sales_order` | 105/105 | 0 | `candidate_for_shadow` |
| `sales_persons` | `visma_sales_persons` | ok | `normalize_sales_person` | 10/10 | 0 | `candidate_for_shadow` |
| `subaccounts` | `visma_subaccounts` | ok | `normalize_subaccount` | 8/8 | 0 | `candidate_for_shadow` |
| `supplier_classes` | `visma_supplier_classes` | ok | `-` | 15/15 | 0 | `candidate_for_shadow` |
| `supplier_documents` | `visma_supplier_documents` | ok | `-` | 56/56 | 0 | `candidate_for_shadow` |
| `supplier_invoices` | `visma_supplier_invoices` | ok | `-` | 56/56 | 0 | `candidate_for_shadow` |

## Detailed Review

### `accounts`

- bronze table: `visma_accounts`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_accounts.sqlx`
- query file: `config/queries/account.graphql`
- DataMart root: `accounts`
- live sample: ok
- normalizer: `normalize_account`
- normalizer registered: yes
- field parity: 21 delivered vs 18 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_account.sqlx`
- extra delivered fields not in bronze declaration: `analysisCode`, `externalCode1`, `externalCode2`

### `branches`

- bronze table: `visma_branches`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_branches.sqlx`
- query file: `config/queries/branch.graphql`
- DataMart root: `branches`
- live sample: ok
- normalizer: `normalize_branch`
- normalizer registered: yes
- field parity: 9 delivered vs 9 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_branch.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_company.sqlx`

### `budgets`

- bronze table: `visma_budgets`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_budgets.sqlx`
- query file: `config/queries/budget.graphql`
- DataMart root: `budgets`
- live sample: failed
- sample error: `live query returned no sample rows`
- normalizer: `normalize_budget`
- normalizer registered: yes
- field parity: 13 delivered vs 13 bronze fields
- status: `blocked_live_sample`
- missing bronze fields: `createdInDataMartDateTime`
- extra delivered fields not in bronze declaration: `metadata`

### `contacts`

- bronze table: `visma_contacts`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_contacts.sqlx`
- query file: `config/queries/contact.graphql`
- DataMart root: `contacts`
- live sample: ok
- normalizer: `normalize_contact`
- normalizer registered: yes
- field parity: 31 delivered vs 31 bronze fields
- status: `candidate_for_shadow`

### `credit_terms`

- bronze table: `visma_credit_terms`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_credit_terms.sqlx`
- query file: `config/queries/credit_term.graphql`
- DataMart root: `terms`
- live sample: ok
- normalizer: `normalize_credit_term`
- normalizer registered: yes
- field parity: 27 delivered vs 26 bronze fields
- status: `candidate_for_shadow`
- extra delivered fields not in bronze declaration: `createdDateTime`

### `customer_documents`

- bronze table: `visma_customer_documents`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_customer_documents.sqlx`
- query file: `config/queries/customer_document.graphql`
- DataMart root: `customerDocuments`
- live sample: ok
- normalizer: `normalize_customer_document`
- normalizer registered: yes
- field parity: 63 delivered vs 54 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_general_ledger.sqlx`
- extra delivered fields not in bronze declaration: `account`, `branch`, `creditTerms`, `customer`, `location`, `paymentMethod`, `project`, `salesPerson`, `subAccount`

### `customer_payments`

- bronze table: `visma_customer_payments`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_customer_payments.sqlx`
- query file: `config/queries/customer_payment.graphql`
- DataMart root: `customerPayments`
- live sample: ok
- normalizer: `normalize_customer_payment`
- normalizer registered: yes
- field parity: 39 delivered vs 29 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_customer_payment.sqlx`
- extra delivered fields not in bronze declaration: `availableBalanceInCurrency`, `branch`, `cashAccount`, `currency`, `customer`, `location`, `paymentAmountInCurrency`, `paymentMethod`, `refNbr`, `type`

### `customer_sales_prices`

- bronze table: `visma_customer_sales_prices`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_customer_sales_prices.sqlx`
- query file: `config/queries/customer_sales_price.graphql`
- DataMart root: `salesPrices`
- live sample: ok
- normalizer: `normalize_customer_sales_price`
- normalizer registered: yes
- field parity: 20 delivered vs 18 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/common/bridge_customer_price_class.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/finance/dim_customer.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/excel/customer_price_class_business_review_pack_4_5.sqlx`
- extra delivered fields not in bronze declaration: `metadata`, `priceType`

### `customers`

- bronze table: `visma_customers`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_customers.sqlx`
- query file: `config/queries/customer.graphql`
- DataMart root: `customers`
- live sample: ok
- normalizer: `normalize_customer_with_locations`
- normalizer registered: yes
- field parity: 53 delivered vs 53 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/common/customer_price_class_gap_report.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/excel/customer_price_class_unmapped_review.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/excel/seed_customer_price_class_mapping.sqlx`

### `general_ledger_balances_v2`

- bronze table: `visma_general_ledger_balances_v2`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_general_ledger_balances_v2.sqlx`
- query file: `config/queries/general_ledger_balance.graphql`
- DataMart root: `generalLedgerBalancesV2`
- live sample: ok
- normalizer: `normalize_general_ledger_balance`
- normalizer registered: yes
- field parity: 19 delivered vs 19 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_general_ledger_balance.sqlx`

### `journal_transactions`

- bronze table: `visma_journal_transactions`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_journal_transactions.sqlx`
- query file: `config/queries/journal_transaction.graphql`
- DataMart root: `journalTransactions`
- live sample: ok
- normalizer: `-`
- normalizer registered: no
- field parity: 55 delivered vs 55 bronze fields
- status: `candidate_for_shadow`

### `locations`

- bronze table: `visma_locations`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_locations.sqlx`
- query file: `config/queries/location.graphql`
- DataMart root: `locations`
- live sample: ok
- normalizer: `normalize_location`
- normalizer registered: yes
- field parity: 8 delivered vs 8 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_location.sqlx`

### `payment_methods`

- bronze table: `visma_payment_methods`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_payment_methods.sqlx`
- query file: `config/queries/payment_method.graphql`
- DataMart root: `paymentMethods`
- live sample: ok
- normalizer: `-`
- normalizer registered: no
- field parity: 11 delivered vs 11 bronze fields
- status: `candidate_for_shadow`

### `sales_orders_v3`

- bronze table: `visma_sales_orders_v3`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_sales_orders_v3.sqlx`
- query file: `config/queries/sales_order.graphql`
- DataMart root: `salesOrders`
- live sample: ok
- normalizer: `normalize_sales_order`
- normalizer registered: yes
- field parity: 105 delivered vs 105 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/common/bridge_customer_price_class.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/finance/dim_customer.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_sales_order.sqlx`

### `sales_persons`

- bronze table: `visma_sales_persons`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_sales_persons.sqlx`
- query file: `config/queries/sales_person.graphql`
- DataMart root: `salesPersons`
- live sample: ok
- normalizer: `normalize_sales_person`
- normalizer registered: yes
- field parity: 10 delivered vs 10 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_sales_person.sqlx`

### `subaccounts`

- bronze table: `visma_subaccounts`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_subaccounts.sqlx`
- query file: `config/queries/subaccount.graphql`
- DataMart root: `subaccounts`
- live sample: ok
- normalizer: `normalize_subaccount`
- normalizer registered: yes
- field parity: 8 delivered vs 8 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/dimentinals/dim_subaccount.sqlx`

### `supplier_classes`

- bronze table: `visma_supplier_classes`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_supplier_classes.sqlx`
- query file: `config/queries/supplier_class.graphql`
- DataMart root: `supplierClasses`
- live sample: ok
- normalizer: `-`
- normalizer registered: no
- field parity: 15 delivered vs 15 bronze fields
- status: `candidate_for_shadow`

### `supplier_documents`

- bronze table: `visma_supplier_documents`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_supplier_documents.sqlx`
- query file: `config/queries/supplier_document.graphql`
- DataMart root: `supplierDocuments`
- live sample: ok
- normalizer: `-`
- normalizer registered: no
- field parity: 56 delivered vs 56 bronze fields
- status: `candidate_for_shadow`

### `supplier_invoices`

- bronze table: `visma_supplier_invoices`
- bronze schema: `../Dataform/definitions/bronze/visma/visma_supplier_invoices.sqlx`
- query file: `config/queries/supplier_invoice.graphql`
- DataMart root: `apInvoices`
- live sample: ok
- normalizer: `-`
- normalizer registered: no
- field parity: 56 delivered vs 56 bronze fields
- status: `candidate_for_shadow`
- downstream refs:
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/gold/common/source_gap_supplier_invoice_line.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_supplier_invoice.sqlx`
  - `/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen /apps/visma_service/Dataform/definitions/silver/visma/facts/fact_supplier_invoice_line.sqlx`

## Interpretation

- `candidate_for_shadow` means the current query and current normalizer shape can be shadow-tested without obvious bronze field loss.
- `blocked_bronze_schema_missing` means the bronze declaration file does not define a parseable field contract yet, so parity cannot be proven.
- `blocked_parity_gap` means the configured normalizer still drops bronze fields that the legacy contract expects.
- `blocked_no_normalizer_and_parity_gap` means the raw GraphQL shape does not match bronze closely enough and no normalizer bridges it.
- `blocked_join_runtime_gap` means the normalizer expects extra join-style context that the loader does not currently provide.
- `blocked_live_sample` means the endpoint could not be verified with a live sample pull.
