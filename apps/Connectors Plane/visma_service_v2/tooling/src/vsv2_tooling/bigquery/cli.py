"""CLI entry points for BigQuery operations called from the Rust orchestrator.

vsv2-bq-stage       -- MERGE JSONL records into an endpoint's raw staging table (JSON payload).
                       After a successful write it appends one heartbeat row (see heartbeat.py).
vsv2-bq-merge       -- MERGE JSONL records into a live bronze table.
vsv2-bq-shadow      -- Append JSONL records to a _dm_shadow table.
vsv2-bq-reconcile   -- Soft-delete BQ rows whose PK is absent from ERP.

All commands print a single JSON line on success and exit non-zero on error.
"""

from __future__ import annotations

import json
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Annotated, Any

import typer

from vsv2_tooling.bigquery.heartbeat import record_heartbeat
from vsv2_tooling.bigquery.loader import merge, write_quarantine, write_shadow
from vsv2_tooling.bigquery.reconcile import load_erp_ids, reconcile
from vsv2_tooling.bigquery.staging import stage

stage_app = typer.Typer(add_completion=False)
merge_app = typer.Typer(add_completion=False)
shadow_app = typer.Typer(add_completion=False)
reconcile_app = typer.Typer(add_completion=False)
quarantine_app = typer.Typer(add_completion=False)


def _iter_records(input_path: Path) -> Iterator[dict[str, Any]]:
    """JSONL records one at a time, so a large file is never held as Python objects."""
    if str(input_path) == "-":
        for line in sys.stdin:
            if line.strip():
                yield json.loads(line)
        return
    with input_path.open(encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                yield json.loads(line)


def _read_records(input_path: Path) -> list[dict[str, Any]]:
    """Every record of a JSONL file, split into lines the way `_iter_records` splits them.

    str.splitlines would also break a line at U+2028, U+2029 and U+0085, which serde_json writes
    unescaped.
    """
    return list(_iter_records(input_path))


@stage_app.command()
def run_stage(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset: Annotated[str, typer.Option(help="Staging dataset")],
    table: Annotated[str, typer.Option(help="Staging table (the endpoint name)")],
    pk: Annotated[str, typer.Option(help="Key fields, comma-separated, tenant_id first")],
    input: Annotated[Path, typer.Option(help="JSONL file path, or - for stdin")],
    run_id: Annotated[str, typer.Option("--run-id", help="Sync run id")],
    endpoint: Annotated[str, typer.Option("--endpoint", help="Endpoint name")],
    source: Annotated[str, typer.Option("--source", help="Source name")],
    tenant: Annotated[
        str | None, typer.Option("--tenant", help="Tenant the batch belongs to")
    ] = None,
    complete: Annotated[
        bool,
        typer.Option(
            "--complete",
            help="The batch is everything Visma has for the tenant: mark the rest deleted",
        ),
    ] = False,
    force_delete: Annotated[
        bool,
        typer.Option("--force-delete", help="Mark rows deleted even when it is more than a fifth"),
    ] = False,
) -> None:
    """Write records to the endpoint's staging table; creates the table when missing."""
    result = stage(
        project=project,
        dataset=dataset,
        table=table,
        primary_key=pk,
        records=_iter_records(input),
        run_id=run_id,
        endpoint=endpoint,
        source=source,
        tenant=tenant,
        complete=complete,
        force_delete=force_delete,
    )
    # Reached only after a successful write: stage() raising skips the heartbeat. An empty batch is
    # still a sync, so it writes one too (rows_written 0). A heartbeat failure never fails the sync.
    if tenant:
        record_heartbeat(
            project=project,
            dataset=dataset,
            tenant_id=tenant,
            endpoint=endpoint,
            rows_written=result["rows"],
        )
    typer.echo(json.dumps(result))


@merge_app.command()
def run_merge(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset: Annotated[str, typer.Option(help="Target BigQuery dataset")],
    table: Annotated[str, typer.Option(help="Target table name")],
    pk: Annotated[
        str, typer.Option(help="Primary key column name (comma-separated for composite)")
    ],
    input: Annotated[Path, typer.Option(help="JSONL file path, or - for stdin")],
    normalizer: Annotated[
        str | None, typer.Option(help="Normalizer name from REGISTRY (DM endpoints)")
    ] = None,
    run_id: Annotated[
        str | None, typer.Option("--run-id", help="Sync run id for raw staging")
    ] = None,
    endpoint: Annotated[
        str | None, typer.Option("--endpoint", help="Endpoint name for raw staging")
    ] = None,
    source: Annotated[
        str | None, typer.Option("--source", help="Source name for raw staging")
    ] = None,
) -> None:
    """MERGE records from JSONL into the target bronze table."""
    records = _read_records(input)
    n = merge(
        project=project,
        dataset=dataset,
        table=table,
        primary_key=pk,
        records=records,
        normalizer_name=normalizer,
        run_id=run_id,
        endpoint=endpoint,
        source=source,
    )
    typer.echo(json.dumps({"rows": n}))


@shadow_app.command()
def run_shadow(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset_shadow: Annotated[
        str, typer.Option("--dataset-shadow", help="Shadow BigQuery dataset")
    ],
    table: Annotated[str, typer.Option(help="Base table name (suffix _dm_shadow is added)")],
    input: Annotated[Path, typer.Option(help="JSONL file path, or - for stdin")],
    normalizer: Annotated[
        str | None, typer.Option(help="Normalizer name from REGISTRY (DM endpoints)")
    ] = None,
) -> None:
    """Append DataMart records to the shadow table for pre-cutover diffing."""
    records = _read_records(input)
    n = write_shadow(
        project=project,
        dataset_shadow=dataset_shadow,
        table=table,
        records=records,
        normalizer_name=normalizer,
    )
    typer.echo(json.dumps({"rows": n}))


@quarantine_app.command()
def run_quarantine(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset: Annotated[str, typer.Option(help="Target BigQuery dataset")],
    input: Annotated[Path, typer.Option(help="JSONL file path, or - for stdin")],
) -> None:
    """Append rejected records to the quarantine table."""
    records = _read_records(input)
    n = write_quarantine(project=project, dataset=dataset, records=records)
    typer.echo(json.dumps({"rows": n}))


@reconcile_app.command()
def run_reconcile(
    project: Annotated[str, typer.Option(help="GCP project id")],
    dataset: Annotated[str, typer.Option(help="Target BigQuery dataset")],
    table: Annotated[str, typer.Option(help="Target table name")],
    pk: Annotated[str, typer.Option(help="Primary key column name")],
    erp_ids_file: Annotated[
        Path, typer.Option("--erp-ids-file", help="JSONL file: one JSON value per line")
    ],
    tenant_id: Annotated[
        str | None, typer.Option("--tenant-id", help="Tenant id to scope soft deletes")
    ] = None,
) -> None:
    """Soft-delete BQ rows whose PK is absent from the ERP ID file."""
    erp_ids = load_erp_ids(erp_ids_file)
    n = reconcile(
        project=project,
        dataset=dataset,
        table=table,
        pk=pk,
        erp_ids=erp_ids,
        tenant_id=tenant_id,
    )
    typer.echo(json.dumps({"deleted": n}))
