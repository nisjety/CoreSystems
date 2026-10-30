#!/usr/bin/env python3
"""Gate: does silver hold as many sales orders per company as Visma does? Read-only.

Why this exists: until 2026-10-10 `visma_silver.fact_sales_order` kept one row per
(tenant, orderId), but Visma numbers orders per order type, so a quote and the order made from it
share a number. Silver silently dropped 967 orders. v1 has the same SQL, so the parity gate
(v1 against v2) could not see it.
Visma itself can: its SalesOrder list reports how many orders a company has.

Rule, per tenant:
  missing  = Visma's totalCount - silver's order count
  changed  = Visma's totalCount with modifiedSince = the last sync (orders created or changed since)
  FAIL  when missing > changed: more orders are missing than the sync can still be behind on.
  WARN  when silver has more than Visma: orders deleted in Visma since the last full pass stay in
        silver until that pass reconciles them. Not a failure; the number is reported.
  PASS  otherwise.

The list API ignores an order-type filter (tested: `type` and `orderType` both return the company
total), so the check is per company, not per type.

Last sync: MAX(finished_at) of the heartbeat table for sales_orders_v3. Until a sync has written
heartbeats, the last staging write (MAX(updated_at)) stands in, and the report says so.

Visma: GET only, one request at a time, paced by the population gate's limiter (0.3 s after each
response). Tokens come from `vsv2 token --scope sales-order` and are never printed.

Usage: scripts/verify_sales_order_counts.py [--tenants id,id] [--date YYYY-MM-DD]
Writes docs/gates/sales_order_counts_<date>.md and .json. Exit 0 only if no tenant fails.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass
from datetime import UTC, date, datetime
from pathlib import Path

from google.api_core.exceptions import NotFound
from google.cloud import bigquery

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import tenant_scan_compare as tenants  # noqa: E402
from verify_population_vs_visma import PACER  # noqa: E402

PROJECT = "aquatiq-datavarehus"
LOCATION = "europe-north2"
SILVER = f"{PROJECT}.visma_silver.fact_sales_order"
STAGING = f"{PROJECT}.dw_staging_visma.sales_orders_v3"
HEARTBEAT = f"{PROJECT}.dw_staging_visma._sync_heartbeat"
ENDPOINT = "sales_orders_v3"
BASE = "https://salesorder.visma.net/api/v3/SalesOrders"
GATES = ROOT / "docs" / "gates"


@dataclass(frozen=True)
class Verdict:
    word: str  # PASS | WARN | FAIL
    reason: str


def judge(visma_total: int, changed: int, silver_total: int) -> Verdict:
    """The rule from the module docstring, on the three counts of one tenant."""
    missing = visma_total - silver_total
    if missing > changed:
        return Verdict(
            "FAIL",
            f"{missing:,} orders missing from silver, more than the {changed:,} created or "
            "changed in Visma since the last sync",
        )
    if missing < 0:
        return Verdict(
            "WARN",
            f"silver has {-missing:,} more than Visma (deleted in Visma since the last full pass)",
        )
    return Verdict(
        "PASS", f"{missing:,} missing, within the {changed:,} changed since the last sync"
    )


@dataclass(frozen=True)
class Row:
    tenant: str
    name: str
    visma_total: int
    changed: int
    silver_total: int
    last_sync: str
    last_sync_source: str
    verdict: str
    reason: str


def token(tenant: str) -> str:
    """A SalesOrder token. The CLI reads its .env from the v2 root, hence cwd."""
    out = subprocess.run(
        [str(ROOT / "target/debug/vsv2"), "token", "--scope", "sales-order", "--tenant", tenant],
        capture_output=True,
        text=True,
        check=True,
        cwd=ROOT,
    ).stdout
    return out.strip().splitlines()[-1]


def visma_count(bearer: str, since: datetime | None) -> int:
    """Visma's totalCount for the company, optionally only orders modified since `since`."""
    params = {"pageSize": 1, "expand": "lines"}
    if since is not None:
        params["modifiedSince"] = since.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    request = urllib.request.Request(
        f"{BASE}?{urllib.parse.urlencode(params)}",
        headers={"Authorization": f"Bearer {bearer}", "Accept": "application/json"},
    )

    def call() -> int:
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                return int(json.loads(response.read())["totalCount"])
        except urllib.error.HTTPError as error:  # the message never carries the header
            raise RuntimeError(f"SalesOrder list: HTTP {error.code}") from None

    return PACER.run("GET", call)


