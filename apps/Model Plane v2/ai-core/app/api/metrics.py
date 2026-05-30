"""Metrics API.

Internal-only endpoints for observability dashboards and billing-core.

GET  /v2/metrics              — global + all-org counters
GET  /v2/metrics/{org_id}     — single-org counters
GET  /v2/metrics/safety        — safety telemetry global counters
GET  /v2/metrics/safety/{org_id} — per-org safety telemetry
"""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app.services.inference_metrics import get_inference_metrics
from app.services.safety_telemetry import get_safety_telemetry

router = APIRouter(prefix="/v2/metrics", tags=["metrics"])


@router.get("")
async def get_global_metrics() -> dict:
    return get_inference_metrics().get_metrics()


@router.get("/safety")
async def get_global_safety_metrics() -> dict:
    return get_safety_telemetry().get_metrics()


@router.get("/safety/{org_id}")
async def get_org_safety_metrics(org_id: str) -> dict:
    return get_safety_telemetry().get_metrics(org_id=org_id)


@router.get("/{org_id}")
async def get_org_metrics(org_id: str) -> dict:
    metrics = get_inference_metrics().get_metrics(org_id=org_id)
    if metrics is None:
        raise HTTPException(status_code=404, detail="org not found")
    return metrics
