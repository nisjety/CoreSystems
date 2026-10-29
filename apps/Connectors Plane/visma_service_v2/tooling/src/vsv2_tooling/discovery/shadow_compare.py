"""Live sample shadow comparison for synchronized DataMart candidates."""

from __future__ import annotations

import itertools
import json
import os
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import httpx
from rich import print as rprint

from vsv2_tooling._http import DatamartGraphQL, OAuth, VismaEnv
from vsv2_tooling.discovery.review_candidates import (
    COMMON_SYSTEM_FIELDS,
    _business_bronze_fields,
    _load_bronze_schema_fields,
)
from vsv2_tooling.normalizers import REGISTRY as NORMALIZER_REGISTRY

# DataMart-only operational/audit fields. These never appear on the ERP
# side, so comparing them always produces a spurious mismatch. They are
# excluded from every endpoint's compare set by default.
DATAMART_META_FIELDS: frozenset[str] = frozenset(
    {
        "companyId",
        "createdInDataMartDateTime",
        "updatedInDataMartDateTime",
        "deletedDatabaseRecord",
    }
)


@dataclass(frozen=True)
class CandidateConfig:
    endpoint: str
    bigquery_table: str
    api_endpoint: str
    pagination_profile: str
    default_query: dict[str, Any]
    required_query: list[str]
    partitioning: dict[str, Any] | None
    primary_key: list[str]
    datamart_query_file: str
    datamart_root_field: str
    datamart_id_field: str
    datamart_normalizer: str | None
    bronze_schema_path: Path
    compare_fields: list[str]


@dataclass(frozen=True)
class ShadowRow:
    endpoint: str
    bigquery_table: str
    status: str
    erp_rows: int
    datamart_rows: int
    overlap_rows: int
    only_in_erp: int
    only_in_datamart: int
    mismatch_fields: list[str]
    erp_query: dict[str, Any]
    note: str | None


class ErpRestProbe:
    """Minimal ERP REST client for ad-hoc candidate shadow comparisons."""

    def __init__(self, env: VismaEnv, oauth: OAuth, *, timeout: float = 60.0) -> None:
        self.env = env
        self.oauth = oauth
        self.scope_salesorder = os.environ.get("VISMA_OAUTH_SCOPE_SALESORDER") or env.scope_erp
        self._client = httpx.Client(timeout=timeout)

    def get_json(self, path: str, params: dict[str, Any]) -> Any:
        url = (
            path
            if path.startswith("http://") or path.startswith("https://")
            else (self.env.erp_base.rstrip("/") + path)
        )
        primary_scope = self._scope_for_url(url)
        fallback_scope = (
            self.env.scope_erp if primary_scope == self.scope_salesorder else self.scope_salesorder
        )
        for scope in (primary_scope, fallback_scope):
            token = self.oauth.token_for(scope=scope, tenant_id=self.env.tenant_id)
            resp = self._client.get(
                url,
                headers={
                    "Authorization": f"Bearer {token}",
                    "ipp-company-id": self.env.tenant_id,
                    "ipp-application-type": "Visma.net Financials",
                },
                params=_flatten_query(params),
            )
            if resp.status_code == 401 and scope != fallback_scope:
                continue
            if resp.status_code >= 400:
                raise RuntimeError(f"ERP {resp.status_code}: {resp.text[:400]}")
            return resp.json()
        raise RuntimeError("ERP authentication failed with both primary and fallback scopes")

    def close(self) -> None:
        self._client.close()

    def _scope_for_url(self, url: str) -> str:
        parsed = urlparse(url)
        if parsed.netloc == "salesorder.visma.net":
            return self.scope_salesorder
        if parsed.path == "/api/v3/SalesOrders" or parsed.path.startswith("/api/v3/SalesOrders/"):
            return self.scope_salesorder
        return self.env.scope_erp


