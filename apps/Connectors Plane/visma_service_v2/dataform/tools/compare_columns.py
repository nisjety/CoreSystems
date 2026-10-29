#!/usr/bin/env python3
"""Compare how full every column is, in this project and in v1, for every silver, gold and
datamart table.

Row counts do not show a column that came out empty because a bronze field was not carried over.
For each table and each column both sides have, this counts the share of rows with a value and
lists the columns where it dropped by more than --drop percentage points. Only ACTIVE tenants are
counted when the table has a tenant_id (the historical tenants stopped syncing in v1 in 2025, so
v1 has almost nothing for them). Where the table has a last-modified date set on every row on both
sides, only the rows last modified before --cutoff are counted, on both sides (see
parity_findings.py). Two tables that are both empty are not a difference. Writes
docs/dataform_v1_columns.md. Read-only.

Usage: dataform/tools/compare_columns.py [--drop 5] [--only name,name] [--cutoff 2026-10-01]
                                        [--out DIR] [--json PATH]
--out writes the report into DIR instead of docs/. --json writes findings (see parity_findings.py):
the flagged fill rates, every column only one side has (the report lists only the v1 ones), and
tables that could not be compared.
"""

from __future__ import annotations

import argparse
import json
import subprocess
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
OUT = ROOT.parent / "docs" / "dataform_v1_columns.md"
NEW, OLD = "aquatiq-datavarehus", "prj-dw-dev"
SCHEMAS = {
    "visma_silver": "dw_1_silver_visma",
    "visma_silver_common": "dw_1_silver_common",
    "visma_silver_excel": "dw_1_silver_excel",
    "visma_gold_common": "dw_1_gold_common",
    "visma_gold_finance": "dw_1_gold_finance",
    "visma_gold_sales": "dw_1_gold_sales",
    "visma_datamart": "dw_1_gold_bi",
}
SKIP_TYPES = ("ARRAY", "STRUCT", "JSON", "GEOGRAPHY")


