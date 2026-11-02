#!/usr/bin/env python3
"""Compare the bronze tables with v1's, record by record and column by column.

For each bronze table this takes the records both versions have (same tenant and primary key, not
deleted, and, where the table has a last-modified date set on every row on both sides, last modified
before --cutoff on both sides; see parity_findings.py) and reports, per column, the share of those
records where the two values are equal. Each side is deduplicated to one record per tenant and key
(v1 holds duplicate rows), so a duplicate cannot count twice. Numbers match within a tiny tolerance.
A JSON object or array is compared canonically (keys sorted at every level, numbers by value, so 0.0
equals 0 and key order does not matter); see JSON_CANON_UDF. A column that is mostly equal but not
fully shows real differences; a column near 0 shows a wrong mapping.
Writes docs/dataform_v1_bronze_rows.md. Read-only.

Usage: dataform/tools/compare_bronze_rows.py [--cutoff 2026-10-01] [--only a,b] [--min 99.5]
                                            [--out DIR] [--json PATH]
--out writes the report into DIR instead of docs/. --json writes findings (see parity_findings.py).
For a bronze_field finding, v1 is the number of records both sides hold and v2 the number of those
where the field is equal, so diff_pct is minus the share that differs. Tables that were not
compared, and tables with no record in common, are findings too.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from google.cloud import bigquery
from parity_findings import (
    HISTORICAL_SQL,
    LOCATION,
    add_cutoff_option,
    add_output_options,
    before_cutoff,
    columns_by_table,
    cutoff_applies,
    finding,
    key_expr,
    key_sql,
    keys_present,
    modified_column,
    primary_key,
    report_path,
    unset,
    where_sql,
    write_findings,
)

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "docs" / "dataform_v1_bronze_rows.md"
NEW, OLD = "aquatiq-datavarehus", "prj-dw-dev"
SKIP = {
    "syncId", "syncedAt", "firstSyncedAt", "refreshedAt", "seenInFullScopeRuns",
    "consecutiveMissingFullRuns", "isDeleted", "deletedAt", "syncBatch", "metadata",
}  # fmt: skip
COMPARED_TYPES = ("STRING", "INT64", "FLOAT64", "BOOL", "NUMERIC")
# Canonical JSON: a JSON object or array is parsed, its keys sorted at every level, and re-written, so the
# same content in another key order is equal, and numbers compare by value (0.0 and 0 are equal). A string
# that is not a JSON object or array is returned unchanged, so plain codes such as '2.0' stay distinct from '2'.
JSON_CANON_UDF = r"""
CREATE TEMP FUNCTION canon_json(s STRING) RETURNS STRING LANGUAGE js AS r'''
if (s === null) return null;
var t = String(s).trim();
if (t.charAt(0) !== '{' && t.charAt(0) !== '[') return s;
function sortKeys(v) {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (v !== null && typeof v === 'object') {
    var out = {};
    Object.keys(v).sort().forEach(function (k) { out[k] = sortKeys(v[k]); });
    return out;
  }
  return v;
}
try { return JSON.stringify(sortKeys(JSON.parse(t))); } catch (e) { return s; }
''';
"""


def equal(column: str, kind: str) -> str:
    n, o = f"n.`{column}`", f"o.`{column}`"
    if kind in ("FLOAT64", "NUMERIC"):
        return (
            f"(({n} IS NULL AND {o} IS NULL) OR ABS({n} - {o}) <= 0.000001 * GREATEST(1, ABS({o})))"
        )
    if kind == "STRING":
        # Text that parses as a timestamp is compared by instant: v1 and v2 write the same time as
        # '...943Z' and '...943000+00:00'. Other text is compared as text, JSON canonically.
        return (
            f"(canon_json({n}) IS NOT DISTINCT FROM canon_json({o}) "
            f"OR TRIM({n}) IS NOT DISTINCT FROM TRIM({o}) "
            f"OR (SAFE_CAST({n} AS TIMESTAMP) IS NOT NULL AND SAFE_CAST({n} AS TIMESTAMP) = SAFE_CAST({o} AS TIMESTAMP)))"
        )
    return f"({n} IS NOT DISTINCT FROM {o})"


def population(
    client,
    project: str,
    dataset: str,
    table: str,
    key: str,
    modified: str | None,
    cutoff: str,
    conditions: list[str],
) -> dict | None:
    """Records of one side in the population (conditions only): all, with no date, before the cutoff."""
    parts = ["COUNT(*) AS n", f"COUNT(DISTINCT {key}) AS nd"]
    if modified:
        parts += [
            f"COUNTIF({unset(modified)}) AS unset",
            f"COUNTIF({before_cutoff(modified, cutoff)}) AS before",
            f"COUNT(DISTINCT IF({before_cutoff(modified, cutoff)}, {key}, NULL)) AS nd_before",
        ]
    sql = f"SELECT {', '.join(parts)} FROM `{project}.{dataset}.{table}`{where_sql(conditions)}"
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception:
        return None
    return dict(row.items())


def deduplicated(
    project: str, dataset: str, table: str, keys: list[str], cols: dict, conditions: list[str]
) -> str:
    """One record per tenant and key: the latest syncedAt where the side has it."""
    partition = ", ".join(["tenantId"] + [key_expr(k) for k in keys])
    order = "`syncedAt` DESC" if "syncedAt" in cols else "tenantId"
    return (
        f"SELECT * FROM `{project}.{dataset}.{table}`{where_sql(conditions)} "
        f"QUALIFY ROW_NUMBER() OVER (PARTITION BY {partition} ORDER BY {order}) = 1"
    )


def one(client, table: str, cutoff: str, cols_new: dict, cols_old: dict) -> tuple:
    a = cols_new.get(table, {})
    b = cols_old.get(table, {})
    keys = primary_key(table)
    if not keys:
        return table, None, "no primary key in config/endpoints", None
    if not keys_present(keys, a, b):
        return table, None, "key column missing on one side", None
    common = {
        c: t for c, t in a.items() if c in b and c not in SKIP and t == b[c] and t in COMPARED_TYPES
    }
    modified = modified_column(a, b) if cutoff else None
    key = key_sql(keys)
    active = f"tenantId NOT IN ({HISTORICAL_SQL})"
    new_conditions = [active, "NOT isDeleted" if "isDeleted" in a else None]
    old_conditions = [active, "isDeleted IS NOT TRUE" if "isDeleted" in b else None]
    new_conditions = [c for c in new_conditions if c]
    old_conditions = [c for c in old_conditions if c]
    st_new = population(client, NEW, "visma_bronze", table, key, modified, cutoff, new_conditions)
    st_old = population(
        client, OLD, "dw_1_bronze_visma", table, key, modified, cutoff, old_conditions
    )
    if st_new is None or st_old is None:
        return table, None, "population query failed", None
    usable = cutoff_applies(modified, cutoff, st_new, st_old)
    scope = f"last modified before {cutoff} on both sides" if usable else "all rows"
    if modified and not usable:
        scope += f" ({modified} empty on a side)"
    info = {
        "scope": scope,
        "new_rows": st_new["before"] if usable else st_new["n"],
        "new_distinct": st_new["nd_before"] if usable else st_new["nd"],
        "v1_rows": st_old["before"] if usable else st_old["n"],
        "v1_distinct": st_old["nd_before"] if usable else st_old["nd"],
    }
    before = [before_cutoff(modified, cutoff)] if usable else []
    n_sql = deduplicated(NEW, "visma_bronze", table, keys, a, new_conditions + before)
    o_sql = deduplicated(OLD, "dw_1_bronze_visma", table, keys, b, old_conditions + before)
    checks = [f"COUNTIF({equal(c, t)}) AS `e{i}`" for i, (c, t) in enumerate(common.items())]
    on = " AND ".join(f"{key_expr(k, 'n')} = {key_expr(k, 'o')}" for k in ["tenantId", *keys])
    select = ", ".join(["COUNT(*) AS matched", *checks])
    sql = (
        f"{JSON_CANON_UDF}WITH n AS ({n_sql}), o AS ({o_sql}) SELECT {select} FROM n JOIN o ON {on}"
    )
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception as error:
        return table, None, f"query failed: {str(error)[:120]}", info
    eq = {c: row[f"e{i}"] for i, c in enumerate(common)}
    return table, (row["matched"], eq), None, info


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--only", default="")
    ap.add_argument(
        "--min", type=float, default=99.5, help="list columns equal in less than this percent"
    )
    add_cutoff_option(ap)
    add_output_options(ap)
    args = ap.parse_args()
    cutoff = args.cutoff
    client = bigquery.Client(project=NEW)
    cols_new = columns_by_table(client, NEW, "visma_bronze")
    cols_old = columns_by_table(client, OLD, "dw_1_bronze_visma")
    tables = sorted(
        t.table_id
        for t in client.list_tables(f"{NEW}.visma_bronze")
        if t.table_id not in ("visma_tenants", "visma_price_classes")
    )
    only = {t for t in args.only.split(",") if t}
    if only:
        tables = [t for t in tables if t in only]
    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(lambda t: one(client, t, cutoff, cols_new, cols_old), tables))

    cut_text = (
        f"unchanged in v1 since {cutoff} where the table has the date on every row on both sides, all rows otherwise"
        if cutoff
        else "all rows (cutoff turned off)"
    )
    lines = [
        "# Dataform bronze against v1, record by record",
        "",
        "> Generated by `dataform/tools/compare_bronze_rows.py`. Records both versions have (same",
        f"> tenant and key, active tenants, {cut_text}), one record per key on each side. Per column,",
        f"> the share where the values are equal. Listed: columns below {args.min:g} %.",
        "",
    ]
    flagged = 0
    findings = []
    field_note = "v1 = records both sides hold; v2 = of those, records where this field is equal"
    for table, data, why, info in results:
        if data is None:
            lines.append(f"* `{table}`: not compared ({why})")
            findings.append(
                finding(
                    "compare_bronze_rows", "visma_bronze", table, "not_compared", None, None, why
                )
            )
            continue
        matched, eq = data
        bad = [
            (c, 100 * v / matched, v)
            for c, v in eq.items()
            if matched and 100 * v / matched < args.min
        ]
        lines.append(f"## {table}  ({matched:,} records in both)")
        lines.append(
            f"* scope: {info['scope']}. Rows / distinct keys: new {info['new_rows']:,} / "
            f"{info['new_distinct']:,}; v1 {info['v1_rows']:,} / {info['v1_distinct']:,}."
        )
        if not matched:
            lines.append("* no common records")
            findings.append(
                finding(
                    "compare_bronze_rows",
                    "visma_bronze",
                    table,
                    "records_in_both",
                    0,
                    None,
                    "no record in common, so no field was checked",
                )
            )
        elif not bad:
            lines.append(f"* all {len(eq)} columns equal")
        for c, pct, v in sorted(bad, key=lambda x: x[1]):
            lines.append(f"* `{c}`: {pct:.1f} % equal")
            findings.append(
                finding(
                    "compare_bronze_rows",
                    "visma_bronze",
                    table,
                    f"bronze_field:{c}",
                    matched,
                    v,
                    field_note,
                )
            )
        flagged += len(bad)
        lines.append("")
    lines.append(f"{len(results)} tables, {flagged} columns below {args.min:g} %.")
    report = report_path(args.out, OUT)
    report.write_text("\n".join(lines) + "\n")
    print(f"{len(results)} tables, {flagged} columns flagged -> {report}")
    write_findings(args.json, findings)
    if args.json:
        print(f"{len(findings)} findings -> {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
