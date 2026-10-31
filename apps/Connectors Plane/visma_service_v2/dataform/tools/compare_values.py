#!/usr/bin/env python3
"""Compare the totals of the money and quantity columns of the fact tables with v1.

Row counts and fill rates do not show a column that holds the wrong number. For each table this
sums every numeric measure column (names with amount, total, qty, cost, price, balance, debit,
credit, tax, vat, ...) over the ACTIVE tenants and compares with v1. Where the table has a
last-modified date set on every row on both sides, only the rows last modified before --cutoff are
summed, on both sides (a like-for-like cutoff, see parity_findings.py). Differences inside
--tolerance percent are expected for the tables that have no such date: v1 keeps syncing while the
v2 staging data is a snapshot. Writes docs/dataform_v1_values.md. Read-only.

Usage: dataform/tools/compare_values.py [--tolerance 2] [--tables a,b] [--cutoff 2026-10-01]
                                        [--out DIR] [--json PATH]
--out writes the report into DIR instead of docs/. --json writes one finding per column outside
the tolerance, and one per table that could not be compared (see parity_findings.py).
"""

from __future__ import annotations

import argparse
import re
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from google.cloud import bigquery
from parity_findings import (
    HISTORICAL_TENANT_CONDITION,
    LOCATION,
    add_cutoff_option,
    add_output_options,
    before_cutoff,
    columns_by_table,
    cutoff_applies,
    finding,
    modified_column,
    report_path,
    scope_counts,
    where_sql,
    write_findings,
)

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "docs" / "dataform_v1_values.md"
NEW, OLD = "aquatiq-datavarehus", "prj-dw-dev"
# (new dataset, v1 dataset, table)
TABLES = [
    ("visma_silver", "dw_1_silver_visma", t)
    for t in (
        "fact_general_ledger",
        "fact_general_ledger_balance",
        "fact_sales_order",
        "fact_sales_order_line",
        "fact_customer_invoice",
        "fact_customer_invoice_line",
        "fact_customer_payment",
        "fact_supplier_invoice",
        "fact_supplier_invoice_line",
        "fact_supplier_payment",
        "fact_purchase_order",
        "fact_purchase_order_line",
        "fact_inventory_movement_line",
        "fact_shipment",
        "fact_project_transaction",
        "fact_project_budget",
        "fact_cash_transaction",
    )
] + [
    ("visma_datamart", "dw_1_gold_bi", t)
    for t in ("BI_FinanceTable", "BI_SalesOrder", "BI_ProjectPortfolioPnL")
]
MEASURE = re.compile(
    r"amount|total|qty|quantity|cost|price|balance|debit|credit|tax|vat|discount|freight|margin|profit",
    re.I,
)
NOT_MEASURE = re.compile(
    r"(_id|_key|_no|_number|_nbr|_code|_cd|_date|_at|_period|_line_nr|_flag|_count)$", re.I
)
NUMERIC = ("INT64", "FLOAT64", "NUMERIC", "BIGNUMERIC")


def measures(cols: dict[str, str]) -> dict[str, str]:
    return {
        name: kind
        for name, kind in cols.items()
        if kind in NUMERIC and MEASURE.search(name) and not NOT_MEASURE.search(name)
    }


def sums(client, project, dataset, table, names: list[str], where: list[str]) -> dict | None:
    parts = ", ".join(f"SUM(CAST(`{n}` AS FLOAT64)) AS `s{i}`" for i, n in enumerate(names))
    select = "COUNT(*) AS n" + (f", {parts}" if parts else "")
    sql = f"SELECT {select} FROM `{project}.{dataset}.{table}`{where_sql(where)}"
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception:
        return None
    return {"_n": row["n"], **{n: row[f"s{i}"] for i, n in enumerate(names)}}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tolerance", type=float, default=2.0, help="percent")
    ap.add_argument("--tables", default="")
    add_cutoff_option(ap)
    add_output_options(ap)
    args = ap.parse_args()
    cutoff = args.cutoff
    only = {t for t in args.tables.split(",") if t}
    client = bigquery.Client(project=NEW)
    specs = [s for s in TABLES if not only or s[2] in only]
    cols_new = {ds: columns_by_table(client, NEW, ds) for ds in {s[0] for s in specs}}
    cols_old = {ds: columns_by_table(client, OLD, ds) for ds in {s[1] for s in specs}}

    def one(spec):
        new_ds, old_ds, table = spec
        a = cols_new[new_ds].get(table, {})
        b = cols_old[old_ds].get(table, {})
        a_m, b_m = measures(a), measures(b)
        names = [c for c in a_m if c in b_m]
        tenant = HISTORICAL_TENANT_CONDITION if "tenant_id" in a and "tenant_id" in b else None
        modified = modified_column(a, b) if cutoff else None
        st_new = scope_counts(client, NEW, new_ds, table, modified, cutoff, tenant)
        st_old = scope_counts(client, OLD, old_ds, table, modified, cutoff, tenant)
        usable = cutoff_applies(modified, cutoff, st_new, st_old)
        where = [tenant] + ([before_cutoff(modified, cutoff)] if usable else [])
        where = [w for w in where if w]
        scope = f"rows last modified before {cutoff} on both sides" if usable else "all rows"
        if modified and not usable:
            scope += f" ({modified} empty on a side)"
        return (
            spec,
            scope,
            names,
            sums(client, NEW, new_ds, table, names, where),
            sums(client, OLD, old_ds, table, names, where),
        )

    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(one, specs))

    lines = [
        "# Dataform: totals against v1",
        "",
        "> Generated by `dataform/tools/compare_values.py`. Sum of every measure column of the fact",
        "> tables over the active tenants, here and in v1. Where a table has a last-modified date on every",
        f"> row on both sides, only rows last modified before {cutoff or '(no cutoff)'} are summed. v1 keeps",
        f"> syncing and the v2 staging data is a snapshot, so up to {args.tolerance:g} % is expected elsewhere.",
        "> Listed: columns outside that.",
        "",
    ]
    bad = 0
    findings = []
    for (new_ds, _, table), scope, names, a, b in results:
        if not a or not b:
            lines.append(f"* `{new_ds}.{table}`: not comparable")
            findings.append(
                finding(
                    "compare_values", new_ds, table, "not_comparable", None, None, "query failed"
                )
            )
            continue
        off = []
        for n in names:
            x, y = a.get(n) or 0.0, b.get(n) or 0.0
            if abs(x) < 1e-9 and abs(y) < 1e-9:
                continue
            rel = abs(x - y) / max(abs(y), 1e-9) * 100
            if rel > args.tolerance:
                off.append((n, x, y, rel))
                findings.append(finding("compare_values", new_ds, table, f"sum:{n}", y, x))
        lines.append(
            f"## {new_ds}.{table}  (rows {a['_n']:,} / v1 {b['_n']:,}; {len(names)} measure columns; {scope})"
        )
        if not off:
            lines.append("* all within tolerance")
        for n, x, y, rel in off:
            lines.append(f"* `{n}`: {x:,.2f} against v1 {y:,.2f} ({rel:.1f} % off)")
        bad += len(off)
        lines.append("")
    lines.append(f"{len(results)} tables, {bad} columns outside {args.tolerance:g} %.")
    report = report_path(args.out, OUT)
    report.write_text("\n".join(lines) + "\n")
    print(f"{len(results)} tables, {bad} columns outside tolerance -> {report}")
    write_findings(args.json, findings)
    if args.json:
        print(f"{len(findings)} findings -> {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
