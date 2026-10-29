#!/usr/bin/env python3
"""Spot check: do the rows in BigQuery staging match what Visma returns for them now?

For a handful of endpoints that Visma can answer by key, it takes a few rows per tenant from
`dw_staging_visma` (chosen by a hash of the key, so the same run picks the same rows), reads each
one from Visma by its key, and compares the scalar fields both sides have. A row that is gone
(HTTP 404) was deleted in Visma since the fetch; a field that differs is usually a row edited since
(its `lastModifiedDateTime` differs too), and the report says which.

This reads only: `bq` queries and GET requests with a token from `vsv2 token`, a few requests
per tenant, 0.3 s apart. It never prints a token.

Usage: scripts/verify_bq_vs_visma.py [--per 4] [--endpoints a,b] [--tenants id,id]
Writes docs/bq_vs_visma_spotcheck.md for a run without --endpoints and --tenants.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

from google.cloud import bigquery

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import tenant_scan_compare as tenants  # noqa: E402
from _bq_target import DATASET, PROJECT  # noqa: E402

OUT = ROOT / "docs/bq_vs_visma_spotcheck.md"
BASE = "https://api.finance.visma.net"
# endpoint -> path of the single record, with the key fields in braces (in primary-key order)
BY_KEY = {
    "customer_invoices": "/v1/customerinvoice/{referenceNumber}",
    "shipments": "/v1/shipment/{shipmentNumber}",
    "purchase_orders": "/v1/purchaseorder/{orderNbr}",
    "inventory_issues": "/v1/inventoryissue/{referenceNumber}",
    "cash_transactions": "/v1/cashTransaction/{referenceNbr}",
    "suppliers": "/v1/supplier/{number}",
    "customers": "/v1/customer/{number}",
    "accounts": "/v1/account/{accountCD}",
    "projects": "/v1/project/{projectID}",
}
IGNORED = ("_", "tenant_id", "metadata")


def token(tenant: str) -> str:
    out = subprocess.run(
        [str(ROOT / "target/debug/vsv2"), "token", "--scope", "erp", "--tenant", tenant],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return out.strip().splitlines()[-1]


def sample(client: bigquery.Client, endpoint: str, tenant: str, per: int) -> list[dict]:
    sql = (
        f"SELECT TO_JSON_STRING(payload) AS payload FROM "
        f"`{PROJECT}.{DATASET}.{endpoint}` "
        "WHERE tenant_id = @tenant AND deleted_at IS NULL "
        "ORDER BY FARM_FINGERPRINT(record_key) LIMIT @n"
    )
    config = bigquery.QueryJobConfig(
        query_parameters=[
            bigquery.ScalarQueryParameter("tenant", "STRING", tenant),
            bigquery.ScalarQueryParameter("n", "INT64", per),
        ]
    )
    return [json.loads(row.payload) for row in client.query(sql, job_config=config).result()]


def fetch(path: str, bearer: str) -> tuple[int, dict | None]:
    request = urllib.request.Request(
        BASE + path, headers={"Authorization": f"Bearer {bearer}", "Accept": "application/json"}
    )
    try:
        with urllib.request.urlopen(request, timeout=90) as response:
            body = json.loads(response.read())
            return response.status, body[0] if isinstance(body, list) and body else body
    except urllib.error.HTTPError as error:
        return error.code, None


def differences(stored: dict, live: dict) -> list[str]:
    """Top-level scalar fields both sides have that differ."""
    found = []
    for field, value in stored.items():
        if field.startswith(IGNORED[0]) or field in IGNORED[1:]:
            continue
        if isinstance(value, (dict, list)) or field not in live:
            continue
        other = live[field]
        if isinstance(other, (dict, list)):
            continue
        if value != other:
            found.append(field)
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--per", type=int, default=4, help="rows per tenant and endpoint")
    ap.add_argument("--endpoints", default="")
    ap.add_argument("--tenants", default="")
    args = ap.parse_args()
    endpoints = [e for e in args.endpoints.split(",") if e] or list(BY_KEY)
    chosen = {t for t in args.tenants.split(",") if t}
    client = bigquery.Client(project=PROJECT)

    totals: dict[str, Counter] = defaultdict(Counter)
    fields: dict[str, Counter] = defaultdict(Counter)
    examples: list[str] = []
    for tenant, name, _kind in tenants.TENANTS:
        if chosen and tenant not in chosen:
            continue
        bearer = token(tenant)
        for endpoint in endpoints:
            for stored in sample(client, endpoint, tenant, args.per):
                path = BY_KEY[endpoint]
                for key in [p.split("}")[0] for p in path.split("{")[1:]]:
                    path = path.replace(
                        "{" + key + "}", urllib.request.quote(str(stored[key]).strip())
                    )
                status, live = fetch(path, bearer)
                time.sleep(0.3)
                counts = totals[endpoint]
                counts["sampled"] += 1
                if status == 404:
                    counts["gone"] += 1
                    continue
                if status != 200 or live is None:
                    counts[f"http_{status}"] += 1
                    continue
                diff = differences(stored, live)
                counts["identical" if not diff else "differs"] += 1
                for field in diff:
                    fields[endpoint][field] += 1
                if diff and len(examples) < 12:
                    changed = "lastModifiedDateTime" in diff
                    examples.append(
                        f"{name} / {endpoint} / {path.rsplit('/', 1)[-1]}: {', '.join(diff[:6])}"
                        f"{' (edited since: lastModifiedDateTime differs)' if changed else ''}"
                    )

    lines = [
        "# BigQuery staging against Visma, row by row",
        "",
        "> Generated by `scripts/verify_bq_vs_visma.py`. A few rows per tenant and endpoint from",
        "> `dw_staging_visma`, read from Visma by key now and compared on the scalar fields both",
        "> sides have. `gone` = 404 in Visma now; `differs` is mostly rows edited after the fetch.",
        "",
        "| Endpoint | Sampled | Identical | Differs | Gone (404) | Other |",
        "|---|---|---|---|---|---|",
    ]
    for endpoint in endpoints:
        c = totals[endpoint]
        other = sum(v for k, v in c.items() if k.startswith("http_"))
        lines.append(
            f"| `{endpoint}` | {c['sampled']} | {c['identical']} | {c['differs']} "
            f"| {c['gone']} | {other} |"
        )
    shown = [(e, f, n) for e in endpoints for f, n in fields[e].most_common(5)]
    if shown:
        lines += ["", "## Fields that differ", ""]
        lines += [f"- `{e}`: `{f}` in {n} row(s)" for e, f, n in shown]
    if examples:
        lines += ["", "## Examples", ""] + [f"- {text}" for text in examples]
    text = "\n".join(lines) + "\n"
    print(text)
    if not (args.endpoints or args.tenants):
        OUT.write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
