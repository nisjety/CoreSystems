"""Routing and budget routes — /v1/routing."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.domain import RoutingPolicy, UsageRecord
from app.routing import budget as budget_mod
from app.routing import selector as selector_mod
from app.routing.policy import get_policy, upsert_policy

router = APIRouter(prefix="/v1/routing", tags=["routing"])


class SelectRequest(BaseModel):
    org_id: str
    feature_requirements: dict[str, bool] | None = None
    max_input_tokens: int | None = None


class RecordUsageRequest(BaseModel):
    org_id: str
    session_id: str = ""
    cost_nok: float


@router.get("/policy/{org_id}")
async def get_routing_policy(org_id: str) -> dict:
    policy = await get_policy(org_id)
    return policy.model_dump(mode="json")


@router.put("/policy/{org_id}")
async def set_routing_policy(
    org_id: str, body: RoutingPolicy, actor: str = "system"
) -> dict:
    policy = body.model_copy(update={"org_id": org_id})
    saved = await upsert_policy(policy, actor=actor)
    return saved.model_dump(mode="json")


@router.post("/select")
async def select_model(req: SelectRequest) -> dict:
    selection = await selector_mod.select(
        req.org_id,
        feature_requirements=req.feature_requirements,
        max_input_tokens=req.max_input_tokens,
    )
    if selection is None:
        raise HTTPException(
            429, "No model available — budget exhausted or no match"
        )
    return selection.model_dump(mode="json")


@router.get("/budget/{org_id}")
async def check_budget(org_id: str) -> dict:
    policy = await get_policy(org_id)
    result = await budget_mod.check(org_id, policy)
    return result.model_dump(mode="json")


@router.post("/usage")
async def record_usage(req: RecordUsageRequest) -> dict:
    await budget_mod.record(
        UsageRecord(
            org_id=req.org_id,
            session_id=req.session_id,
            cost_nok=req.cost_nok,
        )
    )
    return {"ok": True}
