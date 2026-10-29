#!/usr/bin/env python3
"""Probe unconfigured ERP REST endpoints and generate a working config for each.

For every `name=/path` it fetches a first page (read-only), works out how the endpoint
paginates, finds a natural unique key from the sample, and notes whether rows carry
`lastModifiedDateTime`. With --write it saves `config/endpoints/<name>.json`.

The key is proposed from a sample only, so a full `scripts/ingest_probe.py` run is still what
proves it: the executor rejects a batch that contains two different rows for one key.

Usage:
  scripts/discover_erp_endpoints.py                 # dry run over the built-in list
  scripts/discover_erp_endpoints.py --write         # also write the configs
  scripts/discover_erp_endpoints.py -e rooms=/v1/room --write
"""

from __future__ import annotations

import argparse
import itertools
import json
import re
import sys
from pathlib import Path
from typing import Any

import httpx

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tooling/src"))

from vsv2_tooling._http import OAuth, VismaEnv  # noqa: E402
from vsv2_tooling.discovery.shadow_compare import ErpRestProbe  # noqa: E402

# name -> (path, fixed query). Everything Visma's live Swagger lists as an ERP collection that no
# config covered, minus deprecated duplicates (salesorder v1/v2, creditNote) and tooling endpoints
# that carry no business data (blob URLs, first-time-startup, UI screens).
DEFAULTS: dict[str, tuple[str, dict[str, str]]] = {
    "customer_credit_write_offs": ("/v1/customerCreditWriteOff", {}),
    "inventory_receipt_transfers": ("/v1/inventoryReceipt/transfers", {}),
    "purchase_receipt_transfers": ("/v2/PurchaseReceipt/transfers", {}),
    "discounts": ("/v2/discount", {}),
    "countries": ("/v1/country", {}),
    "currency_rate_types": ("/v2/currencyratetype", {}),
    "inventory_item_classes": ("/v1/inventory/itemClass", {}),
    "inventory_item_post_classes": ("/v1/inventory/itemPostClass", {}),
    "customer_balances": ("/v1/customer/balance", {}),
    "customer_balances_v2": (
        "/v2/customer/balance",
        {"fromFinPeriod": "200001", "toFinPeriod": "203012"},
    ),
    "supplier_balances_v2": (
        "/v2/supplier/balance",
        {"fromFinPeriod": "200001", "toFinPeriod": "203012"},
    ),
    "supplier_inbox_rules": ("/v1/supplier/inboxrules", {}),
    "languages": ("/v1/multilanguage/languages", {}),
}

# Keys a sample cannot reveal. A composite is required when the ERP numbers documents per type.
KEY_OVERRIDES: dict[str, list[str]] = {
    "discounts": ["discountCode", "series"],
    "customer_balances_v2": ["branchNumber.number", "customer.internalId"],
    "supplier_balances_v2": ["branch", "supplier.internalId"],
}

# Measured and deliberately left out (see docs/endpoint_matrix.md notes and the memory file).
NOT_CONFIGURED: dict[str, tuple[str, str]] = {
    "payments": (
        "/v1/payment",
        "returns exactly the rows of /v1/customerPayment (21,391 shared rows, 0 differing), "
        "which `customer_payments` already reads",
    ),
    "discounts_v1": ("/v1/discount", "returns exactly the rows of /v2/discount (129, 0 differing)"),
    "supplier_balances": (
        "/v1/supplier/balance",
        "ignores paging and 111 of 1384 rows have an empty supplier object (no key); "
        "/v2/supplier/balance covers the same data",
    ),
    "supplier_po_balances": (
        "/v1/supplier/POBalance",
        "no v2; 111 of 1384 rows have an empty supplier object (no key), and one keyless row "
        "fails the whole endpoint until a quarantine policy exists",
    ),
    "feature_set": (
        "/v1/featuresset",
        "answers with an object holding `features`, which the adapter does not unwrap",
    ),
    "contract_templates": ("/v1/contractTemplate", "0 rows on this tenant, no key verifiable"),
    "contract_usage": ("/v1/contractUsage", "0 rows on this tenant, no key verifiable"),
    "customer_invoice_work_types": (
        "/v1/customerinvoice/worktypes",
        "0 rows on this tenant, no key verifiable",
    ),
    "expense_receipts": (
        "/v1/expenseReceipt",
        "configured by hand: the synchronous read times out, so it uses background mode",
    ),
}

WRAPPER_KEYS = ("value", "items", "data", "results", "records")
KEY_NAME = re.compile(r"(^id$|id$|cd$|number$|nbr$|code$|ref$|reference$)", re.I)
NOT_A_KEY = re.compile(r"date|time|stamp", re.I)
PAGE_SIZE = 100


def extract(body: Any) -> tuple[list[dict], int | None]:
    """Rows and the API's own total, from the response shapes the ERP uses."""
    if isinstance(body, list):
        total = None
        if body and isinstance(body[0], dict):
            total = (body[0].get("metadata") or {}).get("totalCount")
        return [r for r in body if isinstance(r, dict)], total
    if isinstance(body, dict):
        for key in WRAPPER_KEYS:
            if isinstance(body.get(key), list):
                total = body.get("totalCount") or (body.get("metadata") or {}).get("totalCount")
                return [r for r in body[key] if isinstance(r, dict)], total
        return [body], None  # a single object (e.g. feature flags)
    return [], None


