"""Provision an empty fresh staging contract. Never import old raw files."""

import argparse

from google.cloud import bigquery

from etl.common.bq_writer import BQWriter
from etl.common.fresh_writer import EndpointBQWriter, FreshBQWriter, WarehouseRawBQWriter
from etl.xero.fresh_views import view_queries
from etl.xero.fresh_quality import issue_query


def install_view(client, project, dataset, name, query):
    view = bigquery.Table(f"{project}.{dataset}.{name}")
    view.view_query = query
    view.labels = {"source": "xero_v2", "layer": "staging"}
    view.description = "Read-only projection of isolated, immutable Xero v2 observations."
    existing = client.create_table(view, exists_ok=True)
    if existing.table_type != "VIEW":
        raise ValueError("fresh view name is already occupied by a business table")
    client.update_table(view, ["view_query", "labels", "description"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--dataset", default="dw_staging_xero")
    parser.add_argument("--layout", choices=("legacy", "endpoint", "warehouse"), default="warehouse")
    args = parser.parse_args()
    if args.layout == "warehouse":
        WarehouseRawBQWriter(args.project, args.dataset).bootstrap()
        print("Provisioned natural-name raw Xero history tables; no source records imported.")
        return
    if args.layout == "endpoint":
        EndpointBQWriter(args.project, args.dataset).bootstrap()
        print("Provisioned natural-name Xero staging tables; no source records imported.")
        return
    writer = FreshBQWriter(args.project, args.dataset)
    writer.bootstrap()
    for name, query in view_queries(args.project, args.dataset).items():
        install_view(writer.client, args.project, args.dataset, name, query)
    install_view(writer.client, args.project, args.dataset,
                 "v2_quality_issues", issue_query(args.project, args.dataset))
    from etl.xero.endpoint_coverage import CATALOG_SCHEMA, catalog_rows, coverage_query
    BQWriter(args.project, args.dataset).insert_if_absent(
        "endpoint_catalog", catalog_rows(), ("spec_commit", "api_family", "operation_id"), CATALOG_SCHEMA,
    )
    install_view(writer.client, args.project, args.dataset,
                 "endpoint_coverage", coverage_query(args.project, args.dataset))
    print("Provisioned v2 staging tables and views; no source records imported.")


if __name__ == "__main__":
    main()
