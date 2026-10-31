"""`vsv2-discover` — Phase 0 entry point."""

from __future__ import annotations

import typer

from . import (
    coverage_matrix,
    generate_queries,
    introspect,
    probe,
    review_candidates,
    shadow_compare,
    validate_queries,
)

app = typer.Typer(no_args_is_help=True, help="Phase 0 discovery & gating commands.")


@app.command("introspect")
def cmd_introspect(
    out: str = typer.Option(
        "core/queries/_generated/datamart.sdl",
        help="Where to write the GraphQL SDL.",
    ),
) -> None:
    """Run GraphQL introspection against DataMart and dump SDL + JSON."""
    introspect.run(out_path=out)


@app.command("companies")
def cmd_companies() -> None:
    """List companies visible to the configured Visma Connect token.

    Use the result to fill `tenant_company_map` for Aquatiq AS.
    """
    introspect.list_companies()


@app.command("coverage")
def cmd_coverage(
    bronze_dir: str = typer.Option(
        "../Dataform/definitions/bronze/visma",
        help="Path to v1 Dataform bronze SQLX files.",
    ),
    out: str = typer.Option("docs/phase0_coverage.md"),
) -> None:
    """Build bronze ↔ DataMart coverage matrix → Markdown."""
    coverage_matrix.run(bronze_dir=bronze_dir, out_path=out)


@app.command("probe")
def cmd_probe(
    pages: int = typer.Option(10, help="Pages per probe."),
    out: str = typer.Option("docs/phase0_probe.md"),
) -> None:
    """Run rate-limit / pagination probe against `accounts` and `customers`."""
    probe.run(pages=pages, out_path=out)


@app.command("generate-queries")
def cmd_generate_queries(
    out_dir: str = typer.Option(
        "config/queries/datamart",
        help="Directory where generated GraphQL query files are written.",
    ),
    report: str = typer.Option(
        "docs/layeSystem/Layer1/datamart_graphql_catalog.md",
        help="Markdown report describing live DataMart query roots and endpoint mappings.",
    ),
    introspection: str = typer.Option(
        "core/queries/_generated/datamart.introspection.json",
        help="Path to the live DataMart introspection snapshot.",
    ),
    endpoint_dir: str = typer.Option(
        "config/endpoints",
        help="Endpoint config directory used for compatibility mapping.",
    ),
    sync_configs: bool = typer.Option(
        True,
        "--sync-configs/--no-sync-configs",
        help=(
            "Fill safe endpoint config root_field/type_name values and create missing query files."
        ),
    ),
) -> None:
    """Generate a DataMart GraphQL query catalog from live introspection."""
    generate_queries.run(
        out_dir=out_dir,
        report_path=report,
        introspection_path=introspection,
        endpoint_dir=endpoint_dir,
        sync_configs=sync_configs,
    )


@app.command("validate-queries")
def cmd_validate_queries(
    introspection: str = typer.Option(
        "core/queries/_generated/datamart.introspection.json",
        help="Path to the live DataMart introspection snapshot.",
    ),
    query_dir: str = typer.Option(
        "config/queries/datamart",
        help="Directory with generated GraphQL query files.",
    ),
    report: str = typer.Option(
        "docs/layeSystem/Layer1/datamart_query_validation.md",
        help="Markdown report summarizing query validity and runtime compatibility.",
    ),
) -> None:
    """Validate generated DataMart queries against the live introspection schema."""
    validate_queries.run(
        introspection_path=introspection,
        query_dir=query_dir,
        report_path=report,
    )


@app.command("review-candidates")
def cmd_review_candidates(
    endpoint_dir: str = typer.Option(
        "config/endpoints",
        help="Endpoint config directory used for the synchronized candidate set.",
    ),
    bronze_dir: str = typer.Option(
        "../Dataform/definitions/bronze/visma",
        help="Dataform bronze declaration directory.",
    ),
    report: str = typer.Option(
        "docs/layeSystem/Layer1/datamart_candidate_parity_review.md",
        help="Markdown report for synchronized Layer 1 candidate review.",
    ),
) -> None:
    """Review synchronized DataMart mappings against bronze schemas and normalizers."""
    review_candidates.run(
        endpoint_dir=endpoint_dir,
        bronze_dir=bronze_dir,
        report_path=report,
    )


@app.command("shadow-compare")
def cmd_shadow_compare(
    endpoint_dir: str = typer.Option(
        "config/endpoints",
        help="Endpoint config directory used for the synchronized candidate set.",
    ),
    bronze_dir: str = typer.Option(
        "../Dataform/definitions/bronze/visma",
        help="Dataform bronze declaration directory.",
    ),
    review_report: str = typer.Option(
        "docs/layeSystem/Layer1/datamart_candidate_parity_review.md",
        help="Candidate review report used to select candidate_for_shadow endpoints.",
    ),
    out: str = typer.Option(
        "docs/layeSystem/Layer1/datamart_shadow_compare.md",
        help="Markdown report for live ERP vs DataMart sample comparison.",
    ),
    page_size: int = typer.Option(50, help="Rows per source page."),
    max_pages: int = typer.Option(2, help="Maximum pages per source."),
) -> None:
    """Compare live ERP REST samples against normalized DataMart samples."""
    shadow_compare.run(
        endpoint_dir=endpoint_dir,
        bronze_dir=bronze_dir,
        review_report_path=review_report,
        out_path=out,
        page_size=page_size,
        max_pages=max_pages,
    )


if __name__ == "__main__":
    app()
