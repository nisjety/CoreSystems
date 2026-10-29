#!/usr/bin/env python3
"""Inventory the by-id and child GET endpoints no config reads yet, with their fan-out cost.

A collection endpoint (`/v1/customer`) is one request per page. Its children
(`/v1/customer/{customerCd}/contact`) are one request per parent row, so the cost is the parent's
row count. This script reads Visma's live OpenAPI documents, matches each child path to the
configured collection it hangs off, and prices the fan-out from the row counts the ingest probes
recorded. Nothing is fetched from the tenant.

Writes docs/stage2_child_endpoints.md. The decision (which children are worth their requests) is
the reader's; the script only makes the cost visible.

Usage: scripts/child_endpoint_inventory.py
       scripts/child_endpoint_inventory.py --spec erp=erp.json --spec salesorder=so.json
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs/stage2_child_endpoints.md"
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(ROOT / "tooling/src"))

from discover_erp_endpoints import DEFAULTS, NOT_CONFIGURED  # noqa: E402
from endpoint_matrix import live_status, load_configs  # noqa: E402

SPECS = {
    "erp": "https://integration.visma.net/API-index/doc/swagger",
    "salesorder": "https://salesorder.visma.net/swagger/v3/swagger.json",
}
WINDOW_REQUESTS = 15_000  # ERP fixed-window limit, shared with the live v1 service
PARAM = re.compile(r"\{[^}]+\}")


def load_spec(label: str, saved: dict[str, Path]) -> dict:
    """The OpenAPI document for `label`: a saved file if one was given, else fetched."""
    if label in saved:
        return json.loads(saved[label].read_text())
    return fetch(SPECS[label])


def fetch(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=90) as response:
        return json.load(response)


def get_paths(spec: dict) -> dict[str, dict]:
    return {p: v["get"] for p, v in spec["paths"].items() if "get" in v}


def collection_index(configs: dict[str, dict]) -> dict[str, str]:
    """lower-cased path (host stripped) -> endpoint name, for configured ERP-shaped collections."""
    index = {}
    for name, cfg in configs.items():
        path = cfg.get("api_endpoint")
        if path:
            index[re.sub(r"^https?://[^/]+", "", path).lower()] = name
    return index


def classify(path: str, configured: dict[str, str]) -> tuple[str, str | None]:
    """(kind, parent collection path). kind: covered | by_id | child | subcollection | orphan."""
    lowered = path.lower()
    if lowered in configured:
        return "covered", None
    segments = path.strip("/").split("/")
    # Parent = the longest leading run of literal segments that is a configured collection.
    for cut in range(len(segments) - 1, 0, -1):
        prefix = "/" + "/".join(segments[:cut])
        if PARAM.search(prefix):
            continue
        if prefix.lower() in configured:
            rest = segments[cut:]
            if not any(PARAM.search(segment) for segment in rest):
                return "subcollection", prefix  # one request in total, not one per parent row
            kind = "by_id" if len(rest) == 1 and PARAM.fullmatch(rest[0]) else "child"
            return kind, prefix
    return "orphan", None


def rows_of(name: str, status: dict[str, tuple[str, int]]) -> int | None:
    state, rows = status.get(name, ("untested", 0))
    return rows if state == "proven" and rows else None


# Judged from Visma's own descriptions and the endpoint names; only the ones flagged
# "measured" were compared row for row. Anything not listed here or marked deprecated by Visma
# reads "no decision yet".
JUDGED: dict[str, str] = {
    "/v1/purchasereceipt": "older version of `purchase_receipts` (/v2/PurchaseReceipt), "
    "not compared",
    "/v1/discountcode": "older version of `discount_codes` (/v2/discountCode); not compared",
    "/v1/salesperson": "older version of `sales_persons` (/v2/salesperson); not compared",
    "/v1/salesorder": "Sales Order API v3 (`sales_orders_v3`) is the current interface",
    "/v2/salesorder": "Sales Order API v3 (`sales_orders_v3`) is the current interface",
    "/v1/salesorderbasic": "lean view of the sales orders `sales_orders_v3` already reads",
    "/v1/purchaseorderbasic": "lean view of `purchase_orders`",
    "/v1/projectbasic": "lean view of `projects`",
    "/v1/project/tasks": "needs a project id (one call per project); `dm_project_tasks` "
    "carries all rows (the ERP config `project_tasks` answers 400 without a project id)",
    "/v1/supplierinvoice/approval": "a lookup by `systemId`, not a listing",
    "/v1/blob/presignedurl": "tooling: no business data",
    "/v1/firsttimestartup/status": "tooling: no business data",
    "/v1/uiextension/availablescreens": "tooling: no business data",
}


def unread_reason(path: str, operation: dict) -> str:
    """Why a collection endpoint has no config, or that nobody has decided yet."""
    lowered = path.lower()
    for name, (known, why) in NOT_CONFIGURED.items():
        if known.lower() == lowered:
            return f"`{name}` (measured): {why}"
    if any(known.lower() == lowered for known, _ in DEFAULTS.values()):
        return "configured by discover_erp_endpoints.py"
    if lowered in JUDGED:
        return JUDGED[lowered]
    if operation.get("deprecated"):
        return "deprecated by Visma"
    return "no decision yet"


def response_schema(operation: dict) -> str | None:
    """Name of the schema a 200 response returns (array items unwrapped), OpenAPI 2 or 3."""
    response = (operation.get("responses") or {}).get("200") or {}
    schema = response.get("schema")
    if schema is None:
        for content in (response.get("content") or {}).values():
            schema = content.get("schema")
            break
    if schema and schema.get("type") == "array":
        schema = schema.get("items")
    ref = (schema or {}).get("$ref", "")
    return ref.rsplit("/", 1)[-1] or None


def redundancy_note(label: str, path: str, kind: str, operation: dict, parent: dict) -> str:
    if label == "salesorder":
        lowered = path.lower().rstrip("/")
        if lowered.startswith("/api/v3/salesorders"):
            if lowered.endswith("/{type}"):
                return "the list filtered by order type: no rows the list lacks"
            return "already inside the order via `expand=All,LineFees`"
        return ""
    if kind == "by_id":
        mine, theirs = response_schema(operation), response_schema(parent)
        if mine and mine == theirs:
            return f"same DTO as the list (`{mine}`): adds nothing unless the list omits an expand"
        if mine and theirs:
            return f"different DTO (`{mine}` vs list `{theirs}`): may hold more"
    return ""


def section(label: str, paths: dict, configured: dict, status: dict) -> tuple[list[str], int]:
    kinds: dict[str, list[str]] = defaultdict(list)
    fan_out: dict[str, list[tuple[str, str, int | None, str]]] = defaultdict(list)
    for path in sorted(paths):
        kind, parent = classify(path, configured)
        has_param = bool(PARAM.search(path))
        if kind == "orphan" and not has_param:
            kind = "subcollection"
        kinds[kind].append(path)
        if kind in ("by_id", "child"):
            name = configured[parent.lower()]
            note = redundancy_note(label, path, kind, paths[path], paths.get(parent, {}))
            fan_out[name].append((path, kind, rows_of(name, status), note))

    lines = [
        f"## {label}: {len(paths)} GET endpoints",
        "",
        f"- {len(kinds['covered'])} are collections read by a config",
        f"- {len(kinds['subcollection'])} are collections nothing reads yet (stage 1 gaps, one "
        "request each)",
        f"- {len(kinds['by_id'])} by-id and {len(kinds['child'])} child endpoints under a "
        "configured collection (one request per parent row)",
        f"- {len(kinds['orphan'])} by-id/child endpoints whose parent collection is not configured",
        "",
    ]
    if kinds["subcollection"]:
        lines += ["### Collections nothing reads yet", "", "| Endpoint | Status |", "|---|---|"]
        lines += [f"| `{p}` | {unread_reason(p, paths[p])} |" for p in kinds["subcollection"]]
        lines.append("")
    if fan_out:
        lines += [
            "### By-id and child endpoints",
            "",
            "| Parent config | Parent rows | Endpoint | Kind | Requests to read all | Note |",
            "|---|---|---|---|---|---|",
        ]
    total = 0
    for name in sorted(fan_out):
        for path, kind, rows, note in fan_out[name]:
            shown_rows = f"{rows:,}" if rows else "-"
            cost = f"{rows:,}" if rows else "unknown (parent not proven)"
            lines.append(f"| `{name}` | {shown_rows} | `{path}` | {kind} | {cost} | {note} |")
            if not note.startswith(("already inside", "same DTO", "the list filtered")):
                total += rows or 0
    lines.append("")
    if kinds["orphan"]:
        lines += ["### Parent not configured", ""] + [f"- `{p}`" for p in kinds["orphan"]] + [""]
    return lines, total


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument(
        "--spec",
        action="append",
        default=[],
        metavar="LABEL=FILE",
        help="read this saved OpenAPI file instead of fetching (erp, salesorder)",
    )
    args = ap.parse_args()
    saved = {label: Path(path) for label, _, path in (s.partition("=") for s in args.spec)}
    configured = collection_index(load_configs())
    status = live_status()
    lines = [
        "# Stage 2: by-id and child endpoints",
        "",
        "> Generated by `scripts/child_endpoint_inventory.py` from Visma's live OpenAPI documents,",
        "> `config/endpoints/*.json` and the row counts in `docs/evidence/`. Do not edit by hand.",
        "",
        "Stage 1 reads every *collection* endpoint. The rest are GET endpoints it does not read:",
        "`by_id` is the detail of one row, `child` is the rows one parent owns. Both cost one",
        f"request per parent row. The ERP allows {WINDOW_REQUESTS:,} requests per fixed window and",
        "the live v1 service draws on the same quota, so cost is shown in requests and windows.",
        "",
    ]
    totals: dict[str, int] = {}
    for label in SPECS:
        try:
            paths = get_paths(load_spec(label, saved))
        except OSError as exc:
            print(f"cannot fetch {label} spec: {exc}", file=sys.stderr)
            return 1
        body, totals[label] = section(label, paths, configured, status)
        lines += body
    lines += [
        "## Total",
        "",
        "Reading every by-id/child endpoint above once (parents proven only):",
        "",
    ]
    lines += [
        f"- **{label}**: {n:,} requests = {n / WINDOW_REQUESTS:.1f} rate-limit windows"
        for label, n in totals.items()
    ]
    lines.append("")
    OUT.write_text("\n".join(lines))
    print(f"wrote {OUT.relative_to(ROOT)} ({len(lines)} lines)")
    for label, n in totals.items():
        print(f"  {label}: {n:,} requests in total")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
