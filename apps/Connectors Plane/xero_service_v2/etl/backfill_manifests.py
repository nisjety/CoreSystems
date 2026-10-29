"""Replay durable Xero run manifests into queryable warehouse history."""

import argparse
import os
from uuid import UUID

from google.cloud import storage

from etl.common.bq_writer import BQWriter
from etl.common.gcs_reader import GCSReader
from etl.xero.run_history import manifest_rows, target_dataset, write_manifest_rows


def backfill_manifest_history(
    storage_client, reader, writer_factory, *, bucket: str, default_project: str,
    prefix: str = "_manifests/xero/",
) -> dict[str, int]:
    """Idempotently MERGE each manifest, keeping sensitive runs restricted."""
    if not prefix.endswith("_manifests/xero/"):
        raise ValueError("manifest prefix must end with _manifests/xero/")
    writers = {}
    manifests = rows = 0
    for blob in storage_client.list_blobs(bucket, prefix=prefix):
        if not blob.name.endswith(".json.meta.json"):
            continue
        if not blob.name.startswith(prefix):
            raise ValueError("storage returned a manifest outside the requested prefix")
        logical_path = "_manifests/xero/" + blob.name.removeprefix(prefix)
        manifest = reader._read_data(blob.name.removesuffix(".meta.json"))
        run, outcomes = manifest_rows(logical_path, manifest)
        project, dataset = target_dataset(
            bucket, [row["endpoint"] for row in outcomes], default_project,
        )
        target = (project, dataset)
        if target not in writers:
            writers[target] = writer_factory(project, dataset)
        rows += write_manifest_rows(writers[target], run, outcomes)
        manifests += 1
    return {"manifests": manifests, "rows": rows}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bucket", default=os.environ.get("GCS_BUCKET"))
    parser.add_argument("--project", default=os.environ.get("GCP_PROJECT"))
    parser.add_argument("--generation", help="Replay one isolated ingestion generation")
    parser.add_argument("--namespace", choices=("xero", "v2"), default="xero")
    args = parser.parse_args()
    if not args.bucket or not args.project:
        parser.error("--bucket and --project (or GCS_BUCKET and GCP_PROJECT) are required")
    prefix = "_manifests/xero/"
    if args.generation:
        try:
            generation = str(UUID(args.generation))
        except ValueError:
            parser.error("--generation must be a canonical UUID")
        if generation != args.generation:
            parser.error("--generation must be a canonical UUID")
        prefix = f"{args.namespace}/{generation}/_manifests/xero/"
    result = backfill_manifest_history(
        storage.Client(project=args.project),
        GCSReader(bucket=args.bucket, project=args.project),
        lambda project, dataset: BQWriter(project=project, dataset=dataset),
        bucket=args.bucket, default_project=args.project, prefix=prefix,
    )
    print(f"Replayed {result['manifests']} manifests into {result['rows']} history rows")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
