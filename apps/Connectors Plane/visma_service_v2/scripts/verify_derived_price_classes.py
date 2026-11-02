#!/usr/bin/env python3
"""Check that v2's derived `price_classes` equals v1's `visma_price_classes` for one tenant.

Builds the table with `vsv2_tooling.derived.price_classes` from the v2 bronze rows in the scratch
`vsv2_ingest_*` databases (`customers` for the descriptions, `customer_locations` for the
classes in use) and compares it with the live v1 table by read-only `bq` queries. Prints what
each side has that the other lacks and where the descriptions differ. Writes nothing.

Usage: scripts/verify_derived_price_classes.py [--tenant ID] [--project prj-dw-dev]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import reconcile_sources as rs  # noqa: E402

from vsv2_tooling.derived import price_classes  # noqa: E402

PILOT_TENANT = "33f319b9-4d57-11ee-960c-025417856183"


def bronze_rows(bronze: dict, endpoint: str) -> list[dict]:
    if endpoint not in bronze:
        return []
    sql = f"select payload::text from vsv2.local_bronze_record where endpoint = '{endpoint}'"
    return [json.loads(line) for line in rs.psql(bronze[endpoint][0], sql)]


def v1_rows(project: str, tenant: str) -> list[dict]:
    sql = (
        "SELECT id, description "
        f"FROM `{project}.dw_1_bronze_visma.visma_price_classes` "
        "WHERE tenantId = @tenant AND NOT COALESCE(isDeleted, FALSE)"
    )
    result = subprocess.run(
        [
            "bq",
            "query",
            "--use_legacy_sql=false",
            "--format=json",
            "--max_rows=1000000",  # bq returns only 100 rows by default
            f"--parameter=tenant::{tenant}",
            sql,
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tenant", default=PILOT_TENANT)
    ap.add_argument("--project", default="prj-dw-dev")
    args = ap.parse_args()

    bronze = rs.locate(rs.scratch_databases())
    locations = bronze_rows(bronze, "customer_locations")
    customers = bronze_rows(bronze, "customers")
    v2 = {r["id"]: r["description"] for r in price_classes(locations, customers)}
    v1 = {r["id"]: (r["description"] or "") for r in v1_rows(args.project, args.tenant)}

    print(f"inputs: {len(locations):,} customer_locations rows, {len(customers):,} customers rows")
    print(f"v2 derived: {len(v2)} classes | v1 table: {len(v1)} classes")
    only_v1, only_v2 = sorted(set(v1) - set(v2)), sorted(set(v2) - set(v1))
    differ = sorted(k for k in set(v1) & set(v2) if v1[k] != v2[k])
    print(f"only in v1: {len(only_v1)} {only_v1[:10]}")
    print(f"only in v2: {len(only_v2)} {only_v2[:10]}")
    print(f"description differs: {len(differ)} {[(k, v1[k], v2[k]) for k in differ[:5]]}")
    return 0 if not (only_v1 or only_v2 or differ) else 1


if __name__ == "__main__":
    raise SystemExit(main())
