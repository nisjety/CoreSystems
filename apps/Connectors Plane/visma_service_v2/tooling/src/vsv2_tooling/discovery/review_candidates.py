"""Review synchronized DataMart candidates against bronze schemas."""

from __future__ import annotations

import inspect
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from graphql import OperationDefinitionNode, parse
from rich import print as rprint

from vsv2_tooling._http import DatamartGraphQL, OAuth, VismaEnv
from vsv2_tooling.normalizers import REGISTRY as NORMALIZER_REGISTRY

COMMON_SYSTEM_FIELDS = frozenset(
    {
        "syncId",
        "syncedAt",
        "firstSyncedAt",
        "refreshedAt",
        "seenInFullScopeRuns",
        "consecutiveMissingFullRuns",
        "isDeleted",
        "deletedAt",
        "tenantId",
        "syncBatch",
        "organizationName",
    }
)


@dataclass(frozen=True)
class Candidate:
    endpoint: str
    bigquery_table: str
    query_file: str
    root_field: str
    type_name: str | None
    join_normalizer: str | None
    bronze_schema_path: Path


@dataclass(frozen=True)
class ReviewRow:
    endpoint: str
    bigquery_table: str
    root_field: str
    query_file: str
    bronze_schema_path: str
    sample_ok: bool
    sample_error: str | None
    normalizer: str | None
    normalizer_registered: bool
    normalizer_aux_params: list[str]
    bronze_field_count: int
    delivered_field_count: int
    missing_fields: list[str]
    extra_fields: list[str]
    status: str
    downstream_refs: list[str]


def run(
    *,
    endpoint_dir: str,
    bronze_dir: str,
    report_path: str,
) -> None:
    root = Path(endpoint_dir).resolve().parents[1]
    candidates = _load_candidates(Path(endpoint_dir), Path(bronze_dir))
    env = VismaEnv.from_env()
    oauth = OAuth(env)
    dm = DatamartGraphQL(env, oauth)
    try:
        rows = [_review_candidate(root, dm, candidate) for candidate in candidates]
    finally:
        dm.close()
        oauth.close()

    report = Path(report_path)
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(_render_report(rows))
    ready = sum(1 for row in rows if row.status == "candidate_for_shadow")
    rprint(f"[green]✓[/green] reviewed {len(rows)} synchronized DataMart candidate(s)")
    rprint(f"[green]✓[/green] candidate_for_shadow: {ready}")
    rprint(f"[green]✓[/green] report → {report}")


def _load_candidates(endpoint_dir: Path, bronze_dir: Path) -> list[Candidate]:
    candidates: list[Candidate] = []
    for path in sorted(endpoint_dir.glob("*.json")):
        if path.name.startswith("_"):
            continue
        data = json.loads(path.read_text())
        dm = data.get("datamart")
        if not isinstance(dm, dict) or not dm.get("root_field"):
            continue
        candidates.append(
            Candidate(
                endpoint=data["endpoint_name"],
                bigquery_table=data["bigquery_table"],
                query_file=dm["query_file"],
                root_field=dm["root_field"],
                type_name=dm.get("type_name"),
                join_normalizer=dm.get("join_normalizer"),
                bronze_schema_path=bronze_dir / f"{data['bigquery_table']}.sqlx",
            )
        )
    return candidates