def run(
    *,
    endpoint_dir: str,
    bronze_dir: str,
    review_report_path: str,
    out_path: str,
    page_size: int,
    max_pages: int,
) -> None:
    endpoint_dir_path = Path(endpoint_dir)
    bronze_dir_path = Path(bronze_dir)
    candidate_names = _load_shadow_candidate_names(Path(review_report_path))
    configs = _load_candidate_configs(endpoint_dir_path, bronze_dir_path, candidate_names)

    env = VismaEnv.from_env()
    oauth = OAuth(env)
    dm = DatamartGraphQL(env, oauth)
    erp = ErpRestProbe(env, oauth)

    try:
        company_id = _resolve_company_id(env, dm, configs)
        rows = [
            _shadow_compare_candidate(
                endpoint_dir_path.parents[1],
                dm,
                erp,
                cfg,
                page_size=page_size,
                max_pages=max_pages,
                company_id=company_id,
            )
            for cfg in configs
        ]
    finally:
        erp.close()
        dm.close()
        oauth.close()

    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(_render_report(rows))
    clean = sum(1 for row in rows if row.status == "clean_sample")
    rprint(f"[green]✓[/green] shadow-compared {len(rows)} candidate endpoint(s)")
    rprint(f"[green]✓[/green] clean_sample: {clean}")
    rprint(f"[green]✓[/green] report → {out}")


def _load_shadow_candidate_names(report_path: Path) -> list[str]:
    names: list[str] = []
    in_summary = False
    for raw_line in report_path.read_text().splitlines():
        line = raw_line.strip()
        if line == "## Summary":
            in_summary = True
            continue
        if in_summary and line.startswith("## "):
            break
        if not in_summary or not line.startswith("| `"):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) < 7:
            continue
        endpoint = cells[0].strip("`")
        status = cells[6].strip("`")
        if status == "candidate_for_shadow":
            names.append(endpoint)
    return names


def _load_candidate_configs(
    endpoint_dir: Path,
    bronze_dir: Path,
    candidate_names: list[str],
) -> list[CandidateConfig]:
    configs: list[CandidateConfig] = []
    for endpoint in candidate_names:
        path = endpoint_dir / f"{endpoint}.json"
        data = json.loads(path.read_text())
        dm = data["datamart"]
        primary_key = data["primary_key"]
        bronze_schema_path = bronze_dir / f"{data['bigquery_table']}.sqlx"
        bronze_fields = _business_bronze_fields(_load_bronze_schema_fields(bronze_schema_path))
        compare_excludes = set(dm.get("compare_excludes") or []) | DATAMART_META_FIELDS
        configs.append(
            CandidateConfig(
                endpoint=endpoint,
                bigquery_table=data["bigquery_table"],
                api_endpoint=data["api_endpoint"],
                pagination_profile=data["pagination_profile"],
                default_query=data.get("default_query") or {},
                required_query=data.get("required_query") or [],
                partitioning=data.get("partitioning"),
                primary_key=primary_key if isinstance(primary_key, list) else [primary_key],
                datamart_query_file=dm["query_file"],
                datamart_root_field=dm["root_field"],
                datamart_id_field=dm["id_field"],
                datamart_normalizer=dm.get("join_normalizer"),
                bronze_schema_path=bronze_schema_path,
                compare_fields=[
                    field
                    for field in bronze_fields
                    if field not in COMMON_SYSTEM_FIELDS and field not in compare_excludes
                ],
            )
        )
    return configs


def _resolve_company_id(
    env: VismaEnv,
    dm: DatamartGraphQL,
    configs: list[CandidateConfig],
) -> int | None:
    if not any(
        _query_requires_company_id(Path(cfg.datamart_query_file).read_text()) for cfg in configs
    ):
        return None
    if env.company_id:
        try:
            return int(env.company_id)
        except ValueError:
            return None
    for root_field in ("accounts", "branches", "customers", "locations"):
        query = f"query DiscoverCompanyIds {{ {root_field}(pageSize: 100) {{ companyId }} }}"
        try:
            data = dm.execute(query, {"pageSize": 100})
        except Exception:
            continue
        rows = data.get(root_field) or []
        for row in rows:
            if isinstance(row, dict) and row.get("companyId") is not None:
                try:
                    return int(row["companyId"])
                except (TypeError, ValueError):
                    continue
    return None


