"""Generate a DataMart query catalog from live introspection."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

from rich import print as rprint

_IDENTITY_FIELD_CANDIDATES = ("id", "companyId", "updatedInDataMartDateTime")

_SAFE_ENDPOINT_ALIASES: dict[str, str] = {
    "accounts": "accounts",
    "branches": "branches",
    "budgets": "budgets",
    "contacts": "contacts",
    "credit_terms": "terms",
    "customer_documents": "customerDocuments",
    "customer_payments": "customerPayments",
    "customer_sales_prices": "salesPrices",
    "customers": "customers",
    "general_ledger_balances_v2": "generalLedgerBalancesV2",
    "journal_transactions": "journalTransactions",
    "locations": "locations",
    "payment_methods": "paymentMethods",
    "sales_orders_v3": "salesOrders",
    "sales_persons": "salesPersons",
    "subaccounts": "subaccounts",
    "supplier_classes": "supplierClasses",
    "supplier_documents": "supplierDocuments",
    "supplier_invoices": "apInvoices",
}


@dataclass(frozen=True)
class QueryField:
    root_name: str
    type_name: str
    is_list: bool
    args: tuple[dict, ...]


def run(
    *,
    out_dir: str,
    report_path: str,
    introspection_path: str,
    endpoint_dir: str,
    sync_configs: bool,
) -> None:
    query_fields, types_by_name = _load_catalog(Path(introspection_path))

    query_out_dir = Path(out_dir)
    query_out_dir.mkdir(parents=True, exist_ok=True)
    for query_field in query_fields:
        query_path = query_out_dir / f"{_camel_to_snake(query_field.root_name)}.graphql"
        query_path.write_text(_render_query(query_field, types_by_name))

    endpoint_dir_path = Path(endpoint_dir)
    endpoint_rows = _build_endpoint_rows(
        endpoint_dir=endpoint_dir_path,
        query_fields=query_fields,
        types_by_name=types_by_name,
    )
    sync_actions: list[dict[str, object]] = []
    if sync_configs:
        sync_actions = _sync_endpoint_configs(
            endpoint_dir=endpoint_dir_path,
            query_out_dir=query_out_dir,
            endpoint_rows=endpoint_rows,
        )
        endpoint_rows = _build_endpoint_rows(
            endpoint_dir=endpoint_dir_path,
            query_fields=query_fields,
            types_by_name=types_by_name,
        )

    report = Path(report_path)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(
        _render_report(
            query_fields=query_fields,
            endpoint_rows=endpoint_rows,
            query_out_dir=query_out_dir,
            sync_actions=sync_actions,
        )
    )

    rprint(
        f"[green]✓[/green] generated {len(query_fields)} DataMart query file(s) → {query_out_dir}"
    )
    if sync_actions:
        rprint(f"[green]✓[/green] synced {len(sync_actions)} endpoint config(s) from live schema")
    rprint(f"[green]✓[/green] mapping report → {report}")


def _load_catalog(introspection_path: Path) -> tuple[list[QueryField], dict[str, dict]]:
    if not introspection_path.exists():
        raise SystemExit(
            f"introspection snapshot not found: {introspection_path} "
            "(run `vsv2-discover introspect` first)"
        )

    raw = json.loads(introspection_path.read_text())
    schema = raw.get("__schema") or raw.get("data", {}).get("__schema")
    if not isinstance(schema, dict):
        raise SystemExit(f"invalid introspection payload: {introspection_path}")

    types_by_name = {
        item["name"]: item
        for item in schema.get("types", [])
        if isinstance(item, dict) and item.get("name")
    }
    query_type_name = schema.get("queryType", {}).get("name")
    query_type = types_by_name.get(query_type_name)
    if not isinstance(query_type, dict):
        raise SystemExit(f"query type {query_type_name!r} not found in introspection payload")

    query_fields: list[QueryField] = []
    for field in query_type.get("fields", []) or []:
        type_name = _unwrapped_type_name(field.get("type"))
        if not type_name:
            continue
        query_fields.append(
            QueryField(
                root_name=field["name"],
                type_name=type_name,
                is_list=_is_list_type(field.get("type")),
                args=tuple(field.get("args") or []),
            )
        )
    query_fields.sort(key=lambda item: item.root_name)
    return query_fields, types_by_name


def _render_query(query_field: QueryField, types_by_name: dict[str, dict]) -> str:
    arg_defs = [
        _render_variable(
            arg,
            default_override=(
                "true"
                if query_field.root_name == "generalLedgerBalancesV2"
                and arg["name"] == "includeDeleted"
                else None
            ),
        )
        for arg in query_field.args
    ]
    arg_defs_rendered = ""
    if arg_defs:
        rendered = ",\n  ".join(arg_defs)
        arg_defs_rendered = f"(\n  {rendered}\n)"

    call_args = ",\n    ".join(f"{arg['name']}: ${arg['name']}" for arg in query_field.args)
    call_args_rendered = ""
    if call_args:
        call_args_rendered = f"(\n    {call_args}\n  )"

    selection = _render_selection(
        type_name=query_field.type_name,
        types_by_name=types_by_name,
        depth=0,
        max_depth=1,
        seen=(),
    )
    selection_rendered = ""
    if selection:
        selection_rendered = " {\n" + "\n".join(f"    {line}" for line in selection) + "\n  }"

    query_name = _camel_to_pascal(query_field.root_name)
    return (
        f"query {query_name}{arg_defs_rendered} {{\n"
        f"  {query_field.root_name}{call_args_rendered}{selection_rendered}\n"
        f"}}\n"
    )


def _render_variable(arg: dict, *, default_override: str | None = None) -> str:
    rendered = f"${arg['name']}: {_render_type_ref(arg['type'])}"
    default_value = default_override if default_override is not None else arg.get("defaultValue")
    if default_value is not None:
        rendered += f" = {default_value}"
    return rendered


def _render_selection(
    *,
    type_name: str,
    types_by_name: dict[str, dict],
    depth: int,
    max_depth: int,
    seen: tuple[str, ...],
) -> list[str]:
    if type_name in seen:
        return []
    type_def = types_by_name.get(type_name)
    if not isinstance(type_def, dict) or type_def.get("kind") != "OBJECT":
        return []

    scalar_fields: list[str] = []
    nested_blocks: list[list[str]] = []
    for field in type_def.get("fields", []) or []:
        field_type = _unwrap_type_ref(field.get("type"))
        kind = field_type.get("kind")
        nested_type_name = field_type.get("name")
        field_name = field["name"]
        if kind in {"SCALAR", "ENUM"}:
            scalar_fields.append(field_name)
            continue
        if depth >= max_depth or not nested_type_name:
            continue
        nested = _render_selection(
            type_name=nested_type_name,
            types_by_name=types_by_name,
            depth=depth + 1,
            max_depth=max_depth,
            seen=(*seen, type_name),
        )
        nested = _trim_nested_identity_fields(nested)
        if not nested:
            continue
        nested_blocks.append([f"{field_name} {{", *(f"  {line}" for line in nested), "}"])

    ordered_scalars = _ordered_identity_fields(scalar_fields)
    lines = ordered_scalars[:]
    for block in nested_blocks:
        lines.extend(block)
    return lines


def _trim_nested_identity_fields(lines: list[str]) -> list[str]:
    scalar_lines = [line for line in lines if "{" not in line and "}" not in line]
    preferred = [line for line in scalar_lines if line in _IDENTITY_FIELD_CANDIDATES]
    rest = [line for line in scalar_lines if line not in preferred]
    trimmed = preferred + rest
    return trimmed[:5]


def _ordered_identity_fields(scalar: list[str]) -> list[str]:
    preferred = [name for name in _IDENTITY_FIELD_CANDIDATES if name in scalar]
    rest = [name for name in scalar if name not in preferred]
    return preferred + rest


def _render_report(
    *,
    query_fields: list[QueryField],
    endpoint_rows: list[dict[str, object]],
    query_out_dir: Path,
    sync_actions: list[dict[str, object]],
) -> str:
    direct = sum(1 for row in endpoint_rows if row["status"] == "direct")
    alias = sum(1 for row in endpoint_rows if row["status"] == "alias")
    unmapped = sum(1 for row in endpoint_rows if row["status"] == "unmapped")
    mapped_roots = {row["mapped_query_root"] for row in endpoint_rows if row["mapped_query_root"]}
    unmapped_live_roots = [
        item.root_name for item in query_fields if item.root_name not in mapped_roots
    ]
    created_queries = sum(1 for action in sync_actions if action["created_query_file"])
    updated_configs = sum(1 for action in sync_actions if action["updated_config"])

    lines = [
        "# DataMart GraphQL Catalog",
        "",
        "Generated from the live DataMart introspection snapshot.",
        "",
        f"- live query roots: {len(query_fields)}",
        f"- endpoint configs with DataMart blocks: {len(endpoint_rows)}",
        f"- direct config mappings: {direct}",
        f"- alias-based mappings: {alias}",
        f"- unmapped config placeholders: {unmapped}",
        f"- live query roots without local endpoint mapping: {len(unmapped_live_roots)}",
        f"- query output directory: `{query_out_dir}`",
    ]
    if sync_actions:
        lines.extend(
            [
                f"- synced endpoint configs: {len(sync_actions)}",
                f"- missing config query files created: {created_queries}",
                f"- config JSON files updated: {updated_configs}",
            ]
        )
    lines.extend(
        [
            "",
            "## Live Query Roots",
            "",
            "| query_root | return_type | args | file |",
            "|---|---|---|---|",
        ]
    )
    for query_field in query_fields:
        args = ", ".join(arg["name"] for arg in query_field.args) or "-"
        file_name = f"{_camel_to_snake(query_field.root_name)}.graphql"
        lines.append(
            f"| `{query_field.root_name}` | `{query_field.type_name}` | "
            f"`{args}` | `{query_out_dir / file_name}` |"
        )

    lines.extend(
        [
            "",
            "## Endpoint Config Mapping",
            "",
            "| endpoint | type_name | mapped_query_root | status | generated_query |",
            "|---|---|---|---|---|",
        ]
    )
    for row in endpoint_rows:
        generated_file = (
            f"`{query_out_dir / (_camel_to_snake(row['mapped_query_root']) + '.graphql')}`"
            if row["mapped_query_root"]
            else ""
        )
        lines.append(
            f"| `{row['endpoint']}` | `{row['type_name'] or ''}` | "
            f"`{row['mapped_query_root'] or ''}` | {row['status']} | {generated_file} |"
        )

    lines.extend(
        [
            "",
            "## Live Roots Without Local Endpoint Mapping",
            "",
        ]
    )
    if unmapped_live_roots:
        lines.extend(f"- `{root}`" for root in unmapped_live_roots)
    else:
        lines.append("- none")

    if sync_actions:
        lines.extend(
            [
                "",
                "## Config Sync Actions",
                "",
                "| endpoint | root_field | query_file_created | "
                "type_name_filled | config_updated |",
                "|---|---|---:|---:|---:|",
            ]
        )
        for action in sync_actions:
            lines.append(
                f"| `{action['endpoint']}` | `{action['root_field']}` | "
                f"{'yes' if action['created_query_file'] else 'no'} | "
                f"{'yes' if action['filled_type_name'] else 'no'} | "
                f"{'yes' if action['updated_config'] else 'no'} |"
            )

    lines.extend(
        [
            "",
            "## Notes",
            "",
            "- This catalog follows the live schema exposed by `https://datamart.visma.net/graphql`.",
            "- It uses the current Visma pattern documented in the DataMart docs: "
            "`lastId`, `updatedInDataMart`, and `pageSize` for bulk queries.",
            "- Nested selections are intentionally shallow. They are a schema catalog, not yet a "
            "guaranteed bronze-parity DTO set.",
            "- Unmapped endpoint configs need either an explicit query-root alias or a decision "
            "that the ERP endpoint has no DataMart equivalent.",
        ]
    )
    return "\n".join(lines) + "\n"


def _build_endpoint_rows(
    *,
    endpoint_dir: Path,
    query_fields: list[QueryField],
    types_by_name: dict[str, dict],
) -> list[dict[str, object]]:
    query_root_names = {item.root_name for item in query_fields}
    query_by_root = {item.root_name: item for item in query_fields}
    query_roots_by_type: dict[str, list[str]] = {}
    for item in query_fields:
        query_roots_by_type.setdefault(item.type_name, []).append(item.root_name)
    for roots in query_roots_by_type.values():
        roots.sort()

    rows: list[dict[str, object]] = []
    for path in sorted(endpoint_dir.glob("*.json")):
        if path.name.startswith("_"):
            continue
        data = json.loads(path.read_text())
        dm_cfg = data.get("datamart")
        if not isinstance(dm_cfg, dict):
            continue

        endpoint = data["endpoint_name"]
        type_name = dm_cfg.get("type_name")
        configured_root = dm_cfg.get("root_field")
        configured_query_file = dm_cfg.get("query_file")
        mapped_root: str | None = None
        status = "unmapped"

        if configured_root and configured_root in query_root_names:
            mapped_root = configured_root
            status = "direct"
        elif type_name and len(query_roots_by_type.get(type_name, [])) == 1:
            mapped_root = query_roots_by_type[type_name][0]
            status = "direct"
        else:
            camel_guess = _snake_to_camel(endpoint)
            if camel_guess in query_root_names:
                mapped_root = camel_guess
                status = "direct"
            elif endpoint in _SAFE_ENDPOINT_ALIASES:
                alias = _SAFE_ENDPOINT_ALIASES[endpoint]
                if alias in query_root_names:
                    mapped_root = alias
                    status = "alias"

        rows.append(
            {
                "path": path,
                "endpoint": endpoint,
                "type_name": type_name,
                "configured_root_field": configured_root,
                "configured_query_file": configured_query_file,
                "mapped_query_root": mapped_root,
                "status": status,
                "mapped_type_name": query_by_root[mapped_root].type_name if mapped_root else None,
            }
        )

    rows.sort(key=lambda item: item["endpoint"] or "")
    return rows


def _sync_endpoint_configs(
    *,
    endpoint_dir: Path,
    query_out_dir: Path,
    endpoint_rows: list[dict[str, object]],
) -> list[dict[str, object]]:
    repo_root = endpoint_dir.resolve().parents[1]
    sync_actions: list[dict[str, object]] = []

    for row in endpoint_rows:
        mapped_root = row["mapped_query_root"]
        if not isinstance(mapped_root, str) or not mapped_root:
            continue

        path = row["path"]
        if not isinstance(path, Path):
            continue

        data = json.loads(path.read_text())
        dm_cfg = data.get("datamart")
        if not isinstance(dm_cfg, dict):
            continue

        updated_config = False
        filled_type_name = False

        if dm_cfg.get("root_field") != mapped_root:
            dm_cfg["root_field"] = mapped_root
            updated_config = True

        mapped_type_name = row["mapped_type_name"]
        if not dm_cfg.get("type_name") and isinstance(mapped_type_name, str) and mapped_type_name:
            dm_cfg["type_name"] = mapped_type_name
            updated_config = True
            filled_type_name = True

        query_file = dm_cfg.get("query_file")
        created_query_file = False
        if isinstance(query_file, str) and query_file.strip():
            target_path = repo_root / query_file
            if not target_path.exists():
                source_path = query_out_dir / f"{_camel_to_snake(mapped_root)}.graphql"
                if source_path.exists():
                    target_path.parent.mkdir(parents=True, exist_ok=True)
                    target_path.write_text(source_path.read_text())
                    created_query_file = True

        if updated_config:
            path.write_text(json.dumps(data, indent=2) + "\n")

        sync_actions.append(
            {
                "endpoint": row["endpoint"],
                "root_field": mapped_root,
                "created_query_file": created_query_file,
                "filled_type_name": filled_type_name,
                "updated_config": updated_config,
            }
        )

    return sync_actions


def _render_type_ref(ref: dict | None) -> str:
    if not isinstance(ref, dict):
        return "Unknown"
    kind = ref.get("kind")
    if kind == "NON_NULL":
        return f"{_render_type_ref(ref.get('ofType'))}!"
    if kind == "LIST":
        return f"[{_render_type_ref(ref.get('ofType'))}]"
    return ref.get("name") or "Unknown"


def _unwrap_type_ref(ref: dict | None) -> dict:
    current = ref or {}
    while current.get("kind") in {"NON_NULL", "LIST"}:
        current = current.get("ofType") or {}
    return current


def _unwrapped_type_name(ref: dict | None) -> str | None:
    current = _unwrap_type_ref(ref)
    return current.get("name")


def _is_list_type(ref: dict | None) -> bool:
    current = ref
    while isinstance(current, dict):
        kind = current.get("kind")
        if kind == "LIST":
            return True
        if kind != "NON_NULL":
            return False
        current = current.get("ofType")
    return False


def _camel_to_pascal(value: str) -> str:
    spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", value)
    return "".join(part.capitalize() for part in spaced.replace("_", " ").split())


def _camel_to_snake(value: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", value)
    return value.replace("-", "_").lower()


def _snake_to_camel(value: str) -> str:
    parts = value.split("_")
    return parts[0] + "".join(part.capitalize() for part in parts[1:])