def _review_candidate(root: Path, dm: DatamartGraphQL, candidate: Candidate) -> ReviewRow:
    query_path = root / candidate.query_file
    query_text = query_path.read_text()
    query_fields = _extract_query_output_fields(query_text)
    bronze_fields = _business_bronze_fields(
        _load_bronze_schema_fields(candidate.bronze_schema_path)
    )
    downstream_refs = _find_downstream_refs(candidate.bigquery_table, candidate.bronze_schema_path)

    sample_error: str | None = None
    sample_record: dict[str, Any] | None = None
    try:
        data = dm.execute(
            query_text,
            variables={
                "pageSize": 1,
                "lastId": None,
                "updatedInDataMart": None,
                "updatedSince": None,
            },
        )
        items = data.get(candidate.root_field)
        if isinstance(items, list) and items:
            if isinstance(items[0], dict):
                sample_record = items[0]
        elif isinstance(items, dict):
            sample_record = items
        else:
            sample_error = "live query returned no sample rows"
    except Exception as exc:  # pragma: no cover - live integration path
        sample_error = str(exc)

    normalizer_registered = False
    normalizer_aux_params: list[str] = []
    delivered_fields = list(query_fields)
    if candidate.join_normalizer:
        fn = NORMALIZER_REGISTRY.get(candidate.join_normalizer)
        normalizer_registered = fn is not None
        if fn is not None:
            delivered_fields = _apply_normalizer_preview(fn, sample_record or {})
            normalizer_aux_params = [
                param.name
                for idx, param in enumerate(inspect.signature(fn).parameters.values())
                if idx > 0
            ]
        else:
            delivered_fields = []

    missing_fields = sorted(set(bronze_fields) - set(delivered_fields))
    extra_fields = sorted(set(delivered_fields) - set(bronze_fields))
    status = _classify_status(
        sample_error=sample_error,
        bronze_field_count=len(bronze_fields),
        normalizer_name=candidate.join_normalizer,
        normalizer_registered=normalizer_registered,
        normalizer_aux_params=normalizer_aux_params,
        missing_fields=missing_fields,
    )

    return ReviewRow(
        endpoint=candidate.endpoint,
        bigquery_table=candidate.bigquery_table,
        root_field=candidate.root_field,
        query_file=candidate.query_file,
        bronze_schema_path=str(candidate.bronze_schema_path),
        sample_ok=sample_error is None,
        sample_error=sample_error,
        normalizer=candidate.join_normalizer,
        normalizer_registered=normalizer_registered,
        normalizer_aux_params=normalizer_aux_params,
        bronze_field_count=len(bronze_fields),
        delivered_field_count=len(delivered_fields),
        missing_fields=missing_fields,
        extra_fields=extra_fields,
        status=status,
        downstream_refs=downstream_refs,
    )


def _apply_normalizer_preview(fn, sample_record: dict[str, Any]) -> list[str]:
    normalized = fn(sample_record)
    if not isinstance(normalized, dict):
        return []
    return sorted(key for key in normalized if not key.startswith("_"))


def _load_bronze_schema_fields(path: Path) -> list[str]:
    text = path.read_text()
    marker = text.find("//schema")
    if marker == -1:
        return []
    schema_text = text[marker + len("//schema") :].strip()
    start = schema_text.find("[")
    end = schema_text.rfind("]")
    if start == -1 or end == -1:
        return []
    schema = json.loads(schema_text[start : end + 1])
    return [field["name"] for field in schema if isinstance(field, dict) and field.get("name")]


def _business_bronze_fields(fields: list[str]) -> list[str]:
    return [field for field in fields if field not in COMMON_SYSTEM_FIELDS]


def _extract_query_output_fields(query_text: str) -> list[str]:
    document = parse(query_text)
    for definition in document.definitions:
        if isinstance(definition, OperationDefinitionNode):
            for selection in definition.selection_set.selections:
                if not hasattr(selection, "selection_set") or selection.selection_set is None:
                    continue
                fields: list[str] = []
                for child in selection.selection_set.selections:
                    alias = getattr(child, "alias", None)
                    name = alias.value if alias is not None else child.name.value
                    fields.append(name)
                return sorted(fields)
    return []


def _find_downstream_refs(table: str, bronze_schema_path: Path) -> list[str]:
    definitions_root = bronze_schema_path.resolve().parents[2]
    hits: list[str] = []
    for path in definitions_root.rglob("*.sqlx"):
        if path == bronze_schema_path:
            continue
        text = path.read_text()
        if table in text and "ref(" in text:
            hits.append(str(path))
    hits.sort()
    return hits[:3]


def _classify_status(
    *,
    sample_error: str | None,
    bronze_field_count: int,
    normalizer_name: str | None,
    normalizer_registered: bool,
    normalizer_aux_params: list[str],
    missing_fields: list[str],
) -> str:
    if sample_error is not None:
        return "blocked_live_sample"
    if bronze_field_count == 0:
        return "blocked_bronze_schema_missing"
    if normalizer_name and not normalizer_registered:
        return "blocked_missing_normalizer"
    if normalizer_aux_params:
        return "blocked_join_runtime_gap"
    if missing_fields:
        if normalizer_name is None:
            return "blocked_no_normalizer_and_parity_gap"
        return "blocked_parity_gap"
    return "candidate_for_shadow"


