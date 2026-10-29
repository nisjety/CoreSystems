"""One-shot: fetch the same accountCD from ERP REST and DataMart and print
the controlAccountModule values from both sides for the first N overlapping
rows.

Usage:
    .venv/bin/python tooling/scripts/sample_account_control_module.py [N]
"""

from __future__ import annotations

import json
import sys
from typing import Any

from vsv2_tooling._http import DatamartGraphQL, OAuth, VismaEnv


def _fetch_erp_accounts(env: VismaEnv, oauth: OAuth, page_size: int) -> list[dict[str, Any]]:
    import httpx

    url = f"{env.erp_base.rstrip('/')}/v1/account"
    token = oauth.token_for(scope=env.scope_erp, tenant_id=env.tenant_id)
    with httpx.Client(timeout=60.0) as client:
        resp = client.get(
            url,
            headers={
                "Authorization": f"Bearer {token}",
                "ipp-company-id": env.tenant_id,
                "ipp-application-type": "Connect",
            },
            params={"pageSize": page_size, "pageNumber": 1},
        )
    resp.raise_for_status()
    body = resp.json()
    if isinstance(body, list):
        return body
    if isinstance(body, dict):
        for key in ("value", "data", "items", "results"):
            inner = body.get(key)
            if isinstance(inner, list):
                return inner
    raise RuntimeError(f"unexpected ERP response shape: {type(body).__name__}")


def _fetch_dm_accounts(dm: DatamartGraphQL, page_size: int) -> list[dict[str, Any]]:
    query = (
        "query Accounts($pageSize: Int!) {"
        " accounts(pageSize: $pageSize) {"
        "  accountCd"
        "  controlAccountModule"
        "  type"
        "  description"
        " }"
        "}"
    )
    data = dm.execute(query, {"pageSize": page_size})
    return data.get("accounts") or []


def main() -> None:
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    env = VismaEnv.from_env()
    oauth = OAuth(env)
    dm = DatamartGraphQL(env, oauth)

    erp_rows = _fetch_erp_accounts(env, oauth, page_size=200)
    dm_rows = _fetch_dm_accounts(dm, page_size=200)
    erp_by_cd = {r.get("accountCD"): r for r in erp_rows if r.get("accountCD")}
    dm_by_cd = {r.get("accountCd"): r for r in dm_rows if r.get("accountCd")}
    overlap = sorted(set(erp_by_cd) & set(dm_by_cd))[:n]

    overlap_count = len(set(erp_by_cd) & set(dm_by_cd))
    print(f"erp_rows={len(erp_rows)}  dm_rows={len(dm_rows)}  overlap={overlap_count}")
    print("-" * 80)
    print(f"{'accountCD':<14} {'erp.controlAccountModule':<30} {'dm.controlAccountModule':<30}")
    print("-" * 80)
    erp_distinct: dict[str, int] = {}
    dm_distinct: dict[str, int] = {}
    for cd in overlap:
        e = erp_by_cd[cd]
        d = dm_by_cd[cd]
        erp_val = repr(e.get("controlAccountModule"))
        dm_val = repr(d.get("controlAccountModule"))
        erp_distinct[erp_val] = erp_distinct.get(erp_val, 0) + 1
        dm_distinct[dm_val] = dm_distinct.get(dm_val, 0) + 1
        print(f"{cd:<14} {erp_val:<30} {dm_val:<30}")
    print("-" * 80)
    print(f"distinct ERP values in shown rows: {erp_distinct}")
    print(f"distinct DM  values in shown rows: {dm_distinct}")

    # Aggregate across the full fetched samples too
    full_erp: dict[str, int] = {}
    full_dm: dict[str, int] = {}
    for r in erp_rows:
        v = repr(r.get("controlAccountModule"))
        full_erp[v] = full_erp.get(v, 0) + 1
    for r in dm_rows:
        v = repr(r.get("controlAccountModule"))
        full_dm[v] = full_dm.get(v, 0) + 1
    print()
    print(f"distinct ERP values across full sample ({len(erp_rows)} rows):")
    for v, c in sorted(full_erp.items(), key=lambda kv: -kv[1]):
        print(f"  {v}: {c}")
    print(f"distinct DM values across full sample ({len(dm_rows)} rows):")
    for v, c in sorted(full_dm.items(), key=lambda kv: -kv[1]):
        print(f"  {v}: {c}")

    # Show one raw record from each side for the first overlap key
    if overlap:
        cd = overlap[0]
        print()
        print(f"raw ERP row for {cd}:")
        print(json.dumps(erp_by_cd[cd], indent=2, default=str))
        print()
        print(f"raw DM row for {cd}:")
        print(json.dumps(dm_by_cd[cd], indent=2, default=str))


if __name__ == "__main__":
    main()