def query(client: bigquery.Client, sql: str) -> list[dict]:
    return [dict(row.items()) for row in client.query(sql, location=LOCATION).result()]


def last_syncs(client: bigquery.Client) -> tuple[dict[str, datetime], str]:
    """Per tenant, the time silver's sales orders were last brought up to date, and its source."""
    try:
        rows = query(
            client,
            f"SELECT tenant_id, MAX(finished_at) AS last_at FROM `{HEARTBEAT}` "
            f"WHERE endpoint = '{ENDPOINT}' AND status = 'succeeded' GROUP BY tenant_id",
        )
        if rows:
            return {r["tenant_id"]: r["last_at"] for r in rows}, "heartbeat"
    except NotFound:
        pass
    rows = query(
        client, f"SELECT tenant_id, MAX(updated_at) AS last_at FROM `{STAGING}` GROUP BY tenant_id"
    )
    return {r["tenant_id"]: r["last_at"] for r in rows}, "last staging write (no heartbeat yet)"


def report(rows: list[Row], day: str) -> Path:
    GATES.mkdir(parents=True, exist_ok=True)
    md = GATES / f"sales_order_counts_{day}.md"
    lines = [
        f"# Sales orders per company: Visma against silver ({day})",
        "",
        "Rule: FAIL when more orders are missing from silver than Visma reports as created or",
        "changed since the last sync. See scripts/verify_sales_order_counts.py.",
        "",
        "| Tenant | Visma | Silver | Missing | Changed since sync | Last sync (source) "
        "| Verdict | Why |",
        "|---|---:|---:|---:|---:|---|---|---|",
    ]
    for r in rows:
        missing = r.visma_total - r.silver_total
        lines.append(
            f"| {r.name} | {r.visma_total:,} | {r.silver_total:,} | {missing:,} "
            f"| {r.changed:,} | {r.last_sync} ({r.last_sync_source}) | {r.verdict} | {r.reason} |"
        )
    md.write_text("\n".join(lines) + "\n")
    md.with_suffix(".json").write_text(json.dumps([asdict(r) for r in rows], indent=1))
    return md


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--tenants", default="", help="comma-separated tenant ids (default: all)")
    ap.add_argument("--date", default=date.today().isoformat(), help="the date in the report name")
    args = ap.parse_args()
    chosen = {t for t in args.tenants.split(",") if t}

    client = bigquery.Client(project=PROJECT)
    silver = {
        r["tenant_id"]: r["n"]
        for r in query(
            client, f"SELECT tenant_id, COUNT(*) AS n FROM `{SILVER}` GROUP BY tenant_id"
        )
    }
    syncs, source = last_syncs(client)

    rows: list[Row] = []
    for tenant, name, _kind in tenants.TENANTS:
        if chosen and tenant not in chosen:
            continue
        since = syncs.get(tenant)
        bearer = PACER.run("token", lambda tenant=tenant: token(tenant))
        total = visma_count(bearer, None)
        changed = visma_count(bearer, since) if since is not None else total
        verdict = judge(total, changed, silver.get(tenant, 0))
        rows.append(
            Row(
                tenant,
                name,
                total,
                changed,
                silver.get(tenant, 0),
                since.isoformat() if since else "none",
                source,
                verdict.word,
                verdict.reason,
            )
        )
        held = silver.get(tenant, 0)
        print(
            f"{name:30} visma {total:>7,} silver {held:>7,} changed {changed:>6,}  {verdict.word}"
        )

    md = report(rows, args.date)
    failed = [r for r in rows if r.verdict == "FAIL"]
    print(f"Visma calls: {dict(PACER.calls)}")
    print(f"wrote {md} (exit {1 if failed else 0})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
