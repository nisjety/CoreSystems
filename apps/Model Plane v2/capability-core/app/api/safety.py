"""Content Safety API — text moderation endpoint. Phase 4.3."""
from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.config import settings
from app.safety import check_text

router = APIRouter(prefix="/v1/safety", tags=["safety"])


class SafetyCheckRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=10_000)
    thresholds: dict[str, int] | None = None


class SafetyCheckResponse(BaseModel):
    safe: bool
    categories: dict[str, int]
    blocked_categories: list[str]
    action: str


@router.post("/check", response_model=SafetyCheckResponse)
async def check_safety_endpoint(body: SafetyCheckRequest):
    """Analyze text for harmful content."""
    try:
        result = await check_text(
            body.text,
            azure_endpoint=getattr(settings, "azure_content_safety_endpoint", ""),
            azure_key=getattr(settings, "azure_content_safety_key", ""),
            thresholds=body.thresholds,
        )
        return SafetyCheckResponse(
            safe=result.safe,
            categories=result.categories,
            blocked_categories=result.blocked_categories,
            action=result.action,
        )
    except Exception as exc:
        raise HTTPException(status_code=503, detail=f"Safety check failed: {exc}")
