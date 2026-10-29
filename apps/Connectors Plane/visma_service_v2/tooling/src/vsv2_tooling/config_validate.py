"""Validate endpoint JSON config files against the schema in config/endpoints/_schema.json.

Beyond the schema it enforces the rules the Rust registry applies when it loads a config
(`open_set_problem` below mirrors `EndpointDefinition::open_set_problem`): a config the registry
refuses stops every process that reads `config/endpoints`, so it has to be caught here, before
deploy.

Skips files starting with `_` (treated as schema/metadata). Exits non-zero on any failure.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

from jsonschema import Draft7Validator
from rich.console import Console
from rich.table import Table

from vsv2_tooling.normalizers import REGISTRY as NORMALIZER_REGISTRY


def _repo_root() -> Path:
    # this file: tooling/src/vsv2_tooling/config_validate.py -> repo root is parents[3]
    return Path(__file__).resolve().parents[3]


def _load_schema(endpoints_dir: Path) -> dict:
    schema_path = endpoints_dir / "_schema.json"
    if not schema_path.exists():
        raise FileNotFoundError(f"Schema not found: {schema_path}")
    return json.loads(schema_path.read_text(encoding="utf-8"))


def _iter_endpoint_files(endpoints_dir: Path):
    for path in sorted(endpoints_dir.glob("*.json")):
        if path.name.startswith("_"):
            continue
        yield path


def _is_http_path(value: str) -> bool:
    return (
        value.startswith("/")
        or value.startswith("v1/")
        or value.startswith("v2/")
        or value.startswith("api/")
        or value.startswith("https://")
        or value.startswith("http://")
    )


def _partition_query_params(data: dict) -> set[str]:
    partitioning = data.get("partitioning")
    if not isinstance(partitioning, dict):
        return set()
    params: set[str] = set()
    for dim in partitioning.get("dimensions") or []:
        if isinstance(dim, dict) and isinstance(dim.get("query_param"), str):
            params.add(dim["query_param"])
    return params


def _primary_key_fields(primary_key: object) -> list[str]:
    """The key fields as the registry reads them: a string or a list, joined, split on commas."""
    if isinstance(primary_key, str):
        parts = [primary_key]
    elif isinstance(primary_key, list):
        parts = [part for part in primary_key if isinstance(part, str)]
    else:
        parts = []
    return [field.strip() for field in ",".join(parts).split(",") if field.strip()]


def open_set_problem(data: object) -> str | None:
    """Why ``open_set: true`` cannot work for this config, or ``None``.

    The mirror of ``EndpointDefinition::open_set_problem`` in
    ``core/crates/vsv2-orchestrator/src/registry.rs``: the same rules, in the same order, in the
    same words (the Rust text has an ``endpoint '<name>': `` in front). The first rule that fails
    is the answer, so both sides name the same problem for a config that breaks several. The
    Rust registry refuses such a config when it loads, which would stop every process that reads
    ``config/endpoints``; the fixtures in ``tooling/tests/fixtures/open_set_cases.json`` are read
    by a test on each side, so a rule changed in one place and not the other is seen.
    """
    if not isinstance(data, dict) or data.get("open_set") is not True:
        return None

    def fail(reason: str) -> str:
        return f"open_set needs {reason}"

    # `source` and `pagination_profile` default the way the Rust side defaults them.
    if data.get("source", "erp_rest") != "erp_rest":
        return fail("source erp_rest")
    api_endpoint = data.get("api_endpoint")
    path = api_endpoint.strip() if isinstance(api_endpoint, str) else ""
    if not path or "{" in path or "?" in path:
        return fail("an api_endpoint without placeholders or a query string")
    if data.get("pagination_profile", "finance_page_number") != "finance_page_number":
        return fail("pagination_profile finance_page_number")
    tier = data.get("tier")
    if isinstance(tier, bool) or tier != 1:  # True == 1 in Python, and is not a tier
        return fail("tier 1: only the hourly pass runs it")
    if data.get("partitioning") is not None or data.get("period_scan") is not None:
        return fail("an endpoint that is not partitioned or read by period")
    # The by-key request puts the key values in the path and the key tool prints them under their
    # own names, so they have to be top-level fields of the document.
    key_fields = _primary_key_fields(data.get("primary_key"))
    if not key_fields or any(
        field in ("tenant_id", "_partition_key") or "." in field for field in key_fields
    ):
        return fail("a primary key of top-level fields (no dots, tenant_id or _partition_key)")
    return None


def validate_configs(endpoints_dir: Path, root: Path) -> tuple[list[tuple[str, str]], int]:
    """Validate every config in ``endpoints_dir``; return ``(failures, files_checked)``.

    ``failures`` holds ``(file name, message)`` pairs. ``root`` is the repo root that
    ``datamart.query_file`` paths are relative to.
    """
    schema = _load_schema(endpoints_dir)
    validator = Draft7Validator(schema)

    failures: list[tuple[str, str]] = []
    checked = 0

    for path in _iter_endpoint_files(endpoints_dir):
        checked += 1
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            failures.append((path.name, f"invalid JSON: {exc}"))
            continue

        errors = sorted(validator.iter_errors(data), key=lambda e: list(e.path))
        for err in errors:
            loc = "/".join(str(p) for p in err.path) or "<root>"
            failures.append((path.name, f"{loc}: {err.message}"))

        if data.get("endpoint_name") != path.stem:
            failures.append(
                (
                    path.name,
                    f"endpoint_name must match filename stem '{path.stem}'",
                )
            )

        if data.get("source") == "erp_rest":
            api_endpoint = data.get("api_endpoint")
            if not isinstance(api_endpoint, str) or not api_endpoint.strip():
                failures.append(
                    (
                        path.name,
                        "source=erp_rest requires api_endpoint with a real Visma HTTP path",
                    )
                )
            elif api_endpoint.startswith("get_") or not _is_http_path(api_endpoint):
                failures.append(
                    (
                        path.name,
                        f"api_endpoint is not an executable HTTP path: {api_endpoint}",
                    )
                )

            profile = data.get("pagination_profile")
            if profile is None:
                failures.append(
                    (
                        path.name,
                        "source=erp_rest requires explicit pagination_profile",
                    )
                )
            elif (
                "salesorder.visma.net" in str(api_endpoint)
                and profile != "salesorder_page_index"
                and not (profile == "none" and re.search(r"\{[A-Za-z_]\w*\}", str(api_endpoint)))
            ):
                # A per-parent child such as /Customers/{customerId}/locations returns a plain
                # list and takes no paging parameters; only listings need the page-index profile.
                failures.append(
                    (
                        path.name,
                        "SalesOrder endpoints must use pagination_profile=salesorder_page_index",
                    )
                )
            elif (
                isinstance(api_endpoint, str)
                and (
                    "journaltx.visma.net" in api_endpoint
                    or api_endpoint.lower() == "/v2/journaltransaction"
                )
                and profile != "finance_page_number"
            ):
                # /v2/journaltransaction is a Finance API v2 path: it paginates
                # by 1-based `pageNumber` and is scoped by `lastModifiedDateTime`
                # (the finance_page_number profile). Pages are *line*-grained, so
                # a single batch can span pages; the orchestrator merges
                # journalTransactionLines per (module, batchNumber) after fetch.
                failures.append(
                    (
                        path.name,
                        "JournalTransaction endpoints must use "
                        "pagination_profile=finance_page_number",
                    )
                )
            elif isinstance(api_endpoint, str) and api_endpoint.lower() == "/v2/salesorder":
                failures.append(
                    (
                        path.name,
                        "SalesOrder V1/V2 endpoints are deprecated/EOL; "
                        "remove this config and use sales_orders_v3",
                    )
                )

            background_mode = data.get("background_mode", "off")
            if background_mode != "off":
                if not isinstance(api_endpoint, str) or api_endpoint.startswith("http"):
                    failures.append(
                        (
                            path.name,
                            "background_mode is only supported for Finance API "
                            "relative v1/v2 endpoints",
                        )
                    )
                elif not (
                    api_endpoint.lower().startswith("/v1/")
                    or api_endpoint.lower().startswith("/v2/")
                ):
                    failures.append(
                        (
                            path.name,
                            "background_mode is only supported for Finance API v1/v2 endpoints",
                        )
                    )

            default_query = data.get("default_query") or {}
            partition_params = _partition_query_params(data)
            runtime_required_query = set()
            period_scan = data.get("period_scan")
            if isinstance(period_scan, dict):
                # The runtime sets these once per month; the schema checks the shape.
                scan_params = set(period_scan.get("query_params") or [])
                runtime_required_query.update(scan_params)
                if period_scan.get("resume") and not data.get("modified_field"):
                    failures.append(
                        (
                            path.name,
                            "period_scan.resume needs a modified_field to resume from",
                        )
                    )
                period_field = period_scan.get("period_field")
                key = data.get("primary_key")
                key_fields = key if isinstance(key, list) else [key]
                if period_field and period_field not in key_fields:
                    failures.append(
                        (
                            path.name,
                            f"period_scan.period_field '{period_field}' must be part of "
                            "primary_key, otherwise it separates nothing",
                        )
                    )
                for param in sorted(scan_params & set(default_query)):
                    failures.append(
                        (
                            path.name,
                            f"default_query.{param} is overwritten by period_scan, which sends "
                            "each month under that name",
                        )
                    )
            elif data.get("sync_pattern") == "scoped_wrapper_only":
                # The runtime has no period of its own: starting at the current month would
                # fetch a fraction of the data and look fine.
                failures.append(
                    (
                        path.name,
                        "a scoped_wrapper_only endpoint needs period_scan.first_period, "
                        "the first financial period (YYYYMM) that can hold data",
                    )
                )

            for required in data.get("required_query") or []:
                if (
                    required not in default_query
                    and required not in partition_params
                    and required not in runtime_required_query
                ):
                    failures.append(
                        (
                            path.name,
                            f"required_query '{required}' must be supplied by default_query, "
                            "partitioning.dimensions, or runtime wrapper",
                        )
                    )

            partitioning = data.get("partitioning")
            if isinstance(partitioning, dict):
                dimensions = partitioning.get("dimensions") or []
                seen_params: set[str] = set()
                for dim in dimensions:
                    if not isinstance(dim, dict):
                        continue
                    query_param = dim.get("query_param")
                    source_endpoint = dim.get("source_endpoint")
                    if isinstance(query_param, str):
                        if query_param in seen_params:
                            failures.append(
                                (
                                    path.name,
                                    f"partitioning dimension query_param is duplicated: "
                                    f"{query_param}",
                                )
                            )
                        seen_params.add(query_param)
                    if not isinstance(source_endpoint, str) or not _is_http_path(source_endpoint):
                        failures.append(
                            (
                                path.name,
                                "partitioning source_endpoint is not an executable HTTP path: "
                                f"{source_endpoint}",
                            )
                        )
                    if (
                        dim.get("in_path")
                        and isinstance(query_param, str)
                        and "{" + query_param + "}" not in str(data.get("api_endpoint") or "")
                    ):
                        failures.append(
                            (
                                path.name,
                                f"partitioning dimension '{query_param}' is in_path but "
                                f"api_endpoint has no {{{query_param}}}",
                            )
                        )
                filled = {
                    d["query_param"]
                    for d in dimensions
                    if isinstance(d, dict)
                    and d.get("in_path")
                    and isinstance(d.get("query_param"), str)
                }
            else:
                filled = set()
            for placeholder in re.findall(
                r"\{([A-Za-z_][A-Za-z0-9_]*)\}", str(data.get("api_endpoint") or "")
            ):
                if placeholder not in filled:
                    failures.append(
                        (
                            path.name,
                            f"api_endpoint has {{{placeholder}}} but no partitioning dimension "
                            f"with query_param '{placeholder}' and in_path=true fills it",
                        )
                    )

        problem = open_set_problem(data)
        if problem is not None:
            failures.append((path.name, problem))

        # Invariant #4: Tier 1 endpoints must never flip to DataMart.
        dm = data.get("datamart")
        if data.get("tier") == 1 and isinstance(dm, dict) and dm.get("ready"):
            failures.append(
                (
                    path.name,
                    "Invariant #4 violation: tier=1 endpoint cannot have datamart.ready=true "
                    "(DM lag breaks Tier 1 hourly SLAs — see source_of_truth.md §6)",
                )
            )

        if isinstance(dm, dict):
            dm_enabled = bool(dm.get("ready")) or any(
                bool(v) for v in (dm.get("tenant_overrides") or {}).values()
            )
            query_file = dm.get("query_file")
            if dm_enabled and query_file and not (root / query_file).exists():
                failures.append(
                    (
                        path.name,
                        f"DataMart is enabled but query_file does not exist: {query_file}",
                    )
                )
            elif (
                not dm_enabled
                and dm.get("root_field")
                and query_file
                and not (root / query_file).exists()
            ):
                # Shadow tooling reads the query even while ready=false, so a mapped root whose
                # file is missing breaks silently until someone tries to shadow-compare it.
                failures.append(
                    (
                        path.name,
                        f"DataMart root_field is set but query_file does not exist: {query_file}",
                    )
                )
            normalizer = dm.get("join_normalizer")
            if dm_enabled and isinstance(normalizer, str) and normalizer not in NORMALIZER_REGISTRY:
                failures.append(
                    (
                        path.name,
                        f"DataMart is enabled but join_normalizer is not registered: {normalizer}",
                    )
                )

    return failures, checked


def main() -> None:
    console = Console()
    root = _repo_root()
    endpoints_dir = root / "config" / "endpoints"

    if not endpoints_dir.is_dir():
        console.print(f"[red]Endpoints dir not found:[/red] {endpoints_dir}")
        sys.exit(2)

    try:
        failures, checked = validate_configs(endpoints_dir, root)
    except FileNotFoundError as exc:
        console.print(f"[red]{exc}[/red]")
        sys.exit(2)

    if failures:
        table = Table(title="Endpoint config validation failures", show_lines=False)
        table.add_column("File", style="cyan", no_wrap=True)
        table.add_column("Error", style="red")
        for name, msg in failures:
            table.add_row(name, msg)
        console.print(table)
        console.print(f"[red]FAIL[/red] — {len(failures)} error(s) across {checked} file(s).")
        sys.exit(1)

    console.print(
        f"[green]OK[/green] — validated {checked} endpoint config file(s) against schema."
    )


if __name__ == "__main__":
    main()