def _shadow_compare_candidate(
    repo_root: Path,
    dm: DatamartGraphQL,
    erp: ErpRestProbe,
    cfg: CandidateConfig,
    *,
    page_size: int,
    max_pages: int,
    company_id: int | None,
) -> ShadowRow:
    try:
        dm_records, dm_note = _fetch_datamart_records(
            repo_root,
            dm,
            cfg,
            page_size=page_size,
            max_pages=max_pages,
            company_id=company_id,
        )
    except Exception as exc:
        return ShadowRow(
            endpoint=cfg.endpoint,
            bigquery_table=cfg.bigquery_table,
            status="probe_error",
            erp_rows=0,
            datamart_rows=0,
            overlap_rows=0,
            only_in_erp=0,
            only_in_datamart=0,
            mismatch_fields=[],
            erp_query={},
            note=f"DataMart probe failed: {exc}",
        )

    try:
        erp_records, erp_query, erp_note = _fetch_erp_records(
            erp,
            cfg,
            page_size=page_size,
            max_pages=max_pages,
        )
    except Exception as exc:
        return ShadowRow(
            endpoint=cfg.endpoint,
            bigquery_table=cfg.bigquery_table,
            status="probe_error",
            erp_rows=0,
            datamart_rows=len(dm_records),
            overlap_rows=0,
            only_in_erp=0,
            only_in_datamart=0,
            mismatch_fields=[],
            erp_query={},
            note=f"ERP probe failed: {exc}",
        )

    dm_by_key: dict[str, dict[str, Any]] = {}
    for record in dm_records:
        key = _record_key(record, cfg.primary_key)
        if key is not None:
            dm_by_key.setdefault(key, record)

    erp_by_key: dict[str, dict[str, Any]] = {}
    for record in erp_records:
        key = _record_key(record, cfg.primary_key)
        if key is not None:
            erp_by_key.setdefault(key, record)

    overlap = sorted(set(dm_by_key) & set(erp_by_key))
    only_in_erp = sorted(set(erp_by_key) - set(dm_by_key))
    only_in_dm = sorted(set(dm_by_key) - set(erp_by_key))

    mismatch_fields = sorted(
        {
            field
            for key in overlap
            for field in cfg.compare_fields
            if not _values_equal(
                _get_path(erp_by_key[key], field),
                _get_path(dm_by_key[key], field),
            )
        }
    )

    note = "; ".join(part for part in [dm_note, erp_note] if part) or None
    status = _classify_shadow_status(
        erp_rows=len(erp_records),
        datamart_rows=len(dm_records),
        overlap_rows=len(overlap),
        only_in_erp=len(only_in_erp),
        only_in_datamart=len(only_in_dm),
        mismatch_fields=mismatch_fields,
    )

    return ShadowRow(
        endpoint=cfg.endpoint,
        bigquery_table=cfg.bigquery_table,
        status=status,
        erp_rows=len(erp_records),
        datamart_rows=len(dm_records),
        overlap_rows=len(overlap),
        only_in_erp=len(only_in_erp),
        only_in_datamart=len(only_in_dm),
        mismatch_fields=mismatch_fields,
        erp_query=erp_query,
        note=note,
    )


