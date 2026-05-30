"""Runner management routes — /v1/runners."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app import repository
from app.domain import RunnerStatus
from app.runner_service import RunnerService

router = APIRouter(prefix="/v1/runners", tags=["runners"])

# Injected at startup
_service: RunnerService | None = None


def init(service: RunnerService) -> None:
    global _service
    _service = service


def _svc() -> RunnerService:
    if _service is None:
        raise HTTPException(503, "Service not ready")
    return _service


@router.get("")
async def list_runners(status: str | None = None, limit: int = 100):
    """List registered runners, optionally filtered by status."""
    st = RunnerStatus(status) if status else None
    runners = await repository.list_runners(status=st, limit=limit)
    return {"runners": [r.model_dump(mode="json") for r in runners]}


@router.get("/{runner_id}")
async def get_runner(runner_id: str):
    """Get a single runner by ID."""
    record = await repository.get_runner(runner_id)
    if not record:
        raise HTTPException(404, "Runner not found")
    return record.model_dump(mode="json")


@router.delete("/{runner_id}", status_code=204)
async def delete_runner(runner_id: str):
    """Deregister a runner."""
    deleted = await _svc().handle_deregister(runner_id)
    if not deleted:
        raise HTTPException(404, "Runner not found")
