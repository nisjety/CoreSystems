"""Model config routes — /v1/models."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app import repository
from app.domain import ModelConfig

router = APIRouter(prefix="/v1/models", tags=["models"])


@router.get("")
async def list_models(
    provider: str | None = None,
    enabled: bool | None = None,
) -> dict:
    models = await repository.list_models(provider=provider, enabled=enabled)
    return {"models": [m.model_dump(mode="json") for m in models]}


@router.get("/{model_id}")
async def get_model(model_id: str) -> dict:
    model = await repository.get_model(model_id)
    if model is None:
        raise HTTPException(404, "Model not found")
    return model.model_dump(mode="json")


@router.put("/{model_id}")
async def upsert_model(model_id: str, body: ModelConfig) -> dict:
    model = body.model_copy(update={"model_id": model_id})
    saved = await repository.upsert_model(model)
    return saved.model_dump(mode="json")
