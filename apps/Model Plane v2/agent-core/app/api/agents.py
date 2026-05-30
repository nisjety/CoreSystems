"""Agent run HTTP routes.

POST /agent-runs           — Create a new run
GET  /agent-runs/{run_id}  — Get run by ID
GET  /agent-runs           — List runs for a session
GET  /agent-capabilities   — List supported adapters
"""

from __future__ import annotations

import asyncio
from fastapi import APIRouter, Depends, HTTPException, Query

from app.agent_service import AgentService
from app.domain import CreateRunRequest, RunRecord
from app.middleware.auth import Principal, get_principal

router = APIRouter(prefix="/agent-runs", tags=["agent-runs"])


def _get_service() -> AgentService:
    """Lazy-import to avoid circular dependency at module load."""
    from app.main import agent_service
    return agent_service


# ------------------------------------------------------------------
# POST /agent-runs
# ------------------------------------------------------------------


@router.post("", status_code=201)
async def create_run(
    body: CreateRunRequest,
    principal: Principal = Depends(get_principal),
) -> dict:
    """Create and begin executing an agent run in the background."""
    svc = _get_service()
    run = await svc.create_run(
        request=body,
        session_id=body.context.get("session_id", ""),
        user_id=principal.user_id if not principal.is_internal else body.context.get("user_id", ""),
        org_id=principal.org_id if principal.org_id != "system" else body.context.get("org_id"),
    )
    asyncio.create_task(svc.execute_run(run.id))
    return {"run_id": run.id, "status": run.status.value}


# ------------------------------------------------------------------
# GET /agent-runs/{run_id}
# ------------------------------------------------------------------


@router.get("/{run_id}")
async def get_run(
    run_id: str,
    principal: Principal = Depends(get_principal),
) -> dict:
    from app import repository as repo
    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="Run not found")
    return run.model_dump(mode="json")


# ------------------------------------------------------------------
# GET /agent-runs
# ------------------------------------------------------------------


@router.get("")
async def list_runs(
    session_id: str = Query(..., description="Session to list runs for"),
    limit: int = Query(50, ge=1, le=200),
    principal: Principal = Depends(get_principal),
) -> list[dict]:
    from app import repository as repo
    runs = await repo.list_runs_by_session(session_id, limit=limit)
    return [r.model_dump(mode="json") for r in runs]


# ------------------------------------------------------------------
# GET /agent-capabilities
# ------------------------------------------------------------------


@router.get("/capabilities", tags=["agent-capabilities"])
async def get_capabilities() -> dict:
    """List available framework adapters and their enablement status."""
    from app.adapters import _ADAPTER_REGISTRY

    # Phase status: tracks which adapters have real backends wired.
    _WIRING_STATUS = {
        "letta_memory": "wired (Phase 5)",
        "langgraph_workflow": "wired (Phase 3)",
        "temporal_activity": "wired (Phase 1.3)",
        "langchain_tool": "stub (capability-core)",
    }

    adapters = []
    for name in _ADAPTER_REGISTRY:
        entry = {
            "name": name,
            "enabled": True,
            "wiring": _WIRING_STATUS.get(name, "unknown"),
        }
        adapters.append(entry)

    return {"adapters": adapters}