def _fetch_datamart_records(
    repo_root: Path,
    dm: DatamartGraphQL,
    cfg: CandidateConfig,
    *,
    page_size: int,
    max_pages: int,
    company_id: int | None,
) -> tuple[list[dict[str, Any]], str | None]:
    query_path = repo_root / cfg.datamart_query_file
    query_text = query_path.read_text()
    variables: dict[str, Any] = {
        "pageSize": page_size,
        "lastId": None,
        "updatedInDataMart": None,
        "updatedSince": None,
    }
    if _query_requires_company_id(query_text):
        variables["companyId"] = company_id
    note: str | None = None
    out: list[dict[str, Any]] = []
    for _ in range(max_pages):
        data = dm.execute(query_text, variables)
        rows = data.get(cfg.datamart_root_field) or []
        if not isinstance(rows, list) or not rows:
            break
        normalized_rows = _normalize_rows(rows, cfg.datamart_normalizer)
        out.extend(normalized_rows)
        if len(rows) < page_size:
            break
        last_id = rows[-1].get(cfg.datamart_id_field) if isinstance(rows[-1], dict) else None
        if not last_id:
            note = "DataMart pagination stopped: id_field missing on last row"
            break
        variables["lastId"] = last_id
    return out, note


def _normalize_rows(
    records: list[dict[str, Any]],
    normalizer_name: str | None,
) -> list[dict[str, Any]]:
    if not normalizer_name:
        return records
    fn = NORMALIZER_REGISTRY[normalizer_name]
    return [fn(record) for record in records]


def _fetch_erp_records(
    erp: ErpRestProbe,
    cfg: CandidateConfig,
    *,
    page_size: int,
    max_pages: int,
) -> tuple[list[dict[str, Any]], dict[str, Any], str | None]:
    probe_params, probe_note = _resolve_probe_query(erp, cfg)
    if probe_params is None:
        return [], {}, probe_note
    if (
        "lastModifiedDateTime" in probe_params
        and "lastModifiedDateTimeCondition" not in probe_params
    ):
        probe_params["lastModifiedDateTimeCondition"] = ">="

    out: list[dict[str, Any]] = []
    page = 1 if cfg.pagination_profile == "finance_page_number" else 0
    for _ in range(max_pages):
        params = dict(probe_params)
        params["pageSize"] = page_size
        if cfg.pagination_profile in {"salesorder_page_index", "journaltx_page_index"}:
            params["pageIndex"] = page
        else:
            params["pageNumber"] = page
        resp = erp.get_json(cfg.api_endpoint, params)
        items, has_next = _extract_erp_page(resp)
        out.extend(
            _normalize_erp_record(cfg.endpoint, item) for item in items if isinstance(item, dict)
        )
        if not items:
            break
        if has_next is False or len(items) < page_size:
            break
        page += 1
    return out, probe_params, probe_note


def _normalize_erp_record(endpoint: str, record: dict[str, Any]) -> dict[str, Any]:
    if endpoint == "journal_transactions":
        batch_nbr = record.get("batchNbr")
        batch_number = record.get("batchNumber")
        if batch_nbr is not None and batch_number is None:
            return {**record, "batchNumber": batch_nbr}
        if batch_number is not None and batch_nbr is None:
            return {**record, "batchNbr": batch_number}
    return record


def _resolve_probe_query(
    erp: ErpRestProbe,
    cfg: CandidateConfig,
) -> tuple[dict[str, Any] | None, str | None]:
    if cfg.endpoint == "general_ledger_balances_v2":
        return {"periodId": datetime.now().strftime("%Y%m")}, None

    params = dict(cfg.default_query)
    partitioning = cfg.partitioning
    required = set(cfg.required_query)
    if required and required.issubset(params):
        return params, None
    if not partitioning:
        missing = sorted(required - set(params))
        if missing:
            return None, f"missing required ERP query params: {', '.join(missing)}"
        return params, None

    dimensions = partitioning.get("dimensions") or []
    values_by_param: list[tuple[str, list[Any]]] = []
    for dim in dimensions:
        query_param = dim["query_param"]
        if query_param in params:
            values_by_param.append((query_param, [params[query_param]]))
            continue
        values = _resolve_partition_values(erp, dim)
        if not values:
            values = list(dim.get("fallback_values") or [])
        if not values:
            return None, f"no values resolved for partition dimension {query_param}"
        values_by_param.append((query_param, values))

    attempted = 0
    first_combo: dict[str, Any] | None = None
    for combo in itertools.product(*(values for _, values in values_by_param)):
        candidate = dict(params)
        for (query_param, _), value in zip(values_by_param, combo, strict=False):
            candidate[query_param] = value
        first_combo = first_combo or candidate
        attempted += 1
        try:
            probe_response = erp.get_json(
                cfg.api_endpoint,
                _page_probe_params(candidate, cfg.pagination_profile),
            )
        except Exception as exc:
            return None, str(exc)
        items, _has_next = _extract_erp_page(probe_response)
        if items:
            return candidate, None

    if first_combo is None:
        return None, "no partition combinations available"
    return first_combo, f"all {attempted} partition probe combination(s) returned zero rows"


