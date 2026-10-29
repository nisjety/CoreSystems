#!/usr/bin/env python3
"""Compare every table and view the Dataform project builds with its v1 counterpart.

For each action in the compiled project it counts rows in this project and in v1 (the same table
under its old `dw_1_...` dataset name in prj-dw-dev) and writes the differences to
docs/dataform_v1_comparison.md. Read-only.

Two rules make the counts like-for-like (see parity_findings.py):
- A bronze table with a primary key in config/endpoints is compared on distinct keys, so v1's duplicate
  bronze rows do not count. The report shows the raw rows and the distinct keys on both sides.
- Where the table has a last-modified date set on every row on both sides, only the rows last modified
  before --cutoff count, on both sides. This removes the week of new transactions in v1 by construction.
  A table without such a date is compared on all rows, and the report says so.

v1 keeps running and the v2 staging data is a snapshot, so a small difference is expected on tables
that grow (facts). A table that is missing in v1, or that differs by more than 5 %, needs a look.

Usage: dataform/tools/compare_with_v1.py [--new aquatiq-datavarehus] [--old prj-dw-dev]
                                        [--cutoff 2026-10-01] [--out DIR] [--json PATH]
--out writes the report into DIR instead of docs/; --json writes the differences as findings
(see parity_findings.py). A table with no rows in v1 but rows here is a finding too: its ratio is
undefined, so the report shows n/a and does not flag it.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from google.cloud import bigquery
from parity_findings import (
    LOCATION,
    add_cutoff_option,
    add_output_options,
    before_cutoff,
    columns_by_table,
    cutoff_applies,
    finding,
    key_sql,
    keys_present,
    modified_column,
    primary_key,
    report_path,
    unset,
    write_findings,
)

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT.parent / "docs" / "dataform_v1_comparison.md"
SCHEMAS = {
    "visma_bronze": "dw_1_bronze_visma",
    "visma_bronze_excel": "dw_1_bronze_excel",
    "visma_silver": "dw_1_silver_visma",
    "visma_silver_common": "dw_1_silver_common",
    "visma_silver_excel": "dw_1_silver_excel",
    "visma_gold_common": "dw_1_gold_common",
    "visma_gold_finance": "dw_1_gold_finance",
    "visma_gold_sales": "dw_1_gold_sales",
    "visma_datamart": "dw_1_gold_bi",
}


def compiled() -> list[dict]:
    out = subprocess.run(
        ["dataform", "compile", "--timeout=180s", "--json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return json.loads(out)["tables"]


def stats(client, project, dataset, table, key, modified, cutoff) -> dict | None:
    """Rows of one table: all of them, and before the cutoff. Distinct keys likewise, when keyed.
    None when the table cannot be counted (missing on this side)."""
    parts = ["COUNT(*) AS n"]
    if modified:
        parts += [
            f"COUNTIF({unset(modified)}) AS unset",
            f"COUNTIF({before_cutoff(modified, cutoff)}) AS before",
        ]
    if key:
        parts.append(f"COUNT(DISTINCT {key}) AS nd")
        if modified:
            parts.append(
                f"COUNT(DISTINCT IF({before_cutoff(modified, cutoff)}, {key}, NULL)) AS nd_before"
            )
    sql = f"SELECT {', '.join(parts)} FROM `{project}.{dataset}.{table}`"
    try:
        row = next(iter(client.query(sql, location=LOCATION).result()))
    except Exception:
        return None
    return dict(row.items())


def in_scope(st: dict | None, usable: bool) -> tuple[int | None, int | None]:
    """(rows, distinct keys) in the comparison scope; None where the table could not be counted."""
    if st is None:
        return None, None
    if usable:
        return st["before"], st.get("nd_before")
    return st["n"], st.get("nd")


def row_note(new: int | None, old: int | None) -> str | None:
    if new is None:
        return "count failed or table missing in this project"
    if old is None:
        return "count failed or table missing in v1"
    if not old:
        return "v1 has no rows, so the ratio is undefined"
    return None


def fmt(value: int | None) -> str:
    return "-" if value is None else f"{value:,}"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--new", default="aquatiq-datavarehus")
    ap.add_argument("--old", default="prj-dw-dev")
    add_cutoff_option(ap)
    add_output_options(ap)
    args = ap.parse_args()
    cutoff = args.cutoff
    client = bigquery.Client(project=args.new)
    actions = [t for t in compiled() if t["target"]["schema"] in SCHEMAS]
    cols_new = {s: columns_by_table(client, args.new, s) for s in SCHEMAS}
    cols_old = {SCHEMAS[s]: columns_by_table(client, args.old, SCHEMAS[s]) for s in SCHEMAS}

    def one(t: dict) -> tuple:
        schema, name = t["target"]["schema"], t["target"]["name"]
        old_ds = SCHEMAS[schema]
        a = cols_new[schema].get(name, {})
        b = cols_old[old_ds].get(name, {})
        keys = primary_key(name) if schema == "visma_bronze" else None
        key = key_sql(keys) if keys and keys_present(keys, a, b) else None
        modified = modified_column(a, b) if cutoff else None
        st_new = stats(client, args.new, schema, name, key, modified, cutoff)
        st_old = stats(client, args.old, old_ds, name, key, modified, cutoff)
        usable = cutoff_applies(modified, cutoff, st_new, st_old)
        raw_new, dist_new = in_scope(st_new, usable)
        raw_old, dist_old = in_scope(st_old, usable)
        new, old = (dist_new, dist_old) if key else (raw_new, raw_old)
        if usable:
            scope = f"before {cutoff}"
        elif modified:
            scope = f"all rows, {modified} empty on a side"
        else:
            scope = "all rows"
        ratio = None if not old or new is None else new / old
        return (
            schema,
            name,
            t.get("type"),
            scope,
            key is not None,
            raw_new,
            raw_old,
            dist_new,
            dist_old,
            new,
            old,
            ratio,
            row_note(new, old),
        )

    with ThreadPoolExecutor(max_workers=12) as pool:
        rows = sorted(pool.map(one, actions), key=lambda r: (r[0], r[1]))

    if cutoff:
        cut_text = f"rows last modified before {cutoff} where the table has such a date on every row on both sides, all rows otherwise"
    else:
        cut_text = "all rows (cutoff turned off)"
    lines = [
        "# Dataform: this project against v1",
        "",
        "> Generated by `dataform/tools/compare_with_v1.py`. Row counts of every table and view the",
        f"> project builds, here and in v1 (`prj-dw-dev`, `dw_1_...`). Scope: {cut_text}.",
        "> Bronze tables with a primary key are compared on distinct keys, so v1's duplicate rows do not",
        "> count; raw rows are shown too. v1 keeps syncing and v2 staging is a snapshot. `Ratio` = new / v1",
        "> of the compared count (distinct keys where shown, rows otherwise).",
        "",
        "| Dataset | Table | Type | Scope | Rows new | Rows v1 | Distinct new | Distinct v1 | Ratio |",
        "|---|---|---|---|---:|---:|---:|---:|---:|",
    ]
    flagged = []
    findings = []
    keyed = cut = 0
    for (
        schema,
        name,
        kind,
        scope,
        is_keyed,
        raw_new,
        raw_old,
        dist_new,
        dist_old,
        new,
        old,
        ratio,
        note,
    ) in rows:
        keyed += is_keyed
        cut += scope.startswith("before")
        shown = "n/a" if ratio is None else f"{ratio:.3f}"
        lines.append(
            f"| {schema} | `{name}` | {kind} | {scope} | {fmt(raw_new)} | {fmt(raw_old)} "
            f"| {fmt(dist_new)} | {fmt(dist_old)} | {shown} |"
        )
        differs = new is None or old is None or (ratio is not None and abs(ratio - 1) > 0.05)
        if differs:
            flagged.append((schema, name, new, old, ratio))
        if differs or (old == 0 and new):
            findings.append(finding("compare_with_v1", schema, name, "row_count", old, new, note))
    lines += [
        "",
        f"{len(rows)} objects; {keyed} compared on distinct keys; {cut} on the cutoff; "
        f"{len(flagged)} with a missing side or a ratio more than 5 % from 1.",
        "",
    ]
    report = report_path(args.out, OUT)
    report.write_text("\n".join(lines))
    print(f"{len(rows)} objects compared -> {report}")
    for schema, name, new, old, ratio in flagged:
        r = "n/a" if ratio is None else f"{ratio:.3f}"
        print(f"  {schema}.{name}: new={new} v1={old} ratio={r}")
    write_findings(args.json, findings)
    if args.json:
        print(f"{len(findings)} findings -> {args.json}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
