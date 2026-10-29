#!/usr/bin/env python3
"""Compare the live v1 BigQuery bronze tables with what v2 fetched, per endpoint.

v1 is what feeds Dataform today. For the pilot tenant this measures each v1 table
(`dw_1_bronze_visma.visma_<endpoint>`) with read-only `SELECT COUNT` queries through the `bq`
CLI: all rows, rows not marked deleted, and DISTINCT keys under v1's own primary key and under
v2's. It sets those next to the rows v2 landed in the scratch `vsv2_ingest_*` databases.

Reading it:
  v1 rows / v1 keys   more than 1 means v1 holds duplicates of the same key
  v1 keys (v2 key)    below the v2 row count means v1's MERGE key merged distinct rows
  v2 rows             what one full fetch returned

It compares counts, not rows, and the two sides were fetched at different times, so a little drift
is normal. Nothing is written anywhere except docs/v1_v2_comparison.md.

Usage:
  scripts/v1_v2_row_counts.py
  scripts/v1_v2_row_counts.py --tenant <id> --project prj-dw-dev
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
V1_CONFIGS = ROOT.parent / "sync/endpoint_configs"
OUT = ROOT / "docs/v1_v2_comparison.md"
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402

DATASET = "dw_1_bronze_visma"
PILOT_TENANT = "33f319b9-4d57-11ee-960c-025417856183"

# v1 table -> the v2 config that reads the same URL, where the names differ. v1 reads customers and
# inventory from the Sales Order API, so its `customers` is v2's `sales_order_customers`, not the
# ERP `customers`. A v1 table built by v1's own code from other tables is listed as derived.
V1_TO_V2 = {
    "customers": "sales_order_customers",
    "inventory_items": "sales_order_inventory",
    "invoices": "customer_invoices",
    "sales_orders": "sales_orders_v3",
    "sales_orders_v2": "sales_orders_v3",
}
DERIVED = {"price_classes": "scripts/verify_derived_price_classes.py"}


def table_ref(project: str, table: str) -> str:
    """Backticked table reference; a v1 `bigquery_table` may be `dataset.table`."""
    dataset, _, name = table.rpartition(".")
    return f"`{project}.{dataset or DATASET}.{name}`"


def as_list(key) -> list[str]:
    return key if isinstance(key, list) else [key]


def key_sql(fields: list[str]) -> str:
    """A row's key as one JSON string, from BigQuery column paths (`a.b` for nested fields)."""
    parts = ", ".join(f"{f} AS k{i}" for i, f in enumerate(fields))
    return f"TO_JSON_STRING(STRUCT({parts}))"


