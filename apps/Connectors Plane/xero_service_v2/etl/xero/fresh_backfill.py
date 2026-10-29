"""Replay admitted main-generation Xero raw observations and run manifests."""

import argparse
import os
from collections.abc import Iterable
from uuid import UUID

from google.cloud import bigquery, storage

from etl.cloud_function.main import (
    _publish_current_projections as publish_current_projections,
    current_projection_from_records,
)
from etl.common.fresh_writer import WarehouseRawBQWriter
from etl.xero.fresh_ingestion import FreshConfig, prepare_fresh_observation


MAX_BATCH_SIZE = 250


def canonical_run_id(value: str | None) -> str | None:
    """Accept only the canonical UUID spelling used in immutable run manifests."""
    if value is None:
        return None
    try:
        canonical = str(UUID(value))
    except (AttributeError, TypeError, ValueError) as exc:
        raise ValueError("run ID must be a canonical run ID UUID") from exc
    if canonical != value:
        raise ValueError("run ID must be a canonical run ID UUID")
    return canonical


def committed_sidecars(writer, project: str, run_id: str) -> frozenset[tuple[str, str]]:
    """Return exact pinned raw receipts already committed for one source run."""
    if project != "aquatiq-datavarehus":
        raise ValueError("receipt skip is limited to the active main Xero project")
    run_id = canonical_run_id(run_id)
    query = """
        SELECT source_object, meta_generation
        FROM `aquatiq-datavarehus.dw_staging_xero.ingest_receipts`
        WHERE run_id = @run_id AND endpoint != 'sync_manifest'
    """
    config = bigquery.QueryJobConfig(query_parameters=[
        bigquery.ScalarQueryParameter("run_id", "STRING", run_id),
    ])
    result = set()
    for row in writer.client.query(query, job_config=config).result():
        object_name, generation = row["source_object"], row["meta_generation"]
        if not isinstance(object_name, str) or not isinstance(generation, str) or not generation.isdecimal():
            raise ValueError("committed raw receipt lacks a pinned sidecar identity")
        result.add((object_name, generation))
    return frozenset(result)


def prepared_run_id(prepared) -> str:
    """Read the one source run carried by a prepared raw or manifest receipt."""
    receipts = prepared.batches.get("v2_ingest_receipts", [])
    run_ids = {receipt.get("run_id") for receipt in receipts}
    if len(receipts) != 1 or len(run_ids) != 1 or None in run_ids:
        raise ValueError("prepared observation must contain exactly one run receipt")
    return next(iter(run_ids))


def raw_sidecars(storage_client, bucket: str, prefix: str) -> Iterable[tuple[str, str]]:
    """Yield only pinned raw-response sidecars below one generation prefix."""
    for blob in storage_client.list_blobs(bucket, prefix=prefix):
        if not isinstance(blob.name, str) or not blob.name.startswith(prefix):
            raise ValueError("storage returned an object outside the requested raw prefix")
        if not blob.name.endswith(".meta.json"):
            continue
        generation = str(blob.generation or "")
        if not generation.isdecimal() or generation == "0":
            raise ValueError("raw sidecar is missing an immutable object generation")
        yield blob.name, generation


def manifest_sidecars(storage_client, bucket: str, prefix: str) -> Iterable[tuple[str, str]]:
    """Yield only pinned run-manifest sidecars below one generation prefix."""
    for blob in storage_client.list_blobs(bucket, prefix=prefix):
        if not isinstance(blob.name, str) or not blob.name.startswith(prefix):
            raise ValueError("storage returned an object outside the requested manifest prefix")
        if not blob.name.endswith(".meta.json"):
            continue
        generation = str(blob.generation or "")
        if not generation.isdecimal() or generation == "0":
            raise ValueError("manifest sidecar is missing an immutable object generation")
        yield blob.name, generation


