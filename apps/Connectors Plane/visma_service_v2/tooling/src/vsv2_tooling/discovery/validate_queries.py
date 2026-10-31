"""Validate generated DataMart GraphQL queries against live introspection."""

from __future__ import annotations

import json
from pathlib import Path

from graphql import DirectiveLocation, build_client_schema, parse, validate
from rich import print as rprint

from .generate_queries import QueryField, _camel_to_snake, _load_catalog

_RUNTIME_ARGS = {
    "companyId",
    "includeDeleted",
    "lastId",
    "pageSize",
    "toggleBalanceSign",
    "updatedInDataMart",
    "updatedSince",
}


def run(*, introspection_path: str, query_dir: str, report_path: str) -> None:
    query_fields, _ = _load_catalog(Path(introspection_path))
    schema = _build_schema(Path(introspection_path))
    query_by_file = {_camel_to_snake(field.root_name): field for field in query_fields}

    rows: list[dict[str, object]] = []
    query_dir_path = Path(query_dir)
    for path in sorted(query_dir_path.glob("*.graphql")):
        field = query_by_file.get(path.stem)
        rows.append(_validate_one(schema, path, field))

    report = Path(report_path)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(_render_report(rows, query_dir_path))

    valid = sum(1 for row in rows if row["schema_valid"])
    rprint(f"[green]✓[/green] validated {valid}/{len(rows)} GraphQL query file(s) → {report}")
    if valid != len(rows):
        raise SystemExit(
            f"DataMart query validation failed: {valid}/{len(rows)} valid; see {report}"
        )


def _build_schema(introspection_path: Path):
    raw = json.loads(introspection_path.read_text())
    schema_payload = raw.get("__schema") or raw.get("data", {}).get("__schema")
    if not isinstance(schema_payload, dict):
        raise SystemExit(f"invalid introspection payload: {introspection_path}")
    # Visma's live schema currently exposes nonstandard helper types prefixed
    # with `__` (for example `__AppliedDirective`). `graphql-core` rejects them
    # during schema validation, even though query validation itself still works
    # when the schema is accepted as-is.
    known_locations = {location.name for location in DirectiveLocation}
    compatible_schema = {
        **schema_payload,
        "directives": [
            {
                **directive,
                "locations": [
                    location
                    for location in directive.get("locations", [])
                    if location in known_locations
                ],
            }
            for directive in schema_payload.get("directives", [])
        ],
    }
    return build_client_schema({"__schema": compatible_schema}, assume_valid=True)


def _validate_one(schema, path: Path, field: QueryField | None) -> dict[str, object]:
    source = path.read_text()
    try:
        document = parse(source)
        errors = [str(err) for err in validate(schema, document)]
    except Exception as exc:  # pragma: no cover - parse failures are still reported
        errors = [str(exc)]

    required_extra_args: list[str] = []
    optional_extra_args: list[str] = []
    compatibility = "unknown"
    root_name = field.root_name if field else path.stem

    if field is not None:
        required_extra_args = [
            arg["name"]
            for arg in field.args
            if _is_non_null(arg.get("type"))
            and arg.get("defaultValue") is None
            and arg["name"] not in _RUNTIME_ARGS
        ]
        optional_extra_args = [
            arg["name"]
            for arg in field.args
            if (not _is_non_null(arg.get("type")) or arg.get("defaultValue") is not None)
            and arg["name"] not in _RUNTIME_ARGS
        ]
        compatibility = _classify_compatibility(field, required_extra_args)

    return {
        "file": path.name,
        "root_name": root_name,
        "schema_valid": not errors,
        "compatibility": compatibility,
        "required_extra_args": required_extra_args,
        "optional_extra_args": optional_extra_args,
        "errors": errors,
    }


def _classify_compatibility(field: QueryField, required_extra_args: list[str]) -> str:
    if not field.is_list:
        if field.type_name.endswith("Page"):
            return "requires_page_adapter"
        return "point_lookup"
    if required_extra_args:
        return "requires_runtime_params"
    return "paginated_compatible"


def _is_non_null(ref: dict | None) -> bool:
    current = ref or {}
    return current.get("kind") == "NON_NULL"


def _render_report(rows: list[dict[str, object]], query_dir: Path) -> str:
    schema_valid = sum(1 for row in rows if row["schema_valid"])
    paginated = sum(1 for row in rows if row["compatibility"] == "paginated_compatible")
    point_lookup = sum(1 for row in rows if row["compatibility"] == "point_lookup")
    page_adapter = sum(1 for row in rows if row["compatibility"] == "requires_page_adapter")
    runtime_params = sum(1 for row in rows if row["compatibility"] == "requires_runtime_params")

    lines = [
        "# DataMart Query Validation",
        "",
        f"- query directory: `{query_dir}`",
        f"- total query files: {len(rows)}",
        f"- schema-valid files: {schema_valid}",
        f"- paginated-compatible with current runtime: {paginated}",
        f"- point-lookup only: {point_lookup}",
        f"- require page-wrapper adapter: {page_adapter}",
        f"- require extra runtime params: {runtime_params}",
        "",
        "| file | root | schema_valid | compatibility | "
        "required_extra_args | optional_extra_args |",
        "|---|---|---:|---|---|---|",
    ]
    for row in rows:
        required = ", ".join(row["required_extra_args"]) or "-"
        optional = ", ".join(row["optional_extra_args"]) or "-"
        lines.append(
            f"| `{row['file']}` | `{row['root_name']}` | "
            f"{'yes' if row['schema_valid'] else 'no'} | `{row['compatibility']}` | "
            f"`{required}` | `{optional}` |"
        )

    invalid = [row for row in rows if not row["schema_valid"]]
    if invalid:
        lines.extend(["", "## Schema Errors", ""])
        for row in invalid:
            lines.append(f"### `{row['file']}`")
            lines.extend(f"- {error}" for error in row["errors"])

    lines.extend(
        [
            "",
            "## Notes",
            "",
            "- `paginated_compatible` means the current adapter can execute the query with "
            "`pageSize`, `lastId`, optional watermark, and optional `companyId` only.",
            "- `requires_runtime_params` means the schema requires extra non-null arguments "
            "before the query can be used for scheduled ingestion.",
            "- `point_lookup` queries are valid, but they are not bulk-ingestion queries.",
            "- `requires_page_adapter` queries return page objects with `items`, "
            "not a list at the root.",
        ]
    )
    return "\n".join(lines) + "\n"
