#!/usr/bin/env python3
"""Set `deleted_at` on staging rows that carry a DataMart tombstone.

DataMart marks a deleted record with `deletedDatabaseRecord: true`. The staging writer sets
`deleted_at` for such a record as it arrives, but rows loaded before that rule existed have
`deleted_at` NULL, and an incremental DataMart sync only re-sends rows that changed. This sets it
once for everything already there: from the record's own `updatedInDataMartDateTime` where it has
one, else from when the row last changed.

Only `dm_*` tables are touched, only rows with `deleted_at IS NULL`, and running it twice changes
nothing the second time.

Usage: scripts/bq_backfill_tombstones.py [--project P] [--dataset D] [--dry-run]
"""

from __future__ import annotations

import argparse

from _bq_target import DATASET, PROJECT
from google.cloud import bigquery

WHERE = "deleted_at IS NULL AND JSON_VALUE(payload, '$.deletedDatabaseRecord') = 'true'"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--project", default=PROJECT)
    ap.add_argument("--dataset", default=DATASET)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    client = bigquery.Client(project=args.project)
    tables = sorted(
        t.table_id
        for t in client.list_tables(f"{args.project}.{args.dataset}")
        if t.table_id.startswith("dm_")
    )
    total = 0
    for table in tables:
        ref = f"`{args.project}.{args.dataset}.{table}`"
        if args.dry_run:
            sql = f"SELECT COUNT(*) AS n FROM {ref} WHERE {WHERE}"
            n = next(iter(client.query(sql).result())).n
        else:
            sql = (
                f"UPDATE {ref} SET deleted_at = COALESCE(SAFE_CAST("
                "JSON_VALUE(payload, '$.updatedInDataMartDateTime') AS TIMESTAMP), updated_at) "
                f"WHERE {WHERE}"
            )
            job = client.query(sql)
            job.result()
            n = job.num_dml_affected_rows or 0
        total += n
        if n:
            print(f"{table:42} {n:>8,}")
    verb = "would mark" if args.dry_run else "marked"
    print(f"{verb} {total:,} tombstoned rows in {len(tables)} tables")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
