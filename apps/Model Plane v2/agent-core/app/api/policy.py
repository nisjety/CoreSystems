"""REST API for org policy limits (admin-only).

GET  /api/v1/policy/{org_id}  → OrgPolicyLimits
PUT  /api/v1/policy/{org_id}  → OrgPolicyLimits (upsert)
DELETE /api/v1/policy/{org_id} → 204
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app.database import get_connection
from app.policy import OrgPolicyLimits
from app.policy.repository import delete_org_policy, get_org_policy, upsert_org_policy

router = APIRouter(prefix="/api/v1/policy", tags=["policy"])


@router.get("/{org_id}", response_model=OrgPolicyLimits)
async def get_policy(org_id: str) -> OrgPolicyLimits:
    async with get_connection() as conn:
        policy = await get_org_policy(conn, org_id)
    if policy is None:
        raise HTTPException(status_code=404, detail=f"No policy found for org '{org_id}'")
    return policy


@router.put("/{org_id}", response_model=OrgPolicyLimits)
async def put_policy(org_id: str, body: OrgPolicyLimits) -> OrgPolicyLimits:
    # Ensure the path param matches the body
    merged = body.model_copy(update={"org_id": org_id})
    async with get_connection() as conn:
        await upsert_org_policy(conn, merged)
    return merged


@router.delete("/{org_id}", status_code=204)
async def delete_policy(org_id: str) -> None:
    async with get_connection() as conn:
        deleted = await delete_org_policy(conn, org_id)
    if not deleted:
        raise HTTPException(status_code=404, detail=f"No policy found for org '{org_id}'")