def _render_report(rows: list[ReviewRow]) -> str:
    counts: dict[str, int] = {}
    for row in rows:
        counts[row.status] = counts.get(row.status, 0) + 1

    lines = [
        "# DataMart Candidate Bronze Parity Review",
        "",
        "Live review of the synchronized Layer 1 DataMart mappings against:",
        "",
        "- current endpoint query files",
        "- bronze Dataform declaration schemas",
        "- registered normalizers",
        "- one live sample pull per endpoint",
        "",
        f"- candidates reviewed: {len(rows)}",
    ]
    for status in sorted(counts):
        lines.append(f"- {status}: {counts[status]}")

    lines.extend(
        [
            "",
            "## Summary",
            "",
            "| endpoint | bronze_table | sample | normalizer | "
            "delivered/bronze | missing | status |",
            "|---|---|---:|---|---:|---:|---|",
        ]
    )
    for row in rows:
        normalizer = row.normalizer or "-"
        if row.normalizer and not row.normalizer_registered:
            normalizer = f"{row.normalizer} (missing)"
        lines.append(
            f"| `{row.endpoint}` | `{row.bigquery_table}` | "
            f"{'ok' if row.sample_ok else 'fail'} | `{normalizer}` | "
            f"{row.delivered_field_count}/{row.bronze_field_count} | "
            f"{len(row.missing_fields)} | `{row.status}` |"
        )

    lines.extend(["", "## Detailed Review", ""])
    for row in rows:
        lines.append(f"### `{row.endpoint}`")
        lines.append("")
        lines.append(f"- bronze table: `{row.bigquery_table}`")
        lines.append(f"- bronze schema: `{row.bronze_schema_path}`")
        lines.append(f"- query file: `{row.query_file}`")
        lines.append(f"- DataMart root: `{row.root_field}`")
        lines.append(f"- live sample: {'ok' if row.sample_ok else 'failed'}")
        if row.sample_error:
            lines.append(f"- sample error: `{row.sample_error}`")
        lines.append(f"- normalizer: `{row.normalizer or '-'}`")
        lines.append(f"- normalizer registered: {'yes' if row.normalizer_registered else 'no'}")
        if row.normalizer_aux_params:
            lines.append(
                "- normalizer auxiliary params not wired by runtime: "
                + ", ".join(f"`{name}`" for name in row.normalizer_aux_params)
            )
        lines.append(
            f"- field parity: {row.delivered_field_count} delivered vs "
            f"{row.bronze_field_count} bronze fields"
        )
        lines.append(f"- status: `{row.status}`")
        if row.bronze_field_count == 0:
            lines.append("- bronze schema status: declaration has no parseable `//schema` block")
        if row.downstream_refs:
            lines.append("- downstream refs:")
            lines.extend(f"  - `{path}`" for path in row.downstream_refs)
        if row.missing_fields:
            preview = ", ".join(f"`{name}`" for name in row.missing_fields[:20])
            if len(row.missing_fields) > 20:
                preview += ", ..."
            lines.append(f"- missing bronze fields: {preview}")
        if row.extra_fields:
            preview = ", ".join(f"`{name}`" for name in row.extra_fields[:15])
            if len(row.extra_fields) > 15:
                preview += ", ..."
            lines.append(f"- extra delivered fields not in bronze declaration: {preview}")
        lines.append("")

    lines.extend(
        [
            "## Interpretation",
            "",
            "- `candidate_for_shadow` means the current query and current "
            "normalizer shape can be shadow-tested without obvious bronze field loss.",
            "- `blocked_bronze_schema_missing` means the bronze declaration file "
            "does not define a parseable field contract yet, so parity cannot be proven.",
            "- `blocked_parity_gap` means the configured normalizer still drops "
            "bronze fields that the legacy contract expects.",
            "- `blocked_no_normalizer_and_parity_gap` means the raw GraphQL "
            "shape does not match bronze closely enough and no normalizer bridges it.",
            "- `blocked_join_runtime_gap` means the normalizer expects extra "
            "join-style context that the loader does not currently provide.",
            "- `blocked_live_sample` means the endpoint could not be verified "
            "with a live sample pull.",
        ]
    )
    return "\n".join(lines) + "\n"
