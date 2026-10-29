#!/usr/bin/env python3
"""
CLI entrypoint for the Moment -> BigQuery sync, meant to be run by cron.

    python run_sync.py                     # sync every entity
    python run_sync.py --entities time_records,invoices
    python run_sync.py --list              # show valid entity names

Duplicate/gap safety: every entity is fetched in full (all pages), staged, and
applied with a single atomic MERGE (upsert + soft delete). The snapshot is
deduped on id first, and the MERGE either applies completely or not at all, so
a failed run leaves the previous table untouched - re-running can never
duplicate rows, remove history, or leave a table partially written.

Exit code is 0 only when every requested entity synced successfully, so cron
can alert on failures (e.g. via MAILTO or a wrapper that checks $?).
"""

import argparse
import json
import logging
import sys
import time

# Windows consoles default to a legacy codepage that can't encode unicode
# symbols; harmless on the Linux VPS, needed for local runs.
if sys.stdout.encoding and sys.stdout.encoding.lower() != "utf-8":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger("run_sync")


def main() -> int:
    parser = argparse.ArgumentParser(description="Sync Moment data to BigQuery (full refresh).")
    parser.add_argument(
        "--entities",
        help="Comma-separated entity names to sync (default: all)",
        default=None,
    )
    parser.add_argument("--list", action="store_true", help="List valid entity names and exit")
    args = parser.parse_args()

    # Imports deferred until after arg parsing so --list/--help don't require
    # working credentials.
    import sync as moment_sync

    if args.list:
        print("\n".join(moment_sync.ALL_ENTITIES))
        return 0

    from clients import MomentAPIClient
    from shared.gcp_client import GCPClient
    from bigquery_sync import BigQuerySync

    entities = None
    if args.entities:
        entities = [e.strip() for e in args.entities.split(",") if e.strip()]
        unknown = [e for e in entities if e not in moment_sync.ALL_ENTITIES]
        if unknown:
            logger.error(f"Unknown entities: {unknown}. Valid: {moment_sync.ALL_ENTITIES}")
            return 2

    client = MomentAPIClient()
    gcp = GCPClient()
    bq_sync = BigQuerySync(gcp)

    logger.info(f"Starting sync -> {gcp.project_id}.{gcp.dataset_id} ({gcp.region})")
    t0 = time.time()
    result = moment_sync.sync_all(client, bq_sync, entities=entities)
    elapsed = time.time() - t0

    def fmt(r):
        parts = [f"fetched={r.get('fetched', 0)}", f"merged={r.get('records_synced', 0)}"]
        for key, label in (("inserted", "new"), ("updated", "updated"), ("soft_deleted_total", "flagged_deleted"), ("table_total_rows", "table_rows")):
            if key in r:
                parts.append(f"{label}={r[key]}")
        parts.append("OK" if r.get("success") else f"FAILED: {r.get('error')}")
        return " ".join(parts)

    for entity, r in result["results"].items():
        logger.info(f"  {entity}: {fmt(r)}")
        if r.get("invoice_lines"):
            logger.info(f"  invoice_lines: {fmt(r['invoice_lines'])}")

    summary = {
        "success": result["success"],
        "entities_synced": result.get("entities_synced"),
        "total_records_fetched": result.get("total_records_fetched"),
        "elapsed_seconds": round(elapsed, 1),
    }
    logger.info(f"Sync finished: {json.dumps(summary)}")

    return 0 if result["success"] else 1


if __name__ == "__main__":
    sys.exit(main())
