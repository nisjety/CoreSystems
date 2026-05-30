"""Health check — GET /health."""

from __future__ import annotations

from fastapi import APIRouter

from app.database import get_pool

router = APIRouter(tags=["health"])


@router.get("/health")
async def health() -> dict:
    pool = await get_pool()
    row = await pool.fetchrow("SELECT 1 AS ok")
    return {
        "status": "healthy" if row else "degraded",
        "service": "capability-core",
    }
