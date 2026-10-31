"""Rate-limit + pagination probe — Phase 0 deliverable.

Pulls `pages` pages of `accounts` and `customers` from DataMart, measures
wall-clock per page, captures any `Retry-After`, and writes a Markdown
report to `out_path`.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from pathlib import Path

from rich import print as rprint

from .._http import DatamartGraphQL, OAuth, VismaEnv

_QUERIES = {
    "accounts": """
        query Accounts($pageSize: Int! = 500, $lastId: String) {
          accounts(
            pageSize: $pageSize
            lastId: $lastId
          ) {
            id
            companyId
          }
        }
    """,
    "customers": """
        query Customers($pageSize: Int! = 500, $lastId: String) {
          customers(
            pageSize: $pageSize
            lastId: $lastId
          ) {
            id
            companyId
          }
        }
    """,
}


@dataclass
class _PageStat:
    page: int
    items: int
    elapsed_ms: int
    error: str | None = None


@dataclass
class _Probe:
    entity: str
    page_size: int
    pages_target: int
    pages: list[_PageStat] = field(default_factory=list)
    total_ms: int = 0


def run(pages: int, out_path: str) -> None:
    env = VismaEnv.from_env()

    oauth = OAuth(env)
    gql = DatamartGraphQL(env, oauth)

    page_size = 500
    rprint(f"[cyan]→[/cyan] probing accounts + customers — {pages} pages x {page_size}/page")

    probes: list[_Probe] = []
    try:
        for entity, query in _QUERIES.items():
            probe = _Probe(entity=entity, page_size=page_size, pages_target=pages)
            last_id: str | None = None
            for page in range(1, pages + 1):
                started = time.perf_counter()
                try:
                    data = gql.execute(
                        query,
                        {
                            "pageSize": page_size,
                            "lastId": last_id,
                        },
                    )
                except Exception as exc:
                    elapsed = int((time.perf_counter() - started) * 1000)
                    probe.pages.append(_PageStat(page, 0, elapsed, str(exc)))
                    probe.total_ms += elapsed
                    rprint(f"  [red]✗[/red] {entity} p{page} {elapsed} ms — {exc}")
                    break
                elapsed = int((time.perf_counter() - started) * 1000)
                rows = data.get(entity) or []
                probe.pages.append(_PageStat(page, len(rows), elapsed))
                probe.total_ms += elapsed
                rprint(
                    f"  {'[yellow]·[/yellow]' if rows else '[dim]·[/dim]'} "
                    f"{entity} p{page} {len(rows):>4} rows {elapsed:>5} ms"
                )
                if not rows:
                    break
                last_id = (rows[-1] or {}).get("id")
                if len(rows) < page_size:
                    break
            probes.append(probe)
    finally:
        gql.close()
        oauth.close()

    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(_render_report(env, probes))
    rprint(f"[green]✓[/green] report → {out}")


def _render_report(env: VismaEnv, probes: list[_Probe]) -> str:
    lines: list[str] = [
        "# Phase 0 — DataMart probe",
        "",
        f"- tenant_id: `{env.tenant_id}`",
        f"- endpoint:  `{env.datamart_url}`",
        "",
    ]
    for probe in probes:
        lines.append(
            f"## `{probe.entity}` — {probe.page_size}/page x target {probe.pages_target} pages"
        )
        lines.append("")
        lines.append("| page | items | ms | error |")
        lines.append("|---|---:|---:|---|")
        for s in probe.pages:
            lines.append(f"| {s.page} | {s.items} | {s.elapsed_ms} | {s.error or ''} |")
        rps = (sum(p.items for p in probe.pages) / max(probe.total_ms, 1)) * 1000
        lines.append(f"\n_total: {probe.total_ms} ms · {rps:.1f} records/s_")
        lines.append("")
    lines.append("## Recommended initial budget")
    lines.append("")
    lines.append("- per-minute: 60 _(tune if probe shows headroom)_")
    lines.append("- per-day:    10 000")
    lines.append("")
    return "\n".join(lines)
