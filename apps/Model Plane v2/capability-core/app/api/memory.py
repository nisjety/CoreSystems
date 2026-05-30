"""Memory adapter catalog routes — /v1/memory."""

from __future__ import annotations

from fastapi import APIRouter

from app.memory import adapters as adapter_mod

router = APIRouter(prefix="/v1/memory", tags=["memory"])


@router.get("")
async def list_adapters() -> dict:
    adapters = await adapter_mod.list_adapters()
    return {"adapters": [a.model_dump(mode="json") for a in adapters]}
