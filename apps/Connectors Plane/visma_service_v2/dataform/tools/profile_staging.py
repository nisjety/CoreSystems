#!/usr/bin/env python3
"""Profile the raw staging dataset: per endpoint, which top-level payload keys exist and what
JSON types they hold. The bronze generator reads the result (tools/contract/staging_profile.json),
so the bronze layer can be regenerated without touching BigQuery.

Counts come from a scan of every live row (`JSON_KEYS`); types from a sample of up to SAMPLE rows
per table. Read-only.

Usage: dataform/tools/profile_staging.py [--project aquatiq-datavarehus] [--dataset dw_staging_visma]
"""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from google.cloud import bigquery

SAMPLE = 3000
OUT = Path(__file__).parent / "contract" / "staging_profile.json"


def json_type(value) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, int):
        return "int"
    if isinstance(value, float):
        return "float"
    if isinstance(value, str):
        return "string"
    return "object" if isinstance(value, dict) else "array"


def profile(client: bigquery.Client, project: str, dataset: str, table: str) -> dict:
    ref = f"`{project}.{dataset}.{table}`"
    counts = {
        row.k: row.n
        for row in client.query(
            f"SELECT k, COUNT(*) AS n FROM {ref}, UNNEST(JSON_KEYS(payload, 1)) AS k "
            "WHERE deleted_at IS NULL GROUP BY k",
            location="europe-north2",
        ).result()
    }
    rows = client.query(
        f"SELECT TO_JSON_STRING(payload) AS p FROM {ref} WHERE deleted_at IS NULL "
        f"ORDER BY FARM_FINGERPRINT(record_key) LIMIT {SAMPLE}",
        location="europe-north2",
    ).result()
    types: dict[str, set[str]] = defaultdict(set)
    sampled = 0
    for row in rows:
        sampled += 1
        for key, value in json.loads(row.p).items():
            types[key].add(json_type(value))
    live = next(
        iter(client.query(f"SELECT COUNT(*) AS n FROM {ref} WHERE deleted_at IS NULL").result())
    ).n
    return {
        "live_rows": live,
        "sampled": sampled,
        "keys": {
            k: {"rows": n, "types": sorted(types.get(k, []))} for k, n in sorted(counts.items())
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--project", default="aquatiq-datavarehus")
    ap.add_argument("--dataset", default="dw_staging_visma")
    args = ap.parse_args()
    client = bigquery.Client(project=args.project)
    tables = sorted(
        t.table_id
        for t in client.list_tables(f"{args.project}.{args.dataset}")
        if not t.table_id.startswith(("__stage_", "zz_"))
    )
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(
            pool.map(lambda t: (t, profile(client, args.project, args.dataset, t)), tables)
        )
    OUT.write_text(json.dumps(dict(results), indent=1, sort_keys=True) + "\n")
    print(f"{len(results)} endpoints profiled -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