def compiled() -> list[dict]:
    out = subprocess.run(
        ["dataform", "compile", "--timeout=180s", "--json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return json.loads(out)["tables"]


def fill(client, project, dataset, table, names: list[str], where: list[str]) -> dict | None:
    """Rows in scope, and for each column how many of them have a value."""
    parts = ", ".join(f"COUNTIF(`{n}` IS NOT NULL) AS `c{i}`" for i, n in enumerate(names))
    select = "COUNT(*) AS n" + (f", {parts}" if parts else "")
    sql = f"SELECT {select} FROM `{project}.{dataset}.{table}`{where_sql(where)}"
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception:
        return None
    return {"_rows": row["n"], **{n: row[f"c{i}"] for i, n in enumerate(names)}}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument(
        "--drop", type=float, default=5.0, help="flag a drop of more than this many points"
    )
    ap.add_argument("--only", default="", help="comma-separated table names")
    add_cutoff_option(ap)
    add_output_options(ap)
    args = ap.parse_args()
    cutoff = args.cutoff
    only = {n for n in args.only.split(",") if n}
    client = bigquery.Client(project=NEW)
    actions = [
        t
        for t in compiled()
        if t["target"]["schema"] in SCHEMAS and (not only or t["target"]["name"] in only)
    ]
    cols_new = {s: columns_by_table(client, NEW, s) for s in SCHEMAS}
    cols_old = {SCHEMAS[s]: columns_by_table(client, OLD, SCHEMAS[s]) for s in SCHEMAS}

    def one(t: dict):
        schema, name = t["target"]["schema"], t["target"]["name"]
        old_ds = SCHEMAS[schema]
        a = cols_new[schema].get(name, {})
        b = cols_old[old_ds].get(name, {})
        common = [
            c
            for c in a
            if c in b and not a[c].startswith(SKIP_TYPES) and not b[c].startswith(SKIP_TYPES)
        ]
        tenant = HISTORICAL_TENANT_CONDITION if "tenant_id" in a and "tenant_id" in b else None
        modified = modified_column(a, b) if cutoff else None
        st_new = scope_counts(client, NEW, schema, name, modified, cutoff, tenant)
        st_old = scope_counts(client, OLD, old_ds, name, modified, cutoff, tenant)
        usable = cutoff_applies(modified, cutoff, st_new, st_old)
        where = [w for w in [tenant, before_cutoff(modified, cutoff) if usable else None] if w]
        scope = f"before {cutoff}" if usable else "all rows"
        if modified and not usable:
            scope += f", {modified} empty on a side"
        if not common:
            fills = (None, None)
        else:
            fills = (
                fill(client, NEW, schema, name, common, where),
                fill(client, OLD, old_ds, name, common, where),
            )
        return (
            schema,
            name,
            common,
            fills[0],
            fills[1],
            sorted(set(b) - set(a)),
            sorted(set(a) - set(b)),
            scope,
        )

    with ThreadPoolExecutor(max_workers=8) as pool:
        results = sorted(pool.map(one, actions), key=lambda r: (r[0], r[1]))

    cut_text = (
        f"rows last modified before {cutoff} where that date is set on every row on both sides, all rows otherwise"
        if cutoff
        else "all rows (cutoff turned off)"
    )
    lines = [
        "# Dataform: column fill against v1",
        "",
        "> Generated by `dataform/tools/compare_columns.py`. For every column both sides have: the share",
        f"> of rows with a value, here and in v1. Scope: active tenants only where the table has `tenant_id`; {cut_text}.",
        f"> Listed: columns that are at least {args.drop:g} points emptier here than in v1.",
        "",
    ]
    total_flagged = 0
    empty_both = 0
    skipped = 0
    findings = []
    for schema, name, common, a, b, only_old, only_new, scope in results:
        for col in only_old:
            findings.append(
                finding(
                    "compare_columns",
                    schema,
                    name,
                    f"column_presence:{col}",
                    None,
                    None,
                    "only in v1",
                )
            )
        for col in only_new:
            findings.append(
                finding(
                    "compare_columns",
                    schema,
                    name,
                    f"column_presence:{col}",
                    None,
                    None,
                    "only in v2",
                )
            )
        if not common:
            skipped += 1
            continue
        if a and b and a["_rows"] == 0 and b["_rows"] == 0:
            empty_both += 1
            if only_old:
                lines += ["", f"## {schema}.{name}  (empty on both sides)"]
                lines.append(f"* columns only v1 has: {', '.join(f'`{c}`' for c in only_old)}")
            continue
        if not a or not b or not a["_rows"] or not b["_rows"]:
            rows_note = f"rows new={a and a['_rows']}, v1={b and b['_rows']}"
            lines.append(f"* `{schema}.{name}`: not comparable ({rows_note})")
            findings.append(
                finding("compare_columns", schema, name, "not_comparable", None, None, rows_note)
            )
            continue
        flagged = []
        for col in common:
            pn, po = 100 * a[col] / a["_rows"], 100 * b[col] / b["_rows"]
            if po - pn > args.drop:
                flagged.append((col, pn, po))
                findings.append(
                    finding("compare_columns", schema, name, f"column_fill:{col}", po, pn)
                )
        if flagged or only_old:
            lines += ["", f"## {schema}.{name}  (rows {a['_rows']:,} / v1 {b['_rows']:,}; {scope})"]
            for col, pn, po in flagged:
                lines.append(f"* `{col}`: {pn:.1f} % filled, v1 {po:.1f} %")
            if only_old:
                lines.append(f"* columns only v1 has: {', '.join(f'`{c}`' for c in only_old)}")
            total_flagged += len(flagged)
    lines += [
        "",
        f"{len(results)} tables compared, {total_flagged} columns flagged, {empty_both} empty on both "
        f"sides, {skipped} with no column in common.",
        "",
    ]
    report = report_path(args.out, OUT)
    report.write_text("\n".join(lines))
    print(f"{len(results)} tables, {total_flagged} flagged columns -> {report}")
    write_findings(args.json, findings)
    if args.json:
        print(f"{len(findings)} findings -> {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