def backfill_raw_observations(
    storage_client,
    writer,
    config,
    *,
    bucket: str,
    project: str,
    prefix: str,
    batch_size: int,
    run_id: str | None = None,
    received_sidecars: frozenset[tuple[str, str]] = frozenset(),
) -> dict[str, int]:
    """Replay verified raw response sidecars through bounded idempotent batches."""
    if not 1 <= batch_size <= MAX_BATCH_SIZE:
        raise ValueError(f"batch size must be between 1 and {MAX_BATCH_SIZE}")
    if not prefix.endswith("/raw/xero/"):
        raise ValueError("backfill prefix must target raw Xero response objects")

    selected_run_id = canonical_run_id(run_id)
    run_tag = selected_run_id.replace("-", "")[:6] if selected_run_id else None
    pending = []
    objects = batches = 0
    for object_name, meta_generation in raw_sidecars(storage_client, bucket, prefix):
        # The raw path contains the stable first six hexadecimal run characters.
        # It avoids opening known-unrelated legacy sidecars; the receipt check below
        # protects against the unlikely truncated-prefix collision.
        if run_tag and f"_{run_tag}_" not in object_name:
            continue
        if (object_name, meta_generation) in received_sidecars:
            continue
        prepared = prepare_fresh_observation(
            config, bucket, object_name, meta_generation, project, storage_client,
        )
        if prepared is None:
            continue
        if selected_run_id and prepared_run_id(prepared) != selected_run_id:
            continue
        if (prepared.is_manifest or prepared.layout != "warehouse"
                or prepared.target_project != project or prepared.dataset != "dw_staging_xero"):
            raise ValueError("backfill supports only the main warehouse raw layout")
        pending.append(prepared.batches)
        objects += 1
        if len(pending) == batch_size:
            writer.publish_observations_many(pending)
            pending = []
            batches += 1
    if pending:
        writer.publish_observations_many(pending)
        batches += 1
    return {"objects": objects, "batches": batches}


def backfill_manifests(
    storage_client,
    writer,
    config,
    *,
    bucket: str,
    project: str,
    prefix: str,
    run_id: str | None = None,
) -> dict[str, int]:
    """Replay verified run manifests with their own idempotent receipt fences."""
    if not prefix.endswith("/_manifests/xero/"):
        raise ValueError("backfill prefix must target Xero run manifests")

    selected_run_id = canonical_run_id(run_id)
    objects = 0
    for object_name, meta_generation in manifest_sidecars(storage_client, bucket, prefix):
        if selected_run_id and selected_run_id not in object_name:
            continue
        prepared = prepare_fresh_observation(
            config, bucket, object_name, meta_generation, project, storage_client,
        )
        if prepared is None:
            continue
        if selected_run_id and prepared_run_id(prepared) != selected_run_id:
            continue
        if (not prepared.is_manifest or prepared.layout != "warehouse"
                or prepared.target_project != project or prepared.dataset != "dw_staging_xero"):
            raise ValueError("manifest replay supports only the main warehouse raw layout")
        writer.publish_observation(prepared.batches)
        objects += 1
    return {"objects": objects}


