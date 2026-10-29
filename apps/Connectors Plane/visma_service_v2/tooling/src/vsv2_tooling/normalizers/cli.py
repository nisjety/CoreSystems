"""Standalone normalizer CLI — vsv2-normalize.

Reads JSONL from --input (or stdin), applies a named normalizer from REGISTRY,
writes normalized JSONL to --output (or stdout). Useful for ad-hoc inspection
and pipeline testing independent of the BQ loader.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Annotated

import typer

from vsv2_tooling.normalizers import REGISTRY

app = typer.Typer(add_completion=False, no_args_is_help=True)


@app.command()
def normalize(
    normalizer: Annotated[str, typer.Option(help="Normalizer name from REGISTRY")],
    input: Annotated[Path | None, typer.Option(help="JSONL input file, or omit for stdin")] = None,
    output: Annotated[
        Path | None,
        typer.Option(help="JSONL output file, or omit for stdout"),
    ] = None,
) -> None:
    """Apply a named normalizer to every record in a JSONL stream."""
    fn = REGISTRY.get(normalizer)
    if fn is None:
        typer.echo(
            f"Unknown normalizer: {normalizer!r}. Available: {sorted(REGISTRY)}",
            err=True,
        )
        raise typer.Exit(1)

    lines = input.read_text().splitlines() if input else sys.stdin.readlines()
    records = [json.loads(line) for line in lines if line.strip()]
    normalized = [fn(r) for r in records]

    out_lines = "\n".join(json.dumps(r) for r in normalized)
    if output:
        output.write_text(out_lines + "\n")
    else:
        sys.stdout.write(out_lines + "\n")

    typer.echo(f"normalized {len(normalized)} record(s)", err=True)
