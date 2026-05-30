"""Cron API endpoints — CRUD for scheduled agent runs."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException

from app.cron.domain import CreateCronRequest, CronListResponse, CronTask, CronTaskResponse
from app.cron import repository
from app.cron.scheduler import compute_next_run

router = APIRouter(prefix="/cron", tags=["cron"])


@router.post("", response_model=CronTaskResponse, status_code=201)
async def create_cron(req: CreateCronRequest) -> CronTaskResponse:
    # Validate cron expression
    try:
        from croniter import croniter

        croniter(req.cron_expr)
    except (ValueError, KeyError) as exc:
        raise HTTPException(status_code=400, detail=f"invalid cron expression: {exc}")

    next_run = compute_next_run(req.cron_expr)
    task = CronTask(
        org_id=req.org_id,
        session_id=req.session_id,
        name=req.name,
        cron_expr=req.cron_expr,
        goal=req.goal,
        policy=req.policy,
        next_run_at=next_run,
    )
    created = await repository.create_cron(task)
    return CronTaskResponse(cron_task=created)


@router.get("/{cron_id}", response_model=CronTaskResponse)
async def get_cron(cron_id: str) -> CronTaskResponse:
    task = await repository.get_cron(cron_id)
    if task is None:
        raise HTTPException(status_code=404, detail="cron task not found")
    return CronTaskResponse(cron_task=task)


@router.get("/org/{org_id}", response_model=CronListResponse)
async def list_crons(org_id: str) -> CronListResponse:
    tasks = await repository.list_crons(org_id)
    return CronListResponse(cron_tasks=tasks)


@router.post("/{cron_id}/toggle")
async def toggle_cron(cron_id: str, enabled: bool = True) -> dict:
    task = await repository.get_cron(cron_id)
    if task is None:
        raise HTTPException(status_code=404, detail="cron task not found")
    await repository.toggle_cron(cron_id, enabled)
    return {"cron_id": cron_id, "enabled": enabled}


@router.delete("/{cron_id}", status_code=204)
async def delete_cron(cron_id: str) -> None:
    deleted = await repository.delete_cron(cron_id)
    if not deleted:
        raise HTTPException(status_code=404, detail="cron task not found")