def replay_current_projections(
    storage_client,
    config,
    *,
    bucket: str,
    project: str,
    prefix: str,
    batch_size: int,
    run_id: str | None = None,
) -> dict[str, int]:
    """Rebuild typed current tables from pinned raw evidence without calling Xero."""
    if not 1 <= batch_size <= MAX_BATCH_SIZE:
        raise ValueError(f"batch size must be between 1 and {MAX_BATCH_SIZE}")
    if not prefix.endswith("/raw/xero/"):
        raise ValueError("projection replay prefix must target raw Xero response objects")

    selected_run_id = canonical_run_id(run_id)
    run_tag = selected_run_id.replace("-", "")[:6] if selected_run_id else None
    pending: dict[tuple, list] = {}
    objects = projection_batches = 0

    def publish_pending(key):
        nonlocal projection_batches
        entries = tuple(pending.pop(key, ()))
        if entries:
            publish_current_projections(entries)
            projection_batches += 1

    generation_prefix = prefix.removesuffix("raw/xero/")
    for object_name, meta_generation in raw_sidecars(storage_client, bucket, prefix):
        if run_tag and f"_{run_tag}_" not in object_name:
            continue
        prepared = prepare_fresh_observation(
            config, bucket, object_name, meta_generation, project, storage_client,
        )
        if prepared is None:
            continue
        if selected_run_id and prepared_run_id(prepared) != selected_run_id:
            continue
        if (prepared.is_manifest or prepared.layout != "warehouse"
                or prepared.target_project != project or prepared.dataset != "dw_staging_xero"):
            raise ValueError("projection replay supports only the main warehouse raw layout")
        projection = current_projection_from_records(
            bucket, object_name, object_name.removeprefix(generation_prefix), prepared.records,
        )
        objects += 1
        if projection is None:
            continue
        queue = pending.setdefault(projection.batch_key, [])
        queue.append(projection)
        if len(queue) >= batch_size:
            publish_pending(projection.batch_key)
    for key in tuple(pending):
        publish_pending(key)
    return {"objects": objects, "projection_batches": projection_batches}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generation", required=True, help="Active canonical Xero ingestion generation")
    parser.add_argument("--batch-size", type=int, default=100)
    parser.add_argument(
        "--skip-received", action="store_true",
        help="Skip exact sidecar generations already committed for --run-id",
    )
    parser.add_argument(
        "--run-id",
        help="Replay only one exact canonical source run; useful when the generation retains legacy objects",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--manifests-only",
        action="store_true",
        help="Replay only run manifests; raw response history is not rescanned",
    )
    mode.add_argument(
        "--current-projections",
        action="store_true",
        help="Rebuild typed current staging tables from pinned raw observations",
    )
    args = parser.parse_args()
    try:
        run_id = canonical_run_id(args.run_id)
    except ValueError as exc:
        parser.error(str(exc))
    if args.skip_received and (run_id is None or args.manifests_only or args.current_projections):
        parser.error("--skip-received requires --run-id in raw replay mode")

    config = FreshConfig.from_env()
    project = os.environ.get("GCP_PROJECT")
    bucket = os.environ.get("GCS_BUCKET")
    layout = os.environ.get("XERO_STAGING_LAYOUT")
    if (config is None or config.namespace != "main" or config.generation != args.generation
            or project != "aquatiq-datavarehus"
            or bucket != "aquatiq-datavarehus-xero-raw"
            or os.environ.get("XERO_STAGING_DATASET") != "dw_staging_xero"
            or layout != "warehouse"):
        parser.error("requires the active main Xero warehouse raw-history configuration")

    writer = WarehouseRawBQWriter(project, "dw_staging_xero")
    writer.bootstrap()
    storage_client = storage.Client(project=project)
    if args.manifests_only:
        result = backfill_manifests(
            storage_client, writer, config,
            bucket=bucket,
            project=project,
            prefix=f"xero/{config.generation}/_manifests/xero/",
            run_id=run_id,
        )
        print(f"Replayed {result['objects']} pinned run manifest(s)")
    elif args.current_projections:
        result = replay_current_projections(
            storage_client, config,
            bucket=bucket,
            project=project,
            prefix=f"xero/{config.generation}/raw/xero/",
            batch_size=args.batch_size,
            run_id=run_id,
        )
        print(
            f"Replayed {result['objects']} pinned raw object(s) into "
            f"{result['projection_batches']} typed projection batch(es)"
        )
    else:
        received = committed_sidecars(writer, project, run_id) if args.skip_received else frozenset()
        result = backfill_raw_observations(
            storage_client, writer, config,
            bucket=bucket,
            project=project,
            prefix=f"xero/{config.generation}/raw/xero/",
            batch_size=args.batch_size,
            run_id=run_id,
            received_sidecars=received,
        )
        print(f"Replayed {result['objects']} pinned raw object(s) in {result['batches']} batch(es)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