def _resolve_partition_values(erp: ErpRestProbe, dim: dict[str, Any]) -> list[Any]:
    source_path = dim["source_path"]
    response = erp.get_json(
        dim["source_endpoint"],
        {"pageSize": 200, "pageNumber": 1},
    )
    items, _has_next = _extract_erp_page(response)
    values: list[Any] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            continue
        value = _get_path(item, source_path)
        marker = _canonical_for_key(value)
        if marker is None or marker in seen:
            continue
        seen.add(marker)
        values.append(value)
    return values


def _page_probe_params(params: dict[str, Any], profile: str) -> dict[str, Any]:
    probe = dict(params)
    probe["pageSize"] = 1
    if profile == "salesorder_page_index" or profile == "journaltx_page_index":
        probe["pageIndex"] = 0
    else:
        probe["pageNumber"] = 1
    return probe


def _extract_erp_page(resp: Any) -> tuple[list[Any], bool | None]:
    if isinstance(resp, list):
        return resp, None
    if isinstance(resp, dict):
        for key in ("value", "items", "data", "results", "records"):
            value = resp.get(key)
            if isinstance(value, list):
                next_page = resp.get("nextPage")
                has_next = None
                if next_page is None:
                    has_next = False if key == "value" and "nextPage" in resp else None
                elif isinstance(next_page, str):
                    has_next = bool(next_page.strip())
                else:
                    has_next = True
                return value, has_next
    raise RuntimeError(f"unexpected ERP response shape: {type(resp).__name__}")


def _record_key(record: dict[str, Any], primary_key: list[str]) -> str | None:
    parts: list[str] = []
    for path in primary_key:
        value = _get_path(record, path)
        marker = _canonical_for_key(value)
        if marker is None:
            return None
        parts.append(marker)
    return "\x1f".join(parts)


def _get_path(record: Any, path: str) -> Any:
    current = record
    for part in path.split("."):
        current = _coerce_jsonish(current)
        if not isinstance(current, dict) or part not in current:
            return None
        current = current[part]
    return current


def _coerce_jsonish(value: Any) -> Any:
    if isinstance(value, str):
        stripped = value.strip()
        if stripped.startswith("{") or stripped.startswith("["):
            try:
                return json.loads(stripped)
            except json.JSONDecodeError:
                return value
    return value


def _canonical_for_key(value: Any) -> str | None:
    if value in (None, "", [], {}):
        return None
    value = _coerce_jsonish(value)
    if isinstance(value, (dict, list)):
        return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return str(value)


