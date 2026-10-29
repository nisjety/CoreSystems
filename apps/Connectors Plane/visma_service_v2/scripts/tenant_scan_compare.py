#!/usr/bin/env python3
"""Per-tenant check of the two period-scanned endpoints against v1.

For every tenant: how many `project_transactions` documents and `general_ledger_balances_v2`
balances v1 holds (read-only `bq`) and v2 fetched (a scratch database with rows of several
tenants), which v1 keys v2 lacks, whether one (module, refNbr) came back in several periods, and
the first and last period with data. The first period matters: `period_scan.first_period` in the
endpoint configs must not be later than a tenant's first data.

The tenants are the ones v1 registers (scripts/add_tenants.py in v1). Run the syncs first:

  vsv2 sync --tenant <id> --endpoint project_transactions         (once per tenant, same database)
  vsv2 sync --tenant <id> --endpoint general_ledger_balances_v2

Usage: scripts/tenant_scan_compare.py --db vsv2_alltenants_20260930 \
           [--pilot-db vsv2_ingest_ps2,vsv2_ingest_ps]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402
import v1_v2_key_diff as kd  # noqa: E402

PROJECT = "prj-dw-dev"
PILOT = "33f319b9-4d57-11ee-960c-025417856183"
TENANTS = [
    (PILOT, "Aquatiq AS (pilot)", "active"),
    ("ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd", "Aqua Pharma AS", "active"),
    ("ffa4a061-8084-11e9-b307-0aa512338dd6", "Aqua Pharma Group AS", "active"),
    ("2b6e5cd6-6ce0-11ee-970e-025417856183", "Aquatiq AB", "active"),
    ("d1507c4a-7717-11ee-970e-025417856183", "Aquatiq Aps", "active"),
    ("bed46bb6-21cc-11e9-b307-0aa512338dd6", "Aquatiq Food Automation ApS", "active"),
    ("7d2c489e-21cc-11e9-b307-0aa512338dd6", "Aquatiq Group AS", "active"),
    ("eda0dd16-fa25-11ed-a0e0-0693d8a7c3dd", "Sensilist AS", "active"),
    ("9b1fa69b-5568-11ee-960c-025417856183", "Consult AS", "historical"),
    ("f630d6b0-5566-11ee-960c-025417856183", "Aquatiq Food Factory AS", "historical"),
    ("61320c0a-102d-11ee-a0e0-0693d8a7c3dd", "Aquatiq Hygiene Systems AS", "historical"),
    ("bb6c024e-5567-11ee-960c-025417856183", "Aquatiq Sense AS", "historical"),
]
PT_COLUMNS = "(payload->>'module')||'|'||(payload->>'refNbr'), payload->>'_scan_period'"
GL_COLUMNS = (
    "coalesce(payload->'account'->>'number','')||'|'||coalesce(payload->>'subAccountCd','')"
    "||'|'||(payload->>'financialPeriod')||'|'||coalesce(payload->'ledger'->>'id',''), "
    "payload->>'financialPeriod'"
)


def fetch(dbs: list[str], endpoint: str, tenant: str, columns: str) -> tuple[list[str], list[str]]:
    """(keys, periods) of every row of one tenant and endpoint, across the databases."""
    keys, periods = [], []
    for db in dbs:
        sql = (
            f"select {columns} from vsv2.local_bronze_record "
            f"where endpoint='{endpoint}' and tenant_id='{tenant}'"
        )
        for line in rs.psql(db, sql):
            key, _, period = line.partition(rs.SEP)
            keys.append(key)
            periods.append(period)
    return keys, periods


def span(periods: list[str]) -> str:
    found = sorted(p for p in periods if p)
    return f"{found[0]}..{found[-1]}" if found else "-"


def project_transactions(dbs: list[str], tenant: str) -> dict:
    keys, periods = fetch(dbs, "project_transactions", tenant, PT_COLUMNS)
    v1 = kd.v1_keys(PROJECT, "visma_project_transactions", tenant, ["module", "refNbr"])
    return {"v1": v1, "v2": set(keys), "rows": len(keys), "span": span(periods)}


def gl_balances(dbs: list[str], tenant: str) -> dict:
    keys, periods = fetch(dbs, "general_ledger_balances_v2", tenant, GL_COLUMNS)
    fields = ["account.number", "subAccountCd", "financialPeriod", "ledger.id"]
    v1 = kd.v1_keys(PROJECT, "visma_general_ledger_balances_v2", tenant, fields)
    return {"v1": v1, "v2": set(keys), "rows": len(keys), "span": span(periods)}


def only_v1(side: dict) -> str:
    return str(len(side["v1"] - side["v2"])) if isinstance(side["v1"], set) else "n/a"


def count(side: dict) -> str:
    return f"{len(side['v1']):,}" if isinstance(side["v1"], set) else "n/a"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--db", required=True, help="scratch database holding the tenants' rows")
    ap.add_argument(
        "--pilot-db", default="", help="comma-separated databases holding the pilot tenant's rows"
    )
    args = ap.parse_args()

    print(
        f"{'tenant':30} {'project transactions':^52} | {'GL balances':^44}\n"
        f"{'':30} {'v1':>7} {'v2':>7} {'v1 only':>7} {'multi-period':>12} {'periods':<16} | "
        f"{'v1':>7} {'v2':>7} {'v1 only':>7} {'periods':<14}"
    )
    for tenant, name, _kind in TENANTS:
        dbs = args.pilot_db.split(",") if tenant == PILOT and args.pilot_db else [args.db]
        pt, gl = project_transactions(dbs, tenant), gl_balances(dbs, tenant)
        print(
            f"{name:30} {count(pt):>7} {len(pt['v2']):>7,} {only_v1(pt):>7} "
            f"{pt['rows'] - len(pt['v2']):>12,} {pt['span']:<16} | "
            f"{count(gl):>7} {len(gl['v2']):>7,} {only_v1(gl):>7} {gl['span']:<14}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
