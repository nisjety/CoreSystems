#!/usr/bin/env python3
"""Measure what each `expand*` query flag adds to a configured ERP collection.

Many Finance list endpoints leave nested data out unless a flag such as `expandApplications`
or `expandTaxDetails` is set. A config that sets none of them lands a thinner row in bronze than
the API can give. For every configured ERP collection with such flags this reads one small page
without flags, then once per flag, and reports whether the flag works and which non-empty field
paths it adds.

With --write, the flags that answered without error are added to the config's `default_query`.
A flag that adds nothing to the sample is still enabled: an empty section in 20 rows says
nothing about the other rows. The full ingest run (scripts/ingest_probe.py) is what proves each
enabled config at scale, including its runtime.

Read-only against the tenant: one GET per flag, page size 20.

Usage:
  scripts/expand_coverage.py                    # report only
  scripts/expand_coverage.py --write            # also update config default_query
  scripts/expand_coverage.py --only suppliers,customers
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

import httpx

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tooling/src"))

from vsv2_tooling._http import OAuth, VismaEnv  # noqa: E402
from vsv2_tooling.discovery.shadow_compare import ErpRestProbe  # noqa: E402

SPEC_URL = "https://integration.visma.net/API-index/doc/swagger"
SAMPLE_SIZE = 20
SLOW_FACTOR = 5.0  # a flag this many times slower than the baseline is reported as slow


def non_empty_paths(rows: list[dict], prefix: str = "", depth: int = 3) -> set[str]:
    """Dotted field paths that hold a non-empty value in at least one row."""
    found: set[str] = set()
    for row in rows:
        _walk(row, prefix, depth, found)
    return found


def _walk(value: Any, path: str, depth: int, found: set[str]) -> None:
    if isinstance(value, dict):
        for key, inner in value.items():
            _walk(inner, f"{path}.{key}" if path else key, depth, found)
    elif isinstance(value, list):
        if value and depth > 0:
            found.add(path)
            for item in value:
                _walk(item, f"{path}[]", depth - 1, found)
    elif value not in (None, "", 0, False):
        found.add(path)


def rows_of(body: Any) -> list[dict]:
    if isinstance(body, list):
        return [r for r in body if isinstance(r, dict)]
    if isinstance(body, dict):
        for key in ("value", "items", "data", "results", "records"):
            if isinstance(body.get(key), list):
                return [r for r in body[key] if isinstance(r, dict)]
    return []


def expand_flags(spec: dict, endpoint: str) -> list[str]:
    lowered = {k.lower(): k for k in spec["paths"]}
    path = lowered.get(endpoint.lower())
    if not path:
        return []
    params = spec["paths"][path]["get"].get("parameters", [])
    return [p["name"] for p in params if p["name"].lower().startswith("expand")]


def base_params(cfg: dict) -> dict[str, Any]:
    params: dict[str, Any] = {}
    if cfg.get("pagination_profile") == "finance_page_number":
        params.update({"pageNumber": 1, "pageSize": SAMPLE_SIZE})
    params.update({k: v for k, v in (cfg.get("default_query") or {}).items()})
    return params


def timed_get(
    client: ErpRestProbe, path: str, params: dict
) -> tuple[list[dict] | None, str, float]:
    started = time.monotonic()
    try:
        body = client.get_json(path, params)
    except (RuntimeError, httpx.HTTPError) as exc:
        return (
            None,
            f"{type(exc).__name__}: {exc}"[:160].replace("\n", " "),
            time.monotonic() - started,
        )
    return rows_of(body), "", time.monotonic() - started


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--write", action="store_true", help="enable working flags in the configs")
    ap.add_argument("--only", default="", help="comma-separated endpoint names")
    args = ap.parse_args()
    only = {n for n in args.only.split(",") if n}

    with urllib.request.urlopen(SPEC_URL, timeout=90) as response:
        spec = json.load(response)

    env = VismaEnv.from_env()
    oauth = OAuth(env)
    client = ErpRestProbe(env, oauth, timeout=120.0)
    report: list[str] = []
    changed = 0
    try:
        for path in sorted((ROOT / "config/endpoints").glob("*.json")):
            if path.name.startswith("_"):
                continue
            cfg = json.loads(path.read_text())
            endpoint = cfg.get("api_endpoint") or ""
            if (
                cfg["source"] != "erp_rest"
                or not endpoint
                or endpoint.startswith("http")
                or cfg.get("required_query")
                or cfg.get("partitioning")
                or (only and cfg["endpoint_name"] not in only)
            ):
                continue
            flags = expand_flags(spec, endpoint)
            if not flags:
                continue
            name = cfg["endpoint_name"]
            current = cfg.get("default_query") or {}
            todo = [f for f in flags if str(current.get(f, "")).lower() != "true"]
            if not todo:
                continue

            base_rows, err, base_secs = timed_get(client, endpoint, base_params(cfg))
            if base_rows is None:
                report.append(f"| `{name}` | baseline failed | | {err} |")
                continue
            already = non_empty_paths(base_rows)
            working: list[str] = []
            for flag in todo:
                params = base_params(cfg) | {flag: "true"}
                rows, err, secs = timed_get(client, endpoint, params)
                if rows is None:
                    report.append(f"| `{name}` | `{flag}` | ERROR | {err} |")
                    continue
                added = sorted(non_empty_paths(rows) - already)
                slow = secs > max(10.0, base_secs * SLOW_FACTOR)
                note = ", ".join(added[:6]) + (" ..." if len(added) > 6 else "") or "nothing new"
                report.append(
                    f"| `{name}` | `{flag}` | {'SLOW ' if slow else ''}ok "
                    f"({secs:.1f}s vs {base_secs:.1f}s) | {note} |"
                )
                if not slow:
                    working.append(flag)
            print(f"{name:30} {len(working)}/{len(todo)} flags usable", flush=True)
            if args.write and working:
                merged = dict(current) | {flag: "true" for flag in working}
                cfg["default_query"] = merged
                path.write_text(json.dumps(cfg, indent=2, ensure_ascii=False) + "\n")
                changed += 1
    finally:
        client.close()
        oauth.close()

    print()
    print("| Endpoint | Flag | Result | Adds (non-empty field paths in a 20-row sample) |")
    print("|---|---|---|---|")
    print("\n".join(report))
    print(f"\nupdated {changed} config(s)" if args.write else "\nreport only (no --write)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
