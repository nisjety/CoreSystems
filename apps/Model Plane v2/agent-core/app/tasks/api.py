"""Task API endpoints — CC-style CRUD + claim + output."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.tasks.domain import TaskRecord, TaskStatus, TaskUpdate
from app.tasks import repository

router = APIRouter(prefix="/tasks", tags=["tasks"])


# ---------------------------------------------------------------------------
# Request / Response schemas
# ---------------------------------------------------------------------------


class CreateTaskRequest(BaseModel):
    run_id: str
    session_id: str
    org_id: str | None = None
    subject: str
    description: str = ""
    blocked_by: list[str] | None = None
    metadata: dict | None = None


class UpdateTaskRequest(BaseModel):
    subject: str | None = None
    description: str | None = None
    status: TaskStatus | None = None
    owner_agent_id: str | None = None
    add_blocks: list[str] | None = None
    add_blocked_by: list[str] | None = None
    output: str | None = None
    error: str | None = None
    metadata: dict | None = None


class ClaimTaskRequest(BaseModel):
    agent_id: str


class TaskResponse(BaseModel):
    task: TaskRecord


class TaskListResponse(BaseModel):
    tasks: list[TaskRecord]


class ClaimResponse(BaseModel):
    success: bool
    reason: str | None = None
    task: TaskRecord | None = None
    blocked_by_tasks: list[str] | None = None


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------


@router.post("", response_model=TaskResponse, status_code=201)
async def create_task(req: CreateTaskRequest) -> TaskResponse:
    task = TaskRecord(
        run_id=req.run_id,
        session_id=req.session_id,
        org_id=req.org_id,
        subject=req.subject,
        description=req.description,
        blocked_by=req.blocked_by or [],
        metadata=req.metadata or {},
    )
    created = await repository.create_task(task)
    return TaskResponse(task=created)


@router.get("/{task_id}", response_model=TaskResponse)
async def get_task(task_id: str) -> TaskResponse:
    task = await repository.get_task(task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    return TaskResponse(task=task)


@router.get("/run/{run_id}", response_model=TaskListResponse)
async def list_tasks(run_id: str) -> TaskListResponse:
    tasks = await repository.list_tasks(run_id)
    # Filter out blocked_by refs that are terminal (CC behaviour)
    terminal_ids: set[str] = set()
    for t in tasks:
        from app.tasks.domain import is_terminal as _is_terminal

        if _is_terminal(t.status):
            terminal_ids.add(t.id)
    for t in tasks:
        t.blocked_by = [bid for bid in t.blocked_by if bid not in terminal_ids]
    return TaskListResponse(tasks=tasks)


@router.patch("/{task_id}", response_model=TaskResponse)
async def update_task(task_id: str, req: UpdateTaskRequest) -> TaskResponse:
    update = TaskUpdate(**req.model_dump(exclude_none=True))
    task = await repository.update_task(task_id, update)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    return TaskResponse(task=task)


@router.post("/{task_id}/claim", response_model=ClaimResponse)
async def claim_task(task_id: str, req: ClaimTaskRequest) -> ClaimResponse:
    # Fetch task first to get run_id
    task = await repository.get_task(task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")
    result = await repository.claim_task(task_id, req.agent_id, task.run_id)
    return ClaimResponse(
        success=result.success,
        reason=result.reason,
        task=result.task,
        blocked_by_tasks=result.blocked_by_tasks,
    )


@router.delete("/{task_id}", status_code=204)
async def delete_task(task_id: str) -> None:
    deleted = await repository.delete_task(task_id)
    if not deleted:
        raise HTTPException(status_code=404, detail="task not found")


@router.get("/{task_id}/output")
async def get_task_output(task_id: str) -> dict:
    """Get task output (CC TaskOutputTool equivalent)."""
    task = await repository.get_task(task_id)
    if task is None:
        raise HTTPException(status_code=404, detail="task not found")

    from app.tasks.domain import is_terminal as _is_terminal

    if not _is_terminal(task.status):
        return {
            "retrieval_status": "not_ready",
            "task": {
                "task_id": task.id,
                "status": task.status.value,
                "description": task.subject,
            },
        }

    return {
        "retrieval_status": "success",
        "task": {
            "task_id": task.id,
            "status": task.status.value,
            "description": task.subject,
            "output": task.output,
            "error": task.error,
        },
    }
