"""Health and readiness endpoints."""

from __future__ import annotations

from fastapi import APIRouter

router = APIRouter(tags=["health"])


@router.get("/health")
async def health():
    return {"status": "ok", "service": "llm-worker"}


@router.get("/ready")
async def ready():
    return {"status": "ready", "service": "llm-worker"}
