"""One-shot: fetch the single branch from both ERP REST and DM and print
both raw rows side-by-side. Used to diagnose `branches` field mismatches.
"""

from __future__ import annotations

import json
from typing import Any

import httpx

from vsv2_tooling._http import DatamartGraphQL, OAuth, VismaEnv


def _fetch_erp(env: VismaEnv, oauth: OAuth) -> list[dict[str, Any]]:
    url = f"{env.erp_base.rstrip('/')}/v1/branch"
    token = oauth.token_for(scope=env.scope_erp, tenant_id=env.tenant_id)
    with httpx.Client(timeout=60.0) as client:
        resp = client.get(
            url,
            headers={
                "Authorization": f"Bearer {token}",
                "ipp-company-id": env.tenant_id,
                "ipp-application-type": "Connect",
            },
            params={"pageSize": 50, "pageNumber": 1},
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


def _fetch_dm(dm: DatamartGraphQL) -> list[dict[str, Any]]:
    query = (
        "query Branches($pageSize: Int!) {"
        " branches(pageSize: $pageSize) {"
        "  id companyId updatedInDataMartDateTime"
        "  branchId branchCode bAccountId active mainBranch"
        "  ledgerId parentBranchId countryId"
        "  createdInDataMartDateTime"
        " }"
        "}"
    )
    data = dm.execute(query, {"pageSize": 50})
    return data.get("branches") or []


def main() -> None:
    env = VismaEnv.from_env()
    oauth = OAuth(env)
    dm = DatamartGraphQL(env, oauth)

    erp = _fetch_erp(env, oauth)
    dmrows = _fetch_dm(dm)
    print(f"erp_rows={len(erp)}  dm_rows={len(dmrows)}")
    print("=" * 80)
    print("ERP rows:")
    for r in erp:
        print(json.dumps(r, indent=2, default=str))
        print("-" * 40)
    print("=" * 80)
    print("DM rows:")
    for r in dmrows:
        print(json.dumps(r, indent=2, default=str))
        print("-" * 40)


if __name__ == "__main__":
    main()
