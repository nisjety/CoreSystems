"""Shadow-mode reconciliation — compare `<table>_dm_shadow` vs live bronze.

Tolerances:
  PK fields:  zero diff allowed
  Numerics:   ≤ 1% mismatch rate
  Strings:    ≤ 0.1% mismatch rate (essentially zero)

Fail-flip rule: 7 consecutive clean days → flip
  `config/endpoints/<name>.json:datamart.tenant_overrides[<tenant>] = true`
"""

from __future__ import annotations

import sys
from dataclasses import dataclass

import typer
from google.cloud import bigquery
from rich.console import Console
from rich.table import Table

cli = typer.Typer(no_args_is_help=True, add_completion=False)
console = Console()

_NUMERIC_TYPES = {"INTEGER", "INT64", "FLOAT", "FLOAT64", "NUMERIC", "BIGNUMERIC"}
_STRING_THRESHOLD = 0.001  # 0.1%
_NUMERIC_THRESHOLD = 0.01  # 1.0%


@dataclass
class FieldDiff:
    field: str
    field_type: str
    total: int
    mismatches: int

    @property
    def mismatch_pct(self) -> float:
        return (self.mismatches / self.total * 100) if self.total else 0.0

    @property
    def threshold(self) -> float:
        return _NUMERIC_THRESHOLD if self.field_type in _NUMERIC_TYPES else _STRING_THRESHOLD

    @property
    def ok(self) -> bool:
        rate = self.mismatches / self.total if self.total else 0.0
        return rate <= self.threshold


@cli.command()
def diff(
    table: str = typer.Argument(..., help="Bronze table name, e.g. visma_customers"),
    project: str = typer.Option(..., help="GCP project id"),
    dataset_live: str = typer.Option(..., help="Live bronze dataset"),
    dataset_shadow: str = typer.Option(..., help="DM shadow dataset"),
    pk: str = typer.Option(..., help="Primary key column"),
) -> None:
    """Run today's shadow vs live diff and print per-field mismatch counts."""
    client = bigquery.Client(project=project)

    live_ref = f"`{project}.{dataset_live}.{table}`"
    shadow_ref = f"`{project}.{dataset_shadow}.{table}_dm_shadow`"

    live_meta = client.get_table(f"{project}.{dataset_live}.{table}")
    fields = [(f.name, f.field_type) for f in live_meta.schema if f.name != pk]

    col_counts = ",\n    ".join(
        f"COUNTIF(live.`{name}` IS DISTINCT FROM shadow.`{name}`) AS mismatch_{name}"
        for name, _ in fields
    )

    sql = f"""
        SELECT
            COUNT(*) AS total,
            COUNTIF(shadow.`{pk}` IS NULL) AS pk_only_in_live,
            COUNTIF(live.`{pk}` IS NULL)   AS pk_only_in_shadow,
            {col_counts}
        FROM {live_ref}  AS live
        FULL OUTER JOIN {shadow_ref} AS shadow
            USING (`{pk}`)
    """

    rows = list(client.query(sql).result())
    if not rows:
        console.print("[yellow]No rows returned from diff query.[/yellow]")
        raise typer.Exit(1)

    row = rows[0]
    total = row["total"] or 0
    pk_only_in_live = row["pk_only_in_live"] or 0
    pk_only_in_shadow = row["pk_only_in_shadow"] or 0

    results = [
        FieldDiff(
            field=name,
            field_type=ftype,
            total=total,
            mismatches=row[f"mismatch_{name}"] or 0,
        )
        for name, ftype in fields
    ]

    t = Table(title=f"Shadow diff: {table}  ({total:,} rows compared)")
    t.add_column("field")
    t.add_column("type")
    t.add_column("mismatches", justify="right")
    t.add_column("mismatch %", justify="right")
    t.add_column("ok", justify="center")
    for fd in sorted(results, key=lambda x: -x.mismatches):
        t.add_row(
            fd.field,
            fd.field_type,
            f"{fd.mismatches:,}",
            f"{fd.mismatch_pct:.4f}%",
            ":white_check_mark:" if fd.ok else ":x:",
        )
    console.print(t)

    if pk_only_in_live or pk_only_in_shadow:
        console.print(
            f"[red]PK gap — only-in-live: {pk_only_in_live:,}, "
            f"only-in-shadow: {pk_only_in_shadow:,}[/red]"
        )

    bad = [fd for fd in results if not fd.ok]
    if bad or pk_only_in_live or pk_only_in_shadow:
        console.print(f"[red]{len(bad)} field(s) exceed tolerance. Not clean.[/red]")
        sys.exit(1)
    else:
        console.print("[green]All fields within tolerance. Clean run.[/green]")


if __name__ == "__main__":
    cli()
