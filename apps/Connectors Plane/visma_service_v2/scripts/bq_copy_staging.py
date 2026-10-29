#!/usr/bin/env python3
"""Copy the staging dataset to another GCP project, table by table, and check the copy.

Moving the pipeline to a new project must not start it from an empty warehouse: the state database
(`visma_v2`) holds a checkpoint per endpoint that says "everything up to here is in BigQuery", so
the next incremental sync would only add what changed and the new project would miss all history.
Copying the tables keeps those checkpoints true. (Without a copy, clear the checkpoints and run a
full pass instead; that reloads everything from Visma, hours of requests.)

Only reads the source. Writes the destination: creates its dataset when missing and replaces a
table that is already there, so the script can be run again. Skips the temporary `__stage_*`
tables and scratch `zz_*` tables. Source and destination must be in the same location (a table
copy within one region does not move data and is free). After the copy it compares row counts and
the number of live rows (`deleted_at IS NULL`) per table and exits 1 on any difference.

This is the tool of the move to aquatiq-datavarehus (2026-10-09, from prj-dw-dev). The source is
always named: the scripts' default project is the warehouse itself, which is never a source.

Usage: scripts/bq_copy_staging.py --from SOURCE_PROJECT --to DESTINATION_PROJECT
       [--dataset dw_staging_visma] [--to-dataset NAME] [--dry-run]
"""

from __future__ import annotations

import argparse
import sys

from _bq_target import DATASET
from google.api_core.exceptions import NotFound
from google.cloud import bigquery

SKIP_PREFIXES = ("__stage_", "zz_")


def source_tables(client: bigquery.Client, project: str, dataset: str) -> list[str]:
    return sorted(
        t.table_id
        for t in client.list_tables(f"{project}.{dataset}")
        if t.table_type == "TABLE" and not t.table_id.startswith(SKIP_PREFIXES)
    )


def ensure_destination(client: bigquery.Client, project: str, dataset: str, location: str) -> None:
    try:
        client.get_dataset(f"{project}.{dataset}")
    except NotFound:
        created = bigquery.Dataset(f"{project}.{dataset}")
        created.location = location
        created.description = "Raw staging of Visma v2: one JSON table per endpoint (vsv2)."
        client.create_dataset(created)
        print(f"created {project}.{dataset} in {location}")


def counts(client: bigquery.Client, table_id: str) -> tuple[int, int]:
    sql = f"SELECT COUNT(*) n, COUNTIF(deleted_at IS NULL) live FROM `{table_id}`"
    row = next(iter(client.query(sql).result()))
    return int(row.n), int(row.live)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--from", dest="src_project", required=True)
    ap.add_argument("--to", dest="dst_project", required=True)
    ap.add_argument("--dataset", default=DATASET, help="source dataset")
    ap.add_argument("--to-dataset", default=None, help="destination dataset (default: the same)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    dst_dataset = args.to_dataset or args.dataset
    if (args.src_project, args.dataset) == (args.dst_project, dst_dataset):
        print("source and destination are the same dataset", file=sys.stderr)
        return 2

    client = bigquery.Client(project=args.dst_project)
    source = client.get_dataset(f"{args.src_project}.{args.dataset}")
    tables = source_tables(client, args.src_project, args.dataset)
    print(f"{len(tables)} tables in {args.src_project}.{args.dataset} ({source.location})")
    if args.dry_run:
        print("dry run: nothing copied")
        return 0
    ensure_destination(client, args.dst_project, dst_dataset, source.location)
    destination = client.get_dataset(f"{args.dst_project}.{dst_dataset}")
    if destination.location != source.location:
        print(
            f"the destination is in {destination.location}, the source in {source.location}: "
            "create the destination dataset in the source's location, or use a transfer",
            file=sys.stderr,
        )
        return 2

    config = bigquery.CopyJobConfig(write_disposition=bigquery.WriteDisposition.WRITE_TRUNCATE)
    jobs = {
        table: client.copy_table(
            f"{args.src_project}.{args.dataset}.{table}",
            f"{args.dst_project}.{dst_dataset}.{table}",
            job_config=config,
        )
        for table in tables
    }
    for job in jobs.values():
        job.result()
    print(f"copied {len(jobs)} tables")

    wrong = []
    for table in tables:
        before = counts(client, f"{args.src_project}.{args.dataset}.{table}")
        after = counts(client, f"{args.dst_project}.{dst_dataset}.{table}")
        if before != after:
            wrong.append(f"{table}: source {before} (rows, live) vs copy {after}")
    total = sum(counts(client, f"{args.dst_project}.{dst_dataset}.{t}")[1] for t in tables)
    print(f"{total:,} live rows in {args.dst_project}.{dst_dataset}")
    if wrong:
        print("DIFFERENT:\n  " + "\n  ".join(wrong), file=sys.stderr)
        return 1
    print("every table has the same rows and the same live rows as the source")
    return 0


if __name__ == "__main__":
    sys.exit(main())