def scalar_paths(row: dict) -> dict[str, Any]:
    """Top-level scalars plus one level of nested scalars, keyed by dotted path."""
    out: dict[str, Any] = {}
    for key, value in row.items():
        if key == "metadata":
            continue
        if isinstance(value, dict):
            for sub, inner in value.items():
                if not isinstance(inner, (dict, list)):
                    out[f"{key}.{sub}"] = inner
        elif not isinstance(value, list):
            out[key] = value
    return out


def choose_key(rows: list[dict]) -> list[str] | None:
    """A field (or pair) that is present and unique on every sample row."""
    if not rows:
        return None
    flat = [scalar_paths(r) for r in rows]
    common = [
        p
        for p in flat[0]
        if not NOT_A_KEY.search(p) and all(p in f and f[p] not in (None, "") for f in flat)
    ]
    preferred = [p for p in common if KEY_NAME.search(p.split(".")[-1])]

    def unique(paths: tuple[str, ...]) -> bool:
        seen = {tuple(str(f[p]).strip() for p in paths) for f in flat}
        return len(seen) == len(flat)

    for candidates in (preferred, common):
        for path in candidates:
            if unique((path,)):
                return [path]
    for candidates in (preferred, common):
        for pair in itertools.combinations(candidates[:12], 2):
            if unique(pair):
                return list(pair)
    return None


def probe(probe_client: ErpRestProbe, path: str, query: dict[str, str]) -> dict[str, Any]:
    result: dict[str, Any] = {"path": path}
    for paged in (True, False):
        params = dict(query)
        if paged:
            params.update({"pageNumber": 1, "pageSize": PAGE_SIZE})
        try:
            body = probe_client.get_json(path, params)
        except (RuntimeError, httpx.HTTPError) as exc:
            result["error"] = f"{type(exc).__name__}: {exc}"[:300]
            continue
        rows, total = extract(body)
        if paged and len(rows) > PAGE_SIZE:
            continue  # the endpoint ignores paging and returned everything; read it unpaged
        result.pop("error", None)
        result.update(paged=paged, rows=rows, total=total, wrapped=isinstance(body, dict))
        return result
    return result


def build_config(name: str, path: str, query: dict[str, str], found: dict[str, Any]) -> dict:
    rows = found["rows"]
    key = found["key"]
    usable = all(r.get("lastModifiedDateTime", "0001").startswith("0001") is False for r in rows)
    modified = "lastModifiedDateTime" if usable and "balance" not in name else None
    cfg: dict[str, Any] = {
        "endpoint_name": name,
        "bigquery_table": f"visma_{name}",
        "primary_key": key if len(key) > 1 else key[0],
        "source": "erp_rest",
        "enabled": True,
        "api_method": f"get_{name}",
        "api_endpoint": path,
        "pagination_profile": "finance_page_number" if found["paged"] else "none",
        "modified_field": modified,
        "merge_type": "upsert",
        "sync_pattern": "incremental_upsert" if modified else "full_refresh_upsert",
        "validation_fields": key,
        "notes": (
            f"Found {found['total'] or len(rows)} row(s) via discover_erp_endpoints.py. "
            f"Key chosen from a sample of {len(rows)} row(s); the full ingest run is what "
            "proves it. Stored raw for later Dataform mapping."
            + (" Key set by hand: a sample cannot show it." if name in KEY_OVERRIDES else "")
        ),
    }
    if query:
        cfg["default_query"] = query
    return cfg


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("-e", "--endpoint", action="append", default=[], metavar="name=/path")
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--force", action="store_true", help="overwrite existing configs")
    args = ap.parse_args()

    targets = dict(DEFAULTS)
    if args.endpoint:
        targets = {}
        for spec in args.endpoint:
            name, _, path = spec.partition("=")
            targets[name] = (path, {})

    env = VismaEnv.from_env()
    oauth = OAuth(env)
    client = ErpRestProbe(env, oauth)
    written, problems = 0, []
    try:
        for name, (path, query) in targets.items():
            target = ROOT / "config/endpoints" / f"{name}.json"
            if target.exists() and not args.force:
                print(f"  skip  {name:30} config exists")
                continue
            found = probe(client, path, query)
            if "error" in found:
                problems.append(name)
                print(f"  FAIL  {name:30} {path:34} {found['error'][:110]}")
                continue
            override = KEY_OVERRIDES.get(name)
            if override and not all(k in scalar_paths(r) for r in found["rows"] for k in override):
                found["key"] = None
            else:
                found["key"] = override or choose_key(found["rows"])
            shown = f"rows={len(found['rows'])} total={found['total']} paged={found['paged']}"
            if found["key"] is None:
                problems.append(name)
                fields = sorted(scalar_paths(found["rows"][0]))[:8] if found["rows"] else []
                print(f"  NOKEY {name:30} {path:34} {shown} fields={fields}")
                continue
            cfg = build_config(name, path, query, found)
            print(
                f"  ok    {name:30} {path:34} {shown} key={found['key']} "
                f"incremental={cfg['modified_field'] is not None}"
            )
            if args.write:
                target.write_text(json.dumps(cfg, indent=2, ensure_ascii=False) + "\n")
                written += 1
    finally:
        client.close()
        oauth.close()
    print(f"\nwrote {written} config(s); {len(problems)} need attention: {', '.join(problems)}")
    print("not configured on purpose:")
    for name, (path, why) in NOT_CONFIGURED.items():
        print(f"  - {name} ({path}): {why}")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
