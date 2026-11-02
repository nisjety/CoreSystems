#!/usr/bin/env python3
"""Copy the three inputs the Visma Dataform project reads that are not Visma endpoints.

  * visma_bronze_excel.raw_entity_mapping, raw_charts_of_accounts   mapping workbook (reference_data.xlsx)
  * dw_staging_currency.Historyrates, Rate + the two PowerBI views   exchange rates

They come from the old project (dw_1_bronze_excel, dw_1_staging_currency_n2) as a one-time
snapshot. Nothing keeps them current: the currency service still writes to the old project, and
the workbook is loaded by hand. Replace the snapshot when those move. Only reads the old project;
replaces the copies in the new one, so it can run again.

Usage: dataform/tools/copy_external_inputs.py [--from prj-dw-dev] [--to aquatiq-datavarehus]
"""

from __future__ import annotations

import argparse
import re

from google.cloud import bigquery

LOCATION = "europe-north2"
COPIES = [
    ("dw_1_bronze_excel", "visma_bronze_excel", ["raw_entity_mapping", "raw_charts_of_accounts"]),
    ("dw_1_staging_currency_n2", "dw_staging_currency", ["Historyrates", "Rate"]),
]
VIEWS = [
    ("dw_1_staging_currency_n2", "dw_staging_currency", ["HistoryratesPowerBI", "RatePowerBI"])
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--from", dest="src", default="prj-dw-dev")
    ap.add_argument("--to", dest="dst", default="aquatiq-datavarehus")
    args = ap.parse_args()
    client = bigquery.Client(project=args.dst)
    config = bigquery.CopyJobConfig(write_disposition=bigquery.WriteDisposition.WRITE_TRUNCATE)

    for src_ds, dst_ds, tables in COPIES:
        for table in tables:
            job = client.copy_table(
                f"{args.src}.{src_ds}.{table}", f"{args.dst}.{dst_ds}.{table}", job_config=config
            )
            job.result()
            rows = client.get_table(f"{args.dst}.{dst_ds}.{table}").num_rows
            print(f"copied {src_ds}.{table} -> {dst_ds}.{table} ({rows:,} rows)")

    for src_ds, dst_ds, views in VIEWS:
        for view in views:
            ddl = next(
                iter(
                    client.query(
                        f"SELECT view_definition FROM `{args.src}.{src_ds}.INFORMATION_SCHEMA.VIEWS` "
                        f"WHERE table_name = '{view}'",
                        location=LOCATION,
                    ).result()
                )
            ).view_definition
            ddl = re.sub(
                rf"`?{re.escape(args.src)}\.{re.escape(src_ds)}\.", f"`{args.dst}.{dst_ds}.", ddl
            )
            client.query(
                f"CREATE OR REPLACE VIEW `{args.dst}.{dst_ds}.{view}` AS {ddl}", location=LOCATION
            ).result()
            print(f"view {dst_ds}.{view}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
