#!/usr/bin/env python3
"""Live check of the staging writer against real BigQuery, on a throwaway table.

Creates `dw_staging_visma.zz_live_check` (removed at the end, also after a failure) and runs the
write and delete-reconciliation cases the unit tests can only describe: a new row, a changed row,
an unchanged row that must not be rewritten, a record that disappears from a complete scan and
comes back, the guard against mass deletion, an empty complete scan, DataMart tombstones, and
another tenant's rows staying untouched.

It writes only to that one table. Needs working Google credentials.

Usage: scripts/check_bq_staging_live.py [--project P] [--dataset D]  (default: .env)
"""

from __future__ import annotations

import argparse
import sys
import time

from _bq_target import DATASET, PROJECT
from google.cloud import bigquery

from vsv2_tooling.bigquery.staging import stage

KEY = "tenant_id,referenceNumber"
TABLE = "zz_live_check"


class Check:
    def __init__(self, project: str, dataset: str) -> None:
        self.client = bigquery.Client(project=project)
        self.project, self.dataset = project, dataset
        self.table_id = f"{project}.{dataset}.{TABLE}"
        self.failures: list[str] = []
        self.run = 0

    def write(self, tenant: str, records: list[dict], **options) -> dict:
        self.run += 1
        return stage(
            project=self.project,
            dataset=self.dataset,
            table=TABLE,
            primary_key=KEY,
            records=records,
            run_id=f"run{self.run}",
            endpoint="live_check",
            source="erp_rest",
            tenant=tenant,
            client=self.client,
            **options,
        )

    def rows(self, tenant: str) -> dict[str, dict]:
        sql = (
            "SELECT JSON_VALUE(payload, '$.referenceNumber') AS n, "
            "JSON_VALUE(payload, '$.amount') AS amount, last_run_id, deleted_at, updated_at "
            f"FROM `{self.table_id}` WHERE tenant_id = '{tenant}'"
        )
        return {r.n: dict(r) for r in self.client.query(sql).result()}

    def expect(self, condition: bool, what: str) -> None:
        print(f"  {'ok  ' if condition else 'FAIL'} {what}")
        if not condition:
            self.failures.append(what)


def rec(tenant: str, number: str, **extra) -> dict:
    return {
        "tenant_id": tenant,
        "referenceNumber": number,
        "_source": "erp_rest",
        "_synced_at": "2026-10-02T10:00:00Z",
        **extra,
    }


def run(check: Check) -> None:
    a, b = "tenant-a", "tenant-b"
    print("insert, change, no-op")
    check.write(a, [rec(a, "1", amount=1), rec(a, "2", amount=2), rec(a, "3", amount=3)])
    check.write(b, [rec(b, "1", amount=10), rec(b, "2", amount=20)])
    before = check.rows(a)
    time.sleep(1)
    out = check.write(a, [rec(a, "1", amount=1), rec(a, "2", amount=22)])
    after = check.rows(a)
    check.expect(out["changed"] == 1, "only the changed row is written (changed == 1)")
    check.expect(after["1"] == before["1"], "an unchanged row is not rewritten")
    check.expect(after["2"]["amount"] == "22", "a changed row gets its new payload")

    print("the paging block is not a change")
    t = "tenant-p"
    page = {"totalCount": 5, "maxPageSize": 1000}
    check.write(t, [rec(t, "p1", amount=1, metadata=page)])
    kept = check.rows(t)["p1"]
    time.sleep(1)
    out = check.write(t, [rec(t, "p1", amount=1, metadata={**page, "totalCount": 6})])
    check.expect(out["changed"] == 0, "a new totalCount alone writes nothing (changed == 0)")
    check.expect(check.rows(t)["p1"] == kept, "so the row, updated_at included, is as it was")

    print("a complete scan marks what is missing, for its own tenant only")
    out = check.write(a, [rec(a, "1", amount=1), rec(a, "2", amount=22)], complete=True)
    rows_a, rows_b = check.rows(a), check.rows(b)
    check.expect(out["deleted"] == 1, "one row missing from the scan is marked deleted")
    check.expect(rows_a["3"]["deleted_at"] is not None, "it is row 3")
    check.expect(rows_a["1"]["deleted_at"] is None, "rows in the scan stay live")
    check.expect(all(r["deleted_at"] is None for r in rows_b.values()), "other tenant untouched")

    print("the record comes back")
    out = check.write(a, [rec(a, n, amount=int(n)) for n in "123"], complete=True)
    check.expect(check.rows(a)["3"]["deleted_at"] is None, "a returning record is live again")
    check.expect(out["deleted"] == 0, "nothing is deleted when nothing is missing")

    print("a partial scan is not mistaken for deletions")
    many = [rec(a, f"m{i}", amount=i) for i in range(100)]
    check.write(a, many)
    out = check.write(a, many[:10], complete=True)
    check.expect(out["deleted"] == 0 and out["delete_skipped"] >= 90, "refused: >20% missing")
    live = [r for r in check.rows(a).values() if r["deleted_at"] is None]
    check.expect(len(live) >= 100, "so the rows are still live")
    out = check.write(a, many[:10], complete=True, force_delete=True)
    check.expect(out["deleted"] >= 90, "force_delete overrides the guard")

    print("an empty complete scan")
    out = check.write(b, [], complete=True)
    check.expect(out["deleted"] == 2, "a small tenant whose records are all gone is emptied")
    check.expect(all(r["deleted_at"] is not None for r in check.rows(b).values()), "all marked")

    print("DataMart tombstones")
    t = "tenant-t"
    check.write(t, [rec(t, "x", amount=1, deletedDatabaseRecord=True), rec(t, "y", amount=2)])
    rows_t = check.rows(t)
    check.expect(rows_t["x"]["deleted_at"] is not None, "a tombstone arrives deleted")
    check.expect(rows_t["y"]["deleted_at"] is None, "a live record arrives live")
    check.write(t, [rec(t, "x", amount=1, deletedDatabaseRecord=True)])
    check.expect(check.rows(t)["x"]["deleted_at"] is not None, "a repeated tombstone stays deleted")
    check.write(t, [rec(t, "x", amount=1, deletedDatabaseRecord=False)])
    check.expect(check.rows(t)["x"]["deleted_at"] is None, "a record that is live again is revived")
    check.write(t, [rec(t, "y", amount=2, deletedDatabaseRecord=True)])
    check.expect(check.rows(t)["y"]["deleted_at"] is not None, "a live record can become deleted")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--project", default=PROJECT)
    ap.add_argument("--dataset", default=DATASET)
    args = ap.parse_args()
    check = Check(args.project, args.dataset)
    check.client.delete_table(check.table_id, not_found_ok=True)
    try:
        run(check)
    finally:
        check.client.delete_table(check.table_id, not_found_ok=True)
    print("FAILED: " + "; ".join(check.failures) if check.failures else "all cases passed")
    return 1 if check.failures else 0


if __name__ == "__main__":
    sys.exit(main())