def run_query(sql: str, tenant: str) -> dict | str:
    result = subprocess.run(
        [
            "bq",
            "query",
            "--use_legacy_sql=false",
            "--format=json",
            f"--parameter=tenant::{tenant}",
            sql,
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return json.loads(result.stdout)[0]
    return result.stderr.strip().splitlines()[-1][:120] if result.stderr.strip() else "bq failed"


def measure(project: str, table: str, tenant: str, v1_key: list[str], v2_key: list[str]) -> dict:
    """Rows, not-deleted rows and distinct keys of one v1 table for one tenant.

    A distinct-key count is None when the key is not a column path of the table.
    """
    live = "NOT COALESCE(isDeleted, FALSE)"
    base = f"FROM {table_ref(project, table)} WHERE tenantId = @tenant"
    counts = f"COUNT(*) AS total, COUNTIF({live}) AS live"
    out: dict = {"error": None, "v1_keys": None, "v2_keys": None}
    for label, key in (("v1_keys", v1_key), ("v2_keys", v2_key)):
        if label == "v2_keys" and [k.lower() for k in key] == [k.lower() for k in v1_key]:
            out["v2_keys"] = out["v1_keys"]
            continue
        sql = f"SELECT {counts}, COUNT(DISTINCT IF({live}, {key_sql(key)}, NULL)) AS keys {base}"
        row = run_query(sql, tenant)
        if isinstance(row, dict):
            out.update(total=int(row["total"]), live=int(row["live"]), **{label: int(row["keys"])})
    if "total" not in out:
        row = run_query(f"SELECT {counts} {base}", tenant)
        if isinstance(row, dict):
            out.update(total=int(row["total"]), live=int(row["live"]))
        else:
            out["error"] = row
    return out


# (v2 endpoint, v1 table, document type column, number column) where ERP numbers documents per type,
# so two different documents can share one number.
SHARED_NUMBER_TABLES = [
    ("customer_documents", "visma_customer_documents", "documentType", "referenceNumber"),
    ("supplier_invoices", "visma_supplier_invoices", "documentType", "referenceNumber"),
    ("supplier_payments", "visma_supplier_payments", "type", "refNbr"),
    ("customer_payments", "visma_customer_payments", "type", "refNbr"),
]


def shared_number_check(project: str, tenant: str, bronze: dict) -> list[tuple]:
    """(endpoint, numbers shared by 2+ documents in v2, same in v1 counting deleted rows too)."""
    found = []
    for endpoint, table, type_col, number_col in SHARED_NUMBER_TABLES:
        if endpoint not in bronze:
            continue
        sql = (
            "select count(*), count(distinct payload->>'" + number_col + "') "
            "from vsv2.local_bronze_record where endpoint = '" + endpoint + "'"
        )
        rows, distinct = (int(x) for x in rs.psql(bronze[endpoint][0], sql)[0].split(rs.SEP))
        in_v1 = run_query(
            f"SELECT COUNT(*) AS n FROM (SELECT {number_col} FROM "
            f"{table_ref(project, table)} WHERE tenantId = @tenant GROUP BY {number_col} "
            f"HAVING COUNT(DISTINCT {type_col}) > 1)",
            tenant,
        )
        found.append((endpoint, rows - distinct, in_v1["n"] if isinstance(in_v1, dict) else in_v1))
    return found


def fmt(value: int | None) -> str:
    return "n/a" if value is None else f"{value:,}"


def load_v1() -> dict[str, dict]:
    found = {}
    for path in sorted(V1_CONFIGS.glob("*.json")):
        try:
            cfg = json.loads(path.read_text())
        except ValueError:
            continue
        found[cfg.get("endpoint_name") or path.stem] = cfg
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tenant", default=PILOT_TENANT)
    ap.add_argument("--project", default="prj-dw-dev")
    args = ap.parse_args()

    v1 = load_v1()
    v2_configs = rs.load_configs()
    bronze = rs.locate(rs.scratch_databases())
    shared = [n for n in sorted(v1) if n not in DERIVED and V1_TO_V2.get(n, n) in v2_configs]
    only_v1 = [n for n in sorted(v1) if n not in shared and n not in DERIVED]

    def job(name: str) -> tuple[str, dict]:
        v2_key = as_list(v2_configs[V1_TO_V2.get(name, name)]["primary_key"])
        table = v1[name].get("bigquery_table", f"visma_{name}")
        return name, measure(
            args.project, table, args.tenant, as_list(v1[name]["primary_key"]), v2_key
        )

    with ThreadPoolExecutor(max_workers=6) as pool:
        measured = dict(pool.map(job, shared))

    lines = [
        "# v1 vs v2: the pilot tenant",
        "",
        "> Generated by `scripts/v1_v2_row_counts.py`. v1 = `dw_1_bronze_visma`, read-only counts,",
        "> v2 = one full fetch landed in the scratch ingest databases. Fetched at different times.",
        "",
        "`v1 rows / keys` above 1 = duplicates of one key in v1. "
        "`v1 keys (v2 key)` below `v2 rows` "
        "where the keys differ = rows v1's key merged.",
        "",
        "| Endpoint | v1 key | v2 key | v1 rows | v1 not deleted | v1 keys (v1 key) "
        "| v1 keys (v2 key) | v2 rows |",
        "|---|---|---|---|---|---|---|---|",
    ]
    findings = []
    for name in shared:
        target = V1_TO_V2.get(name, name)
        k1 = as_list(v1[name]["primary_key"])
        k2 = as_list(v2_configs[target]["primary_key"])
        m = measured[name]
        v2_rows = bronze.get(target, ("", 0))[1]
        label = f"`{name}`" + (f" (v2: `{target}`)" if target != name else "")
        differs = [x.lower() for x in k1] != [x.lower() for x in k2]
        if m["error"]:
            lines.append(
                f"| {label} | {', '.join(k1)} | {', '.join(k2)} | {m['error']} | | | | "
                f"{v2_rows:,} |"
            )
            continue
        lines.append(
            f"| {label} | {', '.join(k1)}{' **≠**' if differs else ''} | {', '.join(k2)} "
            f"| {m['total']:,} | {m['live']:,} | {fmt(m['v1_keys'])} | {fmt(m['v2_keys'])} "
            f"| {v2_rows:,} |"
        )
        findings.append((name, m, v2_rows, differs))
    numbers = shared_number_check(args.project, args.tenant, bronze)
    lines += [
        "",
        "## Numbers shared by two documents of different types",
        "",
        "ERP numbers documents per type, so an Invoice and a CreditNote can both be `015034`. "
        "v1 keys these tables on the number alone. If v1 kept both, it would hold numbers with "
        "more than one type; it holds none, so at least one document per shared number is missing.",
        "",
        "| Table | Numbers shared in ERP | Numbers with 2+ types in v1 (incl. deleted) |",
        "|---|---|---|",
    ]
    lines += [f"| `{e}` | {n:,} | {v1} |" for e, n, v1 in numbers]
    lines += ["", "## Derived in v2 (built from other tables, not fetched)", ""]
    lines += [f"- `{n}`: checked by `{how}`" for n, how in DERIVED.items()]
    lines += ["", "## Only in v1 (no v2 config)", ""]
    lines += [f"- `{n}`" for n in only_v1] or ["- none"]
    OUT.write_text("\n".join(lines) + "\n")

    print(f"wrote {OUT.relative_to(ROOT)}: {len(measured)} compared, {len(only_v1)} only in v1")
    for endpoint, in_erp, in_v1 in numbers:
        print(f"  shared numbers {endpoint:22} ERP {in_erp:>3}  v1 {in_v1}")
    print(
        f"\n{'endpoint':30} {'v1 rows':>9} {'v1 keys':>9} {'dup x':>6} "
        f"{'v1 keys(v2key)':>15} {'v2 rows':>9}"
    )
    for name, m, v2_rows, differs in sorted(
        findings, key=lambda f: -(f[1]["live"] / max(f[1]["v1_keys"] or 1, 1))
    ):
        keys = m["v1_keys"]
        dup = f"{m['live'] / keys:.1f}" if keys else "n/a"
        mark = " *" if differs else ""
        print(
            f"{name:30} {m['live']:>9,} {fmt(keys):>9} {dup:>6} "
            f"{fmt(m['v2_keys']):>15} {v2_rows:>9,}{mark}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