def _values_equal(left: Any, right: Any) -> bool:
    left = _coerce_jsonish(left)
    right = _coerce_jsonish(right)
    if isinstance(left, (dict, list)) or isinstance(right, (dict, list)):
        return json.dumps(left, sort_keys=True, separators=(",", ":"), ensure_ascii=False) == (
            json.dumps(right, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        )
    # ERP often returns enum values as CHAR(N) padded with trailing spaces
    # (e.g. `'FA   '`) while DM returns the trimmed value (`'FA'`). Treat
    # trailing whitespace as insignificant when both sides are strings.
    if isinstance(left, str) and isinstance(right, str):
        return left.rstrip() == right.rstrip()
    return left == right


def _classify_shadow_status(
    *,
    erp_rows: int,
    datamart_rows: int,
    overlap_rows: int,
    only_in_erp: int,
    only_in_datamart: int,
    mismatch_fields: list[str],
) -> str:
    if erp_rows == 0 and datamart_rows == 0:
        return "no_live_rows"
    if erp_rows == 0:
        return "no_erp_rows"
    if datamart_rows == 0:
        return "no_datamart_rows"
    if overlap_rows == 0:
        return "no_key_overlap"
    if mismatch_fields:
        return "field_mismatch"
    if only_in_erp or only_in_datamart:
        return "key_gap"
    return "clean_sample"


def _flatten_query(params: dict[str, Any]) -> list[tuple[str, str]]:
    flat: list[tuple[str, str]] = []
    for key, value in sorted(params.items()):
        if value is None:
            continue
        if isinstance(value, list):
            for item in value:
                flat.append((key, str(item)))
        else:
            flat.append((key, str(value)))
    return flat


def _query_requires_company_id(query_text: str) -> bool:
    return "$companyId" in query_text


def _render_report(rows: list[ShadowRow]) -> str:
    lines = [
        "# DataMart Shadow Comparison",
        "",
        "Live sample shadow comparison between ERP REST and normalized DataMart",
        "candidate endpoints for the current tenant.",
        "",
        f"- endpoints compared: {len(rows)}",
        f"- clean_sample: {sum(1 for row in rows if row.status == 'clean_sample')}",
        f"- probe_error: {sum(1 for row in rows if row.status == 'probe_error')}",
        f"- key_gap: {sum(1 for row in rows if row.status == 'key_gap')}",
        f"- field_mismatch: {sum(1 for row in rows if row.status == 'field_mismatch')}",
        f"- no_key_overlap: {sum(1 for row in rows if row.status == 'no_key_overlap')}",
        f"- no_live_rows: {sum(1 for row in rows if row.status == 'no_live_rows')}",
        f"- no_datamart_rows: {sum(1 for row in rows if row.status == 'no_datamart_rows')}",
        f"- no_erp_rows: {sum(1 for row in rows if row.status == 'no_erp_rows')}",
        "",
        (
            "| endpoint | status | erp_rows | datamart_rows | overlap | "
            "only_in_erp | only_in_dm | mismatch_fields |"
        ),
        "|---|---|---:|---:|---:|---:|---:|---|",
    ]
    for row in rows:
        mismatch_fields = ", ".join(f"`{field}`" for field in row.mismatch_fields[:8]) or "-"
        lines.append(
            f"| `{row.endpoint}` | `{row.status}` | {row.erp_rows} | {row.datamart_rows} | "
            f"{row.overlap_rows} | {row.only_in_erp} | {row.only_in_datamart} | {mismatch_fields} |"
        )

    lines.append("")
    for row in rows:
        lines.append(f"## `{row.endpoint}`")
        lines.append("")
        lines.append(f"- status: `{row.status}`")
        lines.append(f"- ERP rows fetched: {row.erp_rows}")
        lines.append(f"- DataMart rows fetched: {row.datamart_rows}")
        lines.append(f"- overlapping keys: {row.overlap_rows}")
        lines.append(f"- only in ERP sample: {row.only_in_erp}")
        lines.append(f"- only in DataMart sample: {row.only_in_datamart}")
        lines.append(f"- ERP probe query: `{json.dumps(row.erp_query, sort_keys=True)}`")
        if row.mismatch_fields:
            lines.append(
                "- mismatched fields: " + ", ".join(f"`{field}`" for field in row.mismatch_fields)
            )
        if row.note:
            lines.append(f"- note: {row.note}")
        lines.append("")
    return "\n".join(lines)
