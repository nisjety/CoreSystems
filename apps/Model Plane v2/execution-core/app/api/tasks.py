"""Task management routes — /v1/tasks."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app import repository
from app.runner_service import RunnerService

router = APIRouter(prefix="/v1/tasks", tags=["tasks"])

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
async def list_tasks(
    run_id: str | None = None,
    status: str | None = None,
    limit: int = 100,
):
    """List tasks, optionally filtered by run_id and/or status."""
    tasks = await repository.list_tasks(run_id=run_id, status=status, limit=limit)
    return {"tasks": [t.model_dump(mode="json") for t in tasks]}


@router.get("/{task_id}")
async def get_task(task_id: str):
    """Get a single task by ID."""
    record = await repository.get_task(task_id)
    if not record:
        raise HTTPException(404, "Task not found")
    return record.model_dump(mode="json")


@router.post("/{task_id}/cancel", status_code=202)
async def cancel_task(task_id: str, reason: str = "api_cancel"):
    """Cancel a running task."""
    await _svc().handle_cancel(task_id, {"reason": reason})
    return {"task_id": task_id, "status": "cancelled"}
