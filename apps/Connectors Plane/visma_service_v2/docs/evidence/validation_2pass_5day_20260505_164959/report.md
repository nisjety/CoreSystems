# 2-pass local 5-day validation

- Artifact directory: `docs/evidence/validation_2pass_5day_20260505_164959`
- Tenant: `33f319b9-4d57-11ee-960c-025417856183`
- Window: `2026-04-29T00:00:00Z` .. `2026-05-03T23:59:59Z`
- Endpoint count: total=73, enabled=70, disabled=3
- Main table chosen for counts: `vsv2.local_bronze_record`
- Run1: pass=69, fail=1
- Run2: pass=69, fail=1

## Findings
- Duplicate-risk endpoints (delta2>0): None
- Deletion-risk endpoints (delta1<0 or delta2<0): None
- Zero-increment endpoints (delta1==0): accounts, attributes, branches, budgets, carriers, cash_accounts, cash_sales, cash_transactions, contacts, credit_terms, currencies, currency_rates, customer_classes, customer_contracts, customer_credit_notes, customer_debit_notes, customer_documents, customer_invoices, customer_locations, customer_overdue_charges, customer_payment_methods, customer_payments, customer_sales_prices, customers, deferral_codes, departments, dimensions, discount_codes, earning_types, employees, expense_claims, financial_periods, fixed_asset_classes, fixed_asset_property_tax_groups, fixed_asset_transactions, fixed_assets, general_ledger_balances_v2, general_ledger_transactions, inventory_adjustments, inventory_issues, inventory_items, inventory_receipts, inventory_transfers, journal_transactions, kit_assemblies, kit_specifications, landed_cost_codes, ledgers, locations, lot_serial_classes, number_sequences, organizations, packaging_types, payment_methods, price_classes, project_account_groups, project_budgets, projects, purchase_orders, purchase_receipts, sales_categories, sales_order_types, sales_orders_v3, sales_persons, shipments, stocktakes_v2, subaccounts, supplier_classes, supplier_documents, supplier_invoices

## Top failing endpoints
- `run1` `sales_orders_v3`: [2m2026-05-05T14:54:51.450105Z[0m [31mERROR[0m sync: fetch failed [3mtenant[0m[2m=[0m33f319b9-4d57-11ee-960c-025417856183 [3mendpoint[0m[2m=[0m"sales_orders_v3" [3merr[0m[2m=[0mupstream returned non-success status 401: 
- `run2` `sales_orders_v3`: [2m2026-05-05T14:59:01.616478Z[0m [31mERROR[0m sync: fetch failed [3mtenant[0m[2m=[0m33f319b9-4d57-11ee-960c-025417856183 [3mendpoint[0m[2m=[0m"sales_orders_v3" [3merr[0m[2m=[0mupstream returned non-success status 401: 
