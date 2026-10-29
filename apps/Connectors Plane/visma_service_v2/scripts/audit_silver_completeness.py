#!/usr/bin/env python3
"""Completeness audit of the v2 Dataform project: staging -> bronze -> silver -> gold.

Two records were dropped silently in silver because a model deduplicated on a key that is coarser
than the staging key (sales orders without the order type, project transaction lines without the
period slice). The comparison with the old project never showed it: the old project had the same
SQL. This script compares every silver and gold model with the source it reads, in the warehouse
`aquatiq-datavarehus` (BigQuery location europe-north2), with SELECT queries only:

  silver   per tenant, the distinct source keys of the non-deleted bronze rows (the primary_key of
           config/endpoints/*.json is the staging key; a line model counts the elements of the JSON
           array it unnests) against the rows of the silver model
  gold     per company, the rows of the silver model against the rows of the gold table
  keys     every business key of a gold fact against the dimension it points to (orphans)
  staging  row counts of staging against bronze, and the deleted source rows silver can lose

Each check has a class: exact (no difference), A (defect: A-drop rows lost, A-key a key that does
not join, A-null a key or attribute that is always NULL), B (intended filter or aggregation) or
derived (no source to compare with). The exit status is 1 when a check no longer matches its class,
so a new defect and a fixed one both show up. Causes and fixes: docs/silver_completeness.md.

Usage:
  scripts/audit_silver_completeness.py                  the table (silver, gold, keys, staging)
  scripts/audit_silver_completeness.py --layer silver   one of silver, gold, keys, staging
  scripts/audit_silver_completeness.py --tenants        also list the tenants that differ

Needs working Google credentials. Nothing is written to BigQuery.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from google.cloud import bigquery

ROOT = Path(__file__).resolve().parent.parent
PROJECT, LOCATION = "aquatiq-datavarehus", "europe-north2"
STAGING, BRONZE, SILVER = "dw_staging_visma", "visma_bronze", "visma_silver"
GF, GS, GC, DM = "visma_gold_finance", "visma_gold_sales", "visma_gold_common", "visma_datamart"
GOLD = {"finance": GF, "sales": GS, "common": GC, "datamart": DM}
WORKERS = 8
LIVE = "WHERE isDeleted IS DISTINCT FROM TRUE GROUP BY 1, 2"


def tbl(dataset: str, table: str) -> str:
    return f"`{PROJECT}.{dataset}.{table}`"


# A source query returns (k, label, n) and a model query (k, n): k joins the two sides (the tenant
# id, the tenant id and a type, or the hash gold stores as company_key), label names the tenant.


def src_distinct(table: str, *cols: str) -> str:
    """Distinct staging keys per tenant over the non-deleted rows of a bronze table."""
    count = f"COUNT(DISTINCT FORMAT('{'|'.join(['%t'] * len(cols))}', {', '.join(cols)}))"
    return f"SELECT tenantId AS k, tenantId AS label, {count} AS n FROM {tbl(BRONZE, table)} {LIVE}"


def src_array(table: str, column: str) -> str:
    """Elements of a JSON array column per tenant over the non-deleted bronze rows."""
    count = f"SUM(ARRAY_LENGTH(JSON_QUERY_ARRAY({column})))"
    return f"SELECT tenantId AS k, tenantId AS label, {count} AS n FROM {tbl(BRONZE, table)} {LIVE}"


def src_typed(parts: list[tuple[str, str, str]], what: str) -> str:
    """Several bronze tables unioned, the movement type in k. parts = (type, table, column)."""
    return " UNION ALL ".join(
        f"SELECT CONCAT(tenantId, '|{kind}') AS k, CONCAT(SUBSTR(tenantId, 1, 8), ' {kind}') AS "
        f"label, {what.format(col=col)} AS n FROM {tbl(BRONZE, table)} {LIVE}"
        for kind, table, col in parts
    )


def rows_by_tenant(dataset: str, table: str, typed: bool = False) -> str:
    key = "CONCAT(tenant_id, '|', movement_type)" if typed else "tenant_id"
    return f"SELECT {key} AS k, COUNT(*) AS n FROM {tbl(dataset, table)} GROUP BY 1"


def label_rows(dataset: str, table: str, where: str = "") -> str:
    """Rows per tenant as the source side (a gold dimension compared with the silver one)."""
    select = "SELECT tenant_id AS k, tenant_id AS label, COUNT(*) AS n"
    return f"{select} FROM {tbl(dataset, table)} {where} GROUP BY 1, 2"


def by_company(table: str) -> str:
    """Silver rows per company, k being the hash that gold stores as company_key."""
    key = "CAST(FARM_FINGERPRINT(CAST(tenant_id AS STRING)) AS STRING)"
    select = f"SELECT {key} AS k, tenant_id AS label, COUNT(*) AS n"
    return f"{select} FROM {tbl(SILVER, table)} GROUP BY 1, 2"


def gold_by_company(dataset: str, table: str) -> str:
    select = "SELECT CAST(company_key AS STRING) AS k, COUNT(*) AS n"
    return f"{select} FROM {tbl(dataset, table)} GROUP BY 1"


def single(sql: str) -> str:
    return f"SELECT 'all' AS k, 'all' AS label, ({sql}) AS n"


def single_model(dataset: str, table: str) -> str:
    return f"SELECT 'all' AS k, (SELECT COUNT(*) FROM {tbl(dataset, table)}) AS n"


@dataclass(frozen=True)
class Check:
    layer: str  # silver or gold
    model: str
    source: str  # what the rows are built from
    grain: str  # the grain the model declares
    key: str  # the key the model deduplicates or joins on
    source_sql: str | None  # (k, label, n); None = derived, rows only
    model_sql: str  # (k, n)
    assertion: str = ""
    klass: str = "exact"  # exact, A-drop, A-key, A-null, B, derived
    cause: str = ""
    expect_diff: bool = False  # a class A or B check with a row difference


@dataclass
class Result:
    check: Check
    source_n: int | None = None
    model_n: int = 0
    off: list[tuple[str, int, int]] = field(default_factory=list)
    error: str = ""

    @property
    def diff(self) -> int | None:
        return None if self.source_n is None else self.model_n - self.source_n

    @property
    def status(self) -> str:
        if self.error:
            return "error"
        if self.source_n is None:
            return "ok"
        if self.check.expect_diff:
            return "ok" if self.diff else "now exact"
        return "ok" if not self.diff else "NEW DIFFERENCE"


# (silver model, bronze table, staging key columns separated by ;  or  []array column)
SOURCES = [
    ("fact_customer_invoice", "visma_customer_invoices", "referenceNumber"),
    ("fact_customer_invoice_line", "visma_customer_invoices", "[]invoiceLines"),
    ("fact_customer_credit_note", "visma_customer_credit_notes", "referenceNumber"),
    ("fact_customer_payment", "visma_customer_payments", "refNbr;type"),
    ("fact_supplier_invoice", "visma_supplier_invoices", "documentType;referenceNumber"),
    ("fact_supplier_invoice_line", "visma_supplier_invoices", "[]invoiceLines"),
    ("fact_supplier_payment", "visma_supplier_payments", "type;refNbr"),
    ("fact_general_ledger", "visma_general_ledger_transactions", "module;batchNumber;lineNumber"),
    ("fact_purchase_order", "visma_purchase_orders", "orderType;orderNbr"),
    ("fact_purchase_order_line", "visma_purchase_orders", "[]lines"),
    ("fact_shipment", "visma_shipments", "shipmentNumber"),
    ("fact_shipment_line", "visma_shipments", "[]shipmentDetailLines"),
    ("fact_cash_transaction", "visma_cash_transactions", "referenceNbr"),
    ("fact_fixed_asset_transaction", "visma_fixed_asset_transactions", "refNo;lineNo"),
    ("fact_project_budget", "visma_project_budgets",
     "projectID;projectTaskID;accountGroupID;inventoryNumber"),
    ("fact_project_transaction", "visma_project_transactions", "module;refNbr"),
    ("fact_sales_order", "visma_sales_orders_v3", "orderType;orderId"),
    ("dim_account", "visma_accounts", "accountCD"),
    ("dim_branch", "visma_branches", "number"),
    ("dim_cash_account", "visma_cash_accounts", "number"),
    ("dim_company", "visma_branches", "number"),
    ("dim_currency", "visma_currencies", "id"),
    ("dim_currency_rate", "visma_currency_rates", "id"),
    ("dim_customer", "visma_customers", "number"),
    ("dim_department", "visma_departments", "departmentId"),
    ("dim_employee", "visma_employees", "employeeId"),
    ("dim_financial_period", "visma_financial_periods", "year;period"),
    ("dim_fixed_asset", "visma_fixed_assets", "assetID"),
    ("dim_inventory_item", "visma_inventory_items", "inventoryNumber"),
    ("dim_ledger", "visma_ledgers", "number"),
    ("dim_location", "visma_locations", "JSON_VALUE(baccount, '$.number');locationId"),
    ("dim_project", "visma_projects", "projectID"),
    ("dim_sales_person", "visma_sales_persons", "salespersonID"),
    ("dim_subaccount", "visma_subaccounts", "subaccountId"),
    ("dim_supplier", "visma_suppliers", "number"),
    ("dim_warehouse", "visma_warehouses", "warehouseID"),
    ("fact_general_ledger_balance", "visma_general_ledger_balances_v2",
     "JSON_VALUE(account, '$.number');subAccountCd;financialPeriod;JSON_VALUE(ledger, '$.id')"),
]  # fmt: skip

# model -> (grain the model declares, key it really uses)
META = {
    "fact_customer_invoice": ("invoice header", "(tenant, ref, type)"),
    "fact_customer_invoice_line": ("invoice line", "(tenant, ref, type, line)"),
    "fact_customer_credit_note": ("per credit note (tenant + ref)", "(tenant, ref)"),
    "fact_customer_payment": ("per payment (tenant + refNbr)", "(tenant, refNbr)"),
    "fact_supplier_invoice": ("per invoice (tenant + ref)", "(tenant, ref)"),
    "fact_supplier_invoice_line": ("per line (tenant + ref + line)", "(tenant, ref, line, item)"),
    "fact_supplier_payment": ("per payment (tenant + refNbr)", "(tenant, refNbr)"),
    "fact_general_ledger": ("GL line", "(tenant, batch, ref, line)"),
    "fact_general_ledger_balance": ("GL balance", "(tenant, account, subaccount, ledger, period)"),
    "fact_purchase_order": ("(tenant + type + number)", "(tenant, type, number)"),
    "fact_purchase_order_line": ("PO line", "(tenant, type, number, line, item)"),
    "fact_shipment": ("per shipment (tenant + number)", "(tenant, number), freshest sync"),
    "fact_shipment_line": ("(tenant + shipment + line)", "(tenant, shipment, line, item)"),
    "fact_cash_transaction": ("cash transaction", "(tenant, ref, type)"),
    "fact_fixed_asset_transaction": ("(tenant + refNo + lineNo)", "(tenant, refNo, lineNo)"),
    "fact_project_budget": ("per project + task + account group", "(tenant, project, task, group)"),
    "fact_project_transaction": ("per document (tenant, module, ref)", "(tenant, module, ref)"),
    "fact_sales_order": ("(tenant, type, id)", "(tenant, type, id)"),
    "dim_account": ("chart of accounts", "(tenant, accountCD)"),
    "dim_branch": ("per tenant + branch", "(tenant, branchId)"),
    "dim_cash_account": ("cash and bank accounts", "(tenant, number)"),
    "dim_company": ("per tenant + branch", "(tenant, branchId)"),
    "dim_currency": ("tenant currency", "(tenant, id)"),
    "dim_currency_rate": ("(tenant, type, date, from, to)", "the same; staging key is id"),
    "dim_customer": ("customer master", "(tenant, number)"),
    "dim_department": ("per (tenant, department)", "(tenant, departmentId)"),
    "dim_employee": ("employee master", "(tenant, employeeId)"),
    "dim_financial_period": ("fiscal period", "(tenant, period); staging (year, period)"),
    "dim_fixed_asset": ("asset register", "(tenant, assetID)"),
    "dim_inventory_item": ("per tenant + inventory", "(tenant, inventoryId); staging: number"),
    "dim_ledger": ("GL ledger", "(tenant, number)"),
    "dim_location": ("location master", "(tenant, locationId); staging adds baccount.number"),
    "dim_project": ("project master", "(tenant, projectID)"),
    "dim_sales_person": ("salesperson master", "(tenant, salespersonID)"),
    "dim_subaccount": ("subaccount segments", "(tenant, subaccountId)"),
    "dim_supplier": ("supplier master", "(tenant, number)"),
    "dim_warehouse": ("warehouse master", "(tenant, warehouseID)"),
}  # fmt: skip
ASSERTIONS = {  # models whose assertion is not assert_<name>_rows_complete
    "fact_shipment_line": "assert_line_item_codes_populated",
    "fact_project_transaction": "assert_project_transaction_documents_complete",
    "fact_sales_order": "assert_sales_order_rows_complete",
    "dim_location": "assert_location_rows_complete",
    "dim_inventory_item": "assert_gold_fact_keys_resolve",
}

# (silver model, gold dataset, gold table, grain)
GOLD_FACTS = [
    ("fact_customer_credit_note", "finance", "fact_customer_credit_note", "credit note"),
    ("fact_customer_invoice", "finance", "fact_customer_invoice", "invoice header"),
    ("fact_customer_invoice_line", "finance", "fact_customer_invoice_line", "invoice line"),
    ("fact_customer_payment", "finance", "fact_customer_payment", "payment"),
    ("fact_fixed_asset_transaction", "finance", "fact_fixed_asset_transaction", "asset txn"),
    ("fact_general_ledger", "finance", "fact_general_ledger", "GL journal line"),
    ("fact_general_ledger", "datamart", "BI_FinanceTable", "per GL line"),
    ("fact_general_ledger_balance", "finance", "fact_general_ledger_balance", "GL balance"),
    ("fact_inventory_movement", "finance", "fact_inventory_movement", "movement header"),
    ("fact_inventory_movement_line", "finance", "fact_inventory_movement_line", "movement line"),
    ("fact_project_transaction_line", "finance", "fact_project_portfolio_line", "project line"),
    ("fact_supplier_invoice", "finance", "fact_supplier_invoice", "invoice header"),
    ("fact_supplier_invoice_line", "finance", "fact_supplier_invoice_line", "invoice line"),
    ("fact_supplier_payment", "finance", "fact_supplier_payment", "payment"),
    ("fact_purchase_order", "sales", "dim_purchase_order", "PO header"),
    ("fact_purchase_order_line", "sales", "fact_purchase_order_line", "PO line"),
    ("fact_sales_order", "sales", "dim_sales_order", "sales order header"),
    ("fact_sales_order_line", "sales", "fact_sales_order_line", "sales order line"),
    ("fact_sales_order_line", "datamart", "BI_SalesOrder", "per sales order line"),
    ("fact_shipment", "sales", "dim_shipment", "shipment header"),
    ("fact_shipment_line", "sales", "fact_shipment_detail_line", "shipment line"),
]  # fmt: skip
GOLD_DIMS = [
    ("dim_account", "finance", "dim_account", "account"),
    ("dim_customer", "finance", "dim_customer", "tenant + customer"),
    ("dim_fixed_asset", "finance", "dim_fixed_asset", "fixed asset"),
    ("dim_supplier", "finance", "dim_supplier", "tenant + business unit + supplier"),
    ("dim_department", "common", "dim_department", "department"),
    ("dim_employee", "common", "dim_employee", "employee"),
    ("dim_financial_period", "common", "dim_financial_period", "period"),
    ("dim_inventory_item", "common", "dim_inventory_item", "tenant + item"),
    ("dim_location", "common", "dim_location", "location"),
    ("dim_project", "common", "dim_project", "project"),
    ("dim_sales_person", "common", "dim_sales_person", "salesperson"),
    ("dim_subaccount", "common", "dim_subaccount", "subaccount"),
    ("dim_warehouse", "common", "dim_warehouse", "warehouse"),
]  # fmt: skip

# (layer, model) -> (class, cause, a row difference is expected). Only the intended differences
# (class B) are listed: the class A defects of the audit of 2026-10-13 were fixed the same day
# (docs/silver_completeness.md), so those models are exact, and a model that stops being exact is
# reported as a new defect.
NOTES = {
    ("silver", "fact_project_transaction"):
        ("B", "one header per document, period slices aggregated", False),
    ("silver", "fact_sales_order_business_unit"):
        ("B", "aggregation of the lines; 38 orders have no lines", False),
    ("silver", "dim_department"):
        ("B", "synthetic Uspesifisert rows for department codes the master lacks", True),
    ("silver", "dim_currency_market_rate"):
        ("B", "source shown as 2 x pairs: direct and inverse", False),
    ("gold", "gold_finance.dim_customer"):
        ("B", "master plus customers that documents reference (+11)", True),
    ("gold", "gold_finance.dim_supplier"):
        ("B", "master plus referenced suppliers, one row per business unit", True),
}  # fmt: skip

MOVEMENT = [  # (type, bronze table, line array column)
    ("ADJUSTMENT", "visma_inventory_adjustments", "adjusmentLines"),
    ("ISSUE", "visma_inventory_issues", "issueLines"),
    ("RECEIPT", "visma_inventory_receipts", "receiptLines"),
    ("TRANSFER", "visma_inventory_transfers", "transferLines"),
]  # fmt: skip
RATES = tbl("dw_staging_currency", "HistoryratesPowerBI")
MARKET = tbl(SILVER, "dim_currency_market_rate")
PROJECT_LINES = (
    "SELECT tenantId AS k, tenantId AS label, COUNT(DISTINCT FORMAT('%t|%t|%t', module, refNbr, "
    f"JSON_VALUE(line, '$.tranId'))) AS n FROM {tbl(BRONZE, 'visma_project_transactions')}, "
    f"UNNEST(JSON_EXTRACT_ARRAY(lines)) AS line {LIVE}"
)
ORDER_LINES = (
    "SELECT tenantId AS k, tenantId AS label, SUM(ARRAY_LENGTH(JSON_EXTRACT_ARRAY(COALESCE("
    "NULLIF(TRIM(orderLines), ''), NULLIF(TRIM(lines), ''), '[]')))) AS n FROM "
    f"{tbl(BRONZE, 'visma_sales_orders_v3')} {LIVE}"
)
BU_SPLIT = (
    "SELECT tenant_id AS k, tenant_id AS label, COUNT(*) AS n FROM (SELECT DISTINCT tenant_id, "
    "order_type, order_id, business_unit_id, business_unit_name FROM "
    f"{tbl(SILVER, 'fact_sales_order_line')}) GROUP BY 1, 2"
)
WAREHOUSE_BINS = (
    "SELECT tenantId AS k, tenantId AS label, COUNT(DISTINCT FORMAT('%t|%t', warehouseID, "
    "JSON_VALUE(loc, '$.locationId'))) AS n FROM "
    f"{tbl(BRONZE, 'visma_warehouses')}, UNNEST(JSON_QUERY_ARRAY(locations)) AS loc {LIVE}"
)
MARKET_PAIRS = single(
    "SELECT 2 * COUNT(DISTINCT FORMAT('%t|%t|%t', currency_code, quote_currency, "
    f"SAFE_CAST(date AS DATE))) FROM {RATES} WHERE currency_code IS NOT NULL AND "
    "quote_currency IS NOT NULL AND rate IS NOT NULL AND date IS NOT NULL"
)
CURRENCIES = single(
    f"SELECT COUNT(DISTINCT c) FROM (SELECT currency_id AS c FROM {tbl(SILVER, 'dim_currency')} "
    f"UNION ALL SELECT from_currency_id FROM {MARKET} "
    f"UNION ALL SELECT to_currency_id FROM {MARKET})"
)
COMBINED = single(
    "SELECT COUNT(DISTINCT customer_combined_key) FROM "
    f"{tbl('visma_silver_common', 'bridge_customer_to_combined')}"
)
NO_GL = (
    "SELECT a.tenant_id AS k, a.tenant_id AS label, COUNT(*) AS n FROM "
    f"{tbl(SILVER, 'dim_account')} AS a WHERE NOT EXISTS (SELECT 1 FROM "
    f"{tbl(SILVER, 'fact_general_ledger')} AS g WHERE g.tenant_id = a.tenant_id AND "
    "g.account_cd = a.account_cd) GROUP BY 1, 2"
)
PROJECTS = (
    "SELECT tenant_id AS k, tenant_id AS label, COUNT(DISTINCT project_id) AS n FROM "
    f"{tbl(GF, 'fact_project_portfolio_line')} GROUP BY 1, 2"
)
RECON = (
    "SELECT tenant_id AS k, tenant_id AS label, COUNT(*) AS n FROM (SELECT tenant_id, project_id "
    f"FROM {tbl(SILVER, 'fact_project_budget')} UNION DISTINCT SELECT tenant_id, project_id FROM "
    f"{tbl(DM, 'BI_ProjectPortfolioPnL')}) GROUP BY 1, 2"
)
BRANCH_TENANTS = (
    "SELECT tenant_id AS k, tenant_id AS label, 1 AS n FROM "
    f"{tbl(SILVER, 'dim_branch')} GROUP BY 1, 2"
)
UNITS = (
    "SELECT tenant_id AS k, tenant_id AS label, COUNT(DISTINCT business_unit_id) AS n FROM "
    f"{tbl(SILVER, 'dim_business_unit')} GROUP BY 1, 2"
)
SPINE = "SELECT DATE_DIFF(DATE '2030-12-31', DATE '2020-01-01', DAY) + 1"
ENTITIES = f"SELECT COUNT(DISTINCT entity_code) FROM {tbl(GC, 'dim_company')}"
MARKET_ROWS = f"SELECT COUNT(*) FROM {MARKET}"
MOVEMENT_ROWS = "assert_inventory_movement_rows_complete"
ITEMS = "assert_line_item_codes_populated"
G_DIM = "assert_gold_dimension_rows_complete"

# (layer, model, source, grain, key, source sql, model sql, assertion): checks with their own SQL
EXPLICIT = [
    ("silver", "fact_inventory_movement", "the four visma_inventory_* tables",
     "(tenant + type + reference)", "(tenant, type, reference)",
     src_typed([(k, t, "referenceNumber") for k, t, _ in MOVEMENT], "COUNT(DISTINCT {col})"),
     rows_by_tenant(SILVER, "fact_inventory_movement", typed=True), MOVEMENT_ROWS),
    ("silver", "fact_inventory_movement_line", "the four line arrays",
     "(tenant + type + reference + line)", "(tenant, type, reference, line, item)",
     src_typed(MOVEMENT, "SUM(ARRAY_LENGTH(JSON_QUERY_ARRAY({col})))"),
     rows_by_tenant(SILVER, "fact_inventory_movement_line", typed=True), ITEMS),
    ("silver", "fact_project_transaction_line", "visma_project_transactions.lines",
     "tenant + module + ref + tran_id", "(tenant, module, ref, period slice, tran)", PROJECT_LINES,
     rows_by_tenant(SILVER, "fact_project_transaction_line"),
     "assert_project_transaction_lines_complete"),
    ("silver", "fact_sales_order_line", "visma_sales_orders_v3.orderLines",
     "(tenant, type, id, line)", "(tenant, type, id, line)", ORDER_LINES,
     rows_by_tenant(SILVER, "fact_sales_order_line"), "assert_sales_order_rows_complete"),
    ("silver", "fact_sales_order_business_unit", "silver.fact_sales_order_line",
     "tenant + order + line unit", "the same", BU_SPLIT,
     rows_by_tenant(SILVER, "fact_sales_order_business_unit"), ""),
    ("silver", "dim_warehouse_location", "visma_warehouses.locations",
     "(tenant, warehouse, location)", "(tenant, warehouse, location)", WAREHOUSE_BINS,
     rows_by_tenant(SILVER, "dim_warehouse_location"), "assert_dimension_rows_complete"),
    ("silver", "dim_business_unit", "silver_excel.business_unit_mapping",
     "historized business unit", "(tenant, unit, validity window)",
     label_rows("visma_silver_excel", "business_unit_mapping"),
     rows_by_tenant(SILVER, "dim_business_unit"), ""),
    ("silver", "dim_currency_market_rate", "dw_staging_currency.HistoryratesPowerBI",
     "pair + date + direction", "(base, quote, date), direct and inverse", MARKET_PAIRS,
     single_model(SILVER, "dim_currency_market_rate"), ""),
    ("gold", "gold_common.dim_company", "silver.dim_branch", "per tenant", "(tenant)",
     BRANCH_TENANTS, rows_by_tenant(GC, "dim_company"), G_DIM),
    ("gold", "gold_common.dim_business_unit", "silver.dim_business_unit",
     "tenant + business unit", "(tenant, unit)", UNITS, rows_by_tenant(GC, "dim_business_unit"),
     G_DIM),
    ("gold", "gold_common.dim_missing_gl_accounts", "silver.dim_account without GL lines",
     "account no GL line uses", "(tenant, account)", NO_GL,
     rows_by_tenant(GC, "dim_missing_gl_accounts"), ""),
    ("gold", "datamart.BI_ProjectPortfolioPnL", "gold_finance.fact_project_portfolio_line",
     "per tenant + project", "(tenant, project)", PROJECTS,
     rows_by_tenant(DM, "BI_ProjectPortfolioPnL"), ""),
    ("gold", "datamart.BI_ProjectBudgetRecon", "fact_project_budget + BI_ProjectPortfolioPnL",
     "per tenant + project", "(tenant, project)", RECON,
     rows_by_tenant(DM, "BI_ProjectBudgetRecon"), ""),
    ("gold", "datamart.RLS_sales_person", "dim_sales_person emails", "user to salesperson",
     "(user, tenant, salesperson)", None, rows_by_tenant(DM, "RLS_sales_person"), ""),
    ("gold", "gold_finance.dim_inventory_item", "gold_common.dim_inventory_item (view)", "view",
     "select *", label_rows(GC, "dim_inventory_item"),
     rows_by_tenant(GF, "dim_inventory_item"), ""),
    ("gold", "gold_common.dim_currency", "silver.dim_currency + rate feed", "currency code",
     "(code)", CURRENCIES, single_model(GC, "dim_currency"), ""),
    ("gold", "gold_common.dim_currency_market_rate", "silver.dim_currency_market_rate",
     "pair + date", "silver key", single(MARKET_ROWS),
     single_model(GC, "dim_currency_market_rate"), ""),
    ("gold", "gold_common.dim_currency_rate", "silver.dim_currency_market_rate", "pair + date",
     "silver key", single(MARKET_ROWS), single_model(GC, "dim_currency_rate"), ""),
    ("gold", "gold_common.dim_date", "generated spine 2020-2030", "per day", "date", single(SPINE),
     single_model(GC, "dim_date"), ""),
    ("gold", "gold_common.dim_customer_combined", "silver_common.bridge_customer_to_combined",
     "combined customer", "(identity)", COMBINED, single_model(GC, "dim_customer_combined"), ""),
    ("gold", "gold_common.dim_entity", "gold_common.dim_company entity codes",
     "reporting entity", "(entity code)", single(ENTITIES), single_model(GC, "dim_entity"), ""),
    ("gold", "gold_common.dim_currency_market_rate_to_nok_daily", "market rates, fill-forward",
     "daily bridge", "(date, currency)", None,
     single_model(GC, "dim_currency_market_rate_to_nok_daily"), ""),
]  # fmt: skip
OTHER_CLASS = {  # EXPLICIT models that are not exact: class and cause
    "gold_common.dim_missing_gl_accounts": ("B", "anti-join of the account master"),
    "datamart.BI_ProjectPortfolioPnL": ("B", "aggregation to one row per project"),
    "datamart.BI_ProjectBudgetRecon": ("B", "outer join; inherits the budget drop"),
    "datamart.RLS_sales_person": ("derived", "mapping built from the salesperson emails"),
    "gold_common.dim_currency": ("B", "global code grain: 15 codes from 122 tenant rows"),
    "gold_common.dim_currency_rate": ("B", "copy of the market-rate table; tenant rates unused"),
    "gold_common.dim_date": ("B", "spine 2020-2030: a date outside it gets no row"),
    "gold_common.dim_customer_combined": ("B", "cross-tenant deduplication of 27,298 customers"),
    "gold_common.dim_entity": ("B", "grouping of legal entities"),
    "gold_common.dim_currency_market_rate_to_nok_daily": ("derived", "daily spine"),
}  # fmt: skip


def make(
    layer: str,
    model: str,
    source: str,
    grain: str,
    key: str,
    src: str | None,
    rows: str,
    assertion: str = "",
) -> Check:
    """A check with the class and cause that NOTES or OTHER_CLASS hold for the model."""
    klass, cause, expect = NOTES.get((layer, model), ("exact", "", False))
    if model in OTHER_CLASS:
        klass, cause = OTHER_CLASS[model]
    return Check(layer, model, source, grain, key, src, rows, assertion, klass, cause, expect)


def assertion_for(model: str) -> str:
    if model in ASSERTIONS:
        return ASSERTIONS[model]
    if model.startswith("dim_"):
        return "assert_dimension_rows_complete"
    name = model.removeprefix("fact_").removesuffix("_line").removesuffix("_balance")
    return f"assert_{name}_rows_complete"


def build_catalogue() -> list[Check]:
    out = []
    for model, table, keys in SOURCES:
        grain, key = META[model]
        if keys.startswith("[]"):
            source, src = f"{table}.{keys[2:]}", src_array(table, keys[2:])
        else:
            source, src = table, src_distinct(table, *keys.split(";"))
        out.append(
            make(
                "silver",
                model,
                source,
                grain,
                key,
                src,
                rows_by_tenant(SILVER, model),
                assertion_for(model),
            )
        )
    for kind in ("adjustment", "issue", "receipt", "transfer"):
        for suffix in ("", "_line"):
            for layer, dataset in (("silver", SILVER), ("gold", GF)):
                unified, name = f"fact_inventory_movement{suffix}", f"fact_inventory_{kind}{suffix}"
                src = label_rows(dataset, unified, f"WHERE movement_type = '{kind.upper()}'")
                model = name if layer == "silver" else f"gold_finance.{name}"
                out.append(
                    make(
                        layer,
                        model,
                        f"{unified} where type = {kind.upper()}",
                        "view",
                        "filter by type",
                        src,
                        rows_by_tenant(dataset, name),
                    )
                )
    for lst, assertion in ((GOLD_FACTS, "assert_gold_fact_rows_complete"), (GOLD_DIMS, G_DIM)):
        for silver_model, dataset, table, grain in lst:
            schema = GOLD[dataset]
            model = f"{schema.removeprefix('visma_')}.{table}"
            if lst is GOLD_FACTS:
                src, rows, key = (
                    by_company(silver_model),
                    gold_by_company(schema, table),
                    "silver key",
                )
            else:
                src, rows, key = (
                    label_rows(SILVER, silver_model),
                    rows_by_tenant(schema, table),
                    "key",
                )
            out.append(
                make("gold", model, f"silver.{silver_model}", grain, key, src, rows, assertion)
            )
    return out + [make(*row) for row in EXPLICIT]


def run_check(client: bigquery.Client, check: Check) -> Result:
    result = Result(check)
    if check.source_sql is None:
        sql = (
            f"WITH m AS ({check.model_sql}) SELECT k AS tenant, 0 AS source_n, n AS model_n FROM m"
        )
    else:
        sql = (
            f"WITH s AS ({check.source_sql}), m AS ({check.model_sql}) SELECT COALESCE(s.label, "
            "s.k, m.k) AS tenant, IFNULL(s.n, 0) AS source_n, IFNULL(m.n, 0) AS model_n FROM s "
            "FULL OUTER JOIN m ON s.k = m.k"
        )
    try:
        rows = list(client.query(sql).result())
    except Exception as error:  # one broken query must not hide the other checks
        result.error = str(error).splitlines()[0][:160]
        return result
    result.model_n = sum(r.model_n for r in rows)
    if check.source_sql is not None:
        result.source_n = sum(r.source_n for r in rows)
        result.off = [(r.tenant, r.source_n, r.model_n) for r in rows if r.source_n != r.model_n]
    return result


# dimension -> (dataset, table, key columns): what a business key of a gold fact must resolve to
FK_DIMS = {
    "customer": (GF, "dim_customer", ["customer_key"]),
    "supplier": (GF, "dim_supplier", ["supplier_key"]),
    "account": (GF, "dim_account", ["account_key"]),
    "inventory_item": (GC, "dim_inventory_item", ["item_key"]),
    "location": (GC, "dim_location", ["location_key", "location_tenant_key"]),
    "warehouse": (GC, "dim_warehouse", ["warehouse_key"]),
    "sales_person": (GC, "dim_sales_person", ["sales_person_key"]),
    "business_unit": (GC, "dim_business_unit", ["business_unit_key"]),
    "project": (GC, "dim_project", ["project_key"]),
    "department": (GC, "dim_department", ["department_key"]),
    "fixed_asset": (GF, "dim_fixed_asset", ["fixed_asset_key"]),
    "purchase_order": (GS, "dim_purchase_order", ["purchase_order_key"]),
    "sales_order": (GS, "dim_sales_order", ["sales_order_key"]),
    "shipment": (GS, "dim_shipment", ["shipment_key"]),
    "company": (GC, "dim_company", ["company_key"]),
    "date": (GC, "dim_date", ["date_key"]),
    "currency": (GC, "dim_currency", ["currency_key"]),
}
# gold dataset, fact, then key column>dimension pairs; a fact can take several lines
FK_FACTS = """
finance fact_customer_credit_note customer_key>customer account_key>account
finance fact_customer_invoice customer_key>customer location_key>location
finance fact_customer_invoice_line account_key>account item_key>inventory_item
finance fact_customer_payment customer_key>customer
finance fact_fixed_asset_transaction fixed_asset_key>fixed_asset
finance fact_general_ledger account_key>account business_unit_key>business_unit
finance fact_general_ledger department_key>department
finance fact_general_ledger_balance account_key>account business_unit_key>business_unit
finance fact_inventory_movement source_warehouse_key>warehouse destination_warehouse_key>warehouse
finance fact_inventory_movement_line item_key>inventory_item
finance fact_inventory_movement_line source_warehouse_key>warehouse
finance fact_inventory_movement_line destination_warehouse_key>warehouse
finance fact_project_portfolio_line project_key>project account_key>account
finance fact_project_portfolio_line business_unit_key>business_unit department_key>department
finance fact_supplier_invoice supplier_key>supplier location_key>location
finance fact_supplier_invoice_line supplier_key>supplier account_key>account item_key>inventory_item
finance fact_supplier_payment supplier_key>supplier
sales dim_sales_order customer_key>customer warehouse_key>warehouse sales_person_key>sales_person
sales dim_shipment from_warehouse_key>warehouse to_warehouse_key>warehouse
sales fact_purchase_order_line supplier_key>supplier item_key>inventory_item
sales fact_purchase_order_line warehouse_key>warehouse purchase_order_key>purchase_order
sales fact_sales_order_line customer_key>customer account_key>account item_key>inventory_item
sales fact_sales_order_line warehouse_key>warehouse sales_person_key>sales_person
sales fact_sales_order_line business_unit_key>business_unit sales_order_key>sales_order
sales fact_shipment_detail_line item_key>inventory_item location_key>location
sales fact_shipment_detail_line warehouse_key>warehouse shipment_key>shipment
"""
FK_KNOWN: dict[str, str] = {}  # every business key of the gold facts resolves since 2026-10-13


def run_fk(client: bigquery.Client) -> list[dict]:
    pairs = []
    for line in FK_FACTS.strip().splitlines():
        dataset, fact, *keys = line.split()
        pairs += [(GOLD[dataset], fact, *key.split(">")) for key in keys]
    for dataset in (GF, GS, GC, DM):  # company, date and currency keys of every table that has them
        cols = "'company_key', 'date_key', 'currency_key'"
        sql = (
            f"SELECT table_name, column_name FROM `{PROJECT}.{dataset}.INFORMATION_SCHEMA.COLUMNS` "
            f"WHERE column_name IN ({cols})"
        )
        for r in client.query(sql).result():
            dimension = r.column_name.removesuffix("_key")
            if r.table_name != FK_DIMS[dimension][1] and not r.table_name.startswith("assert_"):
                pairs.append((dataset, r.table_name, r.column_name, dimension))

    def one(pair: tuple[str, str, str, str]) -> dict:
        dataset, fact, key, dimension = pair
        dim_dataset, dim_table, dim_keys = FK_DIMS[dimension]
        union = " UNION DISTINCT ".join(
            f"SELECT CAST({k} AS STRING) AS k FROM {tbl(dim_dataset, dim_table)}" for k in dim_keys
        )
        sql = (
            f"WITH d AS ({union}) SELECT COUNTIF(f.{key} IS NOT NULL) AS keyed, "
            f"COUNTIF(f.{key} IS NOT NULL AND d.k IS NULL) AS unresolved FROM "
            f"{tbl(dataset, fact)} AS f LEFT JOIN d ON CAST(f.{key} AS STRING) = d.k"
        )
        row = next(iter(client.query(sql).result()))
        known = FK_KNOWN.get(key, "") if row.unresolved else ""
        return {
            "fact": fact,
            "key": key,
            "dimension": dimension,
            "keyed": row.keyed,
            "unresolved": row.unresolved,
            "known": known,
        }

    with ThreadPoolExecutor(max_workers=WORKERS) as pool:
        return list(pool.map(one, pairs))


def run_staging(client: bigquery.Client) -> dict:
    """Staging against bronze row counts, and the deleted rows of the bronze tables silver reads."""

    def counts(dataset: str) -> dict[str, int]:
        sql = f"SELECT table_id, row_count FROM `{PROJECT}.{dataset}.__TABLES__`"
        return {r.table_id: r.row_count for r in client.query(sql).result()}

    staging, bronze = counts(STAGING), counts(BRONZE)
    configs = [
        json.loads(p.read_text()) for p in sorted((ROOT / "config/endpoints").glob("*.json"))
    ]
    configs = [e for e in configs if e.get("bigquery_table") in bronze]
    differ = [
        (e["bigquery_table"], staging.get(e["endpoint_name"]), bronze[e["bigquery_table"]])
        for e in configs
        if staging.get(e["endpoint_name"]) != bronze[e["bigquery_table"]]
    ]
    text = " ".join(c.source_sql or "" for c in build_catalogue())
    read = sorted(set(re.findall(r"visma_bronze\.(visma_\w+)`", text)))
    sql = " UNION ALL ".join(
        f"SELECT '{t}' AS t, COUNTIF(isDeleted) AS dead FROM {tbl(BRONZE, t)}" for t in read
    )
    dead = {r.t: r.dead for r in client.query(sql).result() if r.dead}
    return {"endpoints": len(configs), "differ": differ, "read": len(read), "deleted": dead}


def fmt(n: int | None) -> str:
    return "-" if n is None else f"{n:,}"


def show(rows: list[tuple[str, ...]], numeric: set[int]) -> None:
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    for n, row in enumerate(rows):
        cells = [
            c.rjust(widths[i]) if i in numeric else c.ljust(widths[i]) for i, c in enumerate(row)
        ]
        print("  ".join(cells).rstrip())
        if n == 0:
            print("  ".join("-" * w for w in widths))


def print_results(results: list[Result], tenants: bool) -> None:
    rows = [("layer", "model", "source keys", "model rows", "diff", "tenants", "class", "status")]
    for r in results:
        diff = "-" if r.diff is None else (f"{r.diff:+,}" if r.diff else "0")
        off = "-" if r.source_n is None else str(len(r.off))
        rows.append(
            (
                r.check.layer,
                r.check.model,
                fmt(r.source_n),
                fmt(r.model_n),
                diff,
                off,
                r.check.klass,
                r.error or r.status,
            )
        )
    show(rows, {2, 3, 4, 5})
    counts: dict[str, int] = {}
    for r in results:
        counts[r.check.klass] = counts.get(r.check.klass, 0) + 1
    summary = ", ".join(f"{k} {v}" for k, v in sorted(counts.items()))
    print(f"\n{len(results)} models compared: {summary}")
    for r in results:
        if r.check.klass not in ("exact", "derived"):
            print(f"[{r.check.klass}] {r.check.layer} {r.check.model}: {r.check.cause}")
    for r in results:
        if tenants and r.off:
            print(f"\n{r.check.layer} {r.check.model}: tenants that differ (source, model)")
            for label, s, m in sorted(r.off):
                print(f"  {label[:8]:8s} {s:>10,} {m:>10,} {m - s:>+9,}")


def print_fk(rows: list[dict]) -> None:
    print("\nBusiness keys of the gold facts against their dimensions, the unresolved ones:")
    out = [("fact", "key", "dimension", "keyed rows", "unresolved", "%", "known")]
    for r in rows:
        if r["unresolved"]:
            pct = 100.0 * r["unresolved"] / r["keyed"]
            out.append(
                (
                    r["fact"],
                    r["key"],
                    r["dimension"],
                    fmt(r["keyed"]),
                    fmt(r["unresolved"]),
                    f"{pct:.2f}",
                    r["known"],
                )
            )
    show(out, {3, 4, 5})
    print(f"{len(rows)} key pairs checked, {len(out) - 1} unresolved")


def print_staging(info: dict) -> None:
    print(
        f"\nStaging against bronze: {info['endpoints']} bronze tables with a staging endpoint, "
        f"{len(info['differ'])} with different row counts"
    )
    for name, staged, bronze in info["differ"]:
        print(f"  {name}: staging {staged}, bronze {bronze}")
    total = sum(info["deleted"].values())
    print(
        f"Deleted source rows in the {info['read']} bronze tables silver reads: {total} (class C)"
    )
    for name, dead in info["deleted"].items():
        print(f"  {name}: {dead}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--layer", choices=["silver", "gold", "keys", "staging"], help="only this part")
    ap.add_argument("--only", default="", help="only the models whose name contains this text")
    ap.add_argument("--tenants", action="store_true", help="list the tenants that differ")
    args = ap.parse_args()

    client = bigquery.Client(project=PROJECT, location=LOCATION)
    checks = [
        c for c in build_catalogue() if args.layer in (None, c.layer) and args.only in c.model
    ]
    checks.sort(key=lambda c: (c.layer != "silver", c.model))
    with ThreadPoolExecutor(max_workers=WORKERS) as pool:
        results = list(pool.map(lambda check: run_check(client, check), checks))
    if results:
        print_results(results, args.tenants)
    if args.layer in (None, "keys"):
        print_fk(run_fk(client))
    if args.layer in (None, "staging"):
        print_staging(run_staging(client))
    changed = [r for r in results if r.status != "ok"]
    for r in changed:
        print(f"CHANGED: {r.check.layer} {r.check.model}: {r.error or r.status}", file=sys.stderr)
    return 1 if changed else 0


if __name__ == "__main__":
    sys.exit(main())
