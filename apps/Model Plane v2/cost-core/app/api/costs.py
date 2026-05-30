from __future__ import annotations

from decimal import Decimal
from typing import Any, Dict, Optional

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app import repository
from app.pricing import calculate_usd

router = APIRouter(prefix="/v1/costs", tags=["costs"])


class TurnCostIn(BaseModel):
    run_id: str
    org_id: str
    model: str
    input_tokens: int = Field(ge=0)
    output_tokens: int = Field(ge=0)
    cost_usd: Optional[Decimal] = None
    turn_index: Optional[int] = None
    metadata: Optional[Dict[str, Any]] = None


class TurnCostOut(BaseModel):
    run_id: str
    cost_usd: Decimal


@router.post("/turns", response_model=TurnCostOut)
async def record_turn(body: TurnCostIn) -> TurnCostOut:
    cost = body.cost_usd if body.cost_usd is not None else calculate_usd(
        body.model, body.input_tokens, body.output_tokens
    )
    await repository.insert_run_cost_turn(
        run_id=body.run_id,
        org_id=body.org_id,
        model=body.model,
        input_tokens=body.input_tokens,
        output_tokens=body.output_tokens,
        cost_usd=cost,
        turn_index=body.turn_index,
        metadata=body.metadata,
    )
    return TurnCostOut(run_id=body.run_id, cost_usd=cost)


@router.get("/runs/{run_id}")
async def get_run_cost(run_id: str) -> dict:
    agg = await repository.get_run_cost_aggregate(run_id)
    if agg["turn_count"] == 0:
        raise HTTPException(status_code=404, detail="run not found")
    agg["total_cost_usd"] = str(agg["total_cost_usd"])
    return agg
