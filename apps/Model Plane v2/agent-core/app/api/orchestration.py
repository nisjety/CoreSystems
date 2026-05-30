"""Orchestration HTTP routes.

Todos, plans, approvals, agent-teams, control-actions, SSE event stream, resume.
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import AsyncGenerator

from fastapi import APIRouter, HTTPException, Query, Request
from fastapi.responses import StreamingResponse

from app import repository as repo
from app.domain import ApprovalStatus, PlanStatus, TodoStatus

router = APIRouter(tags=["orchestration"])
logger = logging.getLogger(__name__)


# ------------------------------------------------------------------
# Todos
# ------------------------------------------------------------------


@router.get("/todos")
async def list_todos(
    session_id: str = Query(...),
    run_id: str | None = Query(None),
) -> list[dict]:
    todos = await repo.list_todos(session_id=session_id, run_id=run_id)
    return [t.model_dump(mode="json") for t in todos]


@router.patch("/todos/{todo_id}")
async def update_todo(todo_id: str, body: dict) -> dict:
    new_status = body.get("status")
    if new_status is None:
        raise HTTPException(400, "status is required")
    try:
        status = TodoStatus(new_status)
    except ValueError:
        raise HTTPException(400, f"Invalid status: {new_status}")
    todo = await repo.update_todo_status(todo_id, status)
    if todo is None:
        raise HTTPException(404, "Todo not found")
    return todo.model_dump(mode="json")


# ------------------------------------------------------------------
# Plans
# ------------------------------------------------------------------


@router.get("/plans/{plan_id}")
async def get_plan(plan_id: str) -> dict:
    plan = await repo.get_plan(plan_id)
    if plan is None:
        raise HTTPException(404, "Plan not found")
    return plan.model_dump(mode="json")


@router.post("/plans/{plan_id}/approve")
async def approve_plan(plan_id: str) -> dict:
    from app.plan_mode import approve_plan as _approve
    from app.main import agent_service

    plan = await repo.get_plan(plan_id)
    if plan is None:
        raise HTTPException(404, "Plan not found")
    if plan.status != PlanStatus.PENDING:
        raise HTTPException(409, f"Plan already {plan.status.value}")

    plan = await _approve(plan_id, agent_service)
    return {"plan_id": plan.id, "status": plan.status.value}


@router.post("/plans/{plan_id}/reject")
async def reject_plan(plan_id: str) -> dict:
    from app.plan_mode import reject_plan as _reject
    from app.main import agent_service

    plan = await repo.get_plan(plan_id)
    if plan is None:
        raise HTTPException(404, "Plan not found")
    if plan.status != PlanStatus.PENDING:
        raise HTTPException(409, f"Plan already {plan.status.value}")

    plan = await _reject(plan_id, agent_service)
    return {"plan_id": plan.id, "status": plan.status.value}


# ------------------------------------------------------------------
# Approvals
# ------------------------------------------------------------------


@router.get("/approvals")
async def list_pending_approvals(
    session_id: str = Query(...),
) -> list[dict]:
    approvals = await repo.get_pending_approvals(session_id)
    return [a.model_dump(mode="json") for a in approvals]


@router.post("/approvals/{approval_id}/decide")
async def decide_approval(approval_id: str, body: dict) -> dict:
    decision = body.get("decision")  # "approved" | "denied"
    decided_by = body.get("user_id", "")
    if decision not in ("approved", "denied"):
        raise HTTPException(400, "decision must be 'approved' or 'denied'")

    from app.plan_mode import decide_approval as _decide
    result = await _decide(approval_id, ApprovalStatus(decision), decided_by)
    if result is None:
        raise HTTPException(404, "Approval not found")
    return result.model_dump(mode="json")


# ------------------------------------------------------------------
# Resume run
# ------------------------------------------------------------------


@router.post("/agent-runs/{run_id}/resume")
async def resume_run(run_id: str) -> dict:
    from app.main import agent_service
    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(404, "Run not found")
    await agent_service.resume_run(run_id)
    return {"run_id": run_id, "status": "resuming"}


# ------------------------------------------------------------------
# Cancel run
# ------------------------------------------------------------------


@router.post("/agent-runs/{run_id}/cancel")
async def cancel_run(run_id: str) -> dict:
    from app.main import agent_service
    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(404, "Run not found")
    await agent_service.cancel_run(run_id)
    return {"run_id": run_id, "status": "cancelled"}


# ------------------------------------------------------------------
# SSE event stream
# ------------------------------------------------------------------


@router.get("/agent-runs/{run_id}/events")
async def stream_events(run_id: str, request: Request) -> StreamingResponse:
    """Server-Sent Events stream for a run's lifecycle events."""
    from app.nats_client import NatsManager

    async def event_generator() -> AsyncGenerator[str, None]:
        nats = NatsManager()
        try:
            sub = await nats.subscribe_jetstream(
                subject=f"velion.agent.run.{run_id}.event",
                durable=f"sse-{run_id}",
                stream="VELION_AGENT",
            )
            while True:
                if await request.is_disconnected():
                    break
                try:
                    msg = await sub.next_msg(timeout=30)
                    data = json.loads(msg.data.decode())
                    yield f"event: {data.get('event_type', 'unknown')}\n"
                    yield f"data: {json.dumps(data)}\n\n"
                    await msg.ack()
                except asyncio.TimeoutError:
                    yield ": keepalive\n\n"
        finally:
            if sub:
                await sub.unsubscribe()

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


# ------------------------------------------------------------------
# Agent teams (list children)
# ------------------------------------------------------------------


@router.get("/agent-teams/{run_id}")
async def list_agent_team(run_id: str) -> dict:
    children = await repo.list_child_runs(run_id)
    return {
        "parent_run_id": run_id,
        "children": [c.model_dump(mode="json") for c in children],
        "count": len(children),
    }


# ------------------------------------------------------------------
# Control actions (manual dispatch)
# ------------------------------------------------------------------


@router.post("/control-actions")
async def dispatch_control_action(body: dict) -> dict:
    from app.coordinator import handle_control_action
    from app.main import agent_service
    from app.domain import AgentAction, ActionKind, ActionTarget

    run_id = body.get("run_id", "")
    action_name = body.get("action", "")
    action_input = body.get("input", {})

    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(404, "Run not found")

    action = AgentAction(
        kind=ActionKind.CONTROL,
        target=ActionTarget.AGENT_CORE,
        name=action_name,
        input=action_input,
    )
    result = await handle_control_action(action, run, agent_service)
    return {"action": action_name, "result": result}


# ------------------------------------------------------------------
# V2 SSE event stream (Phase 8 — EventStream-backed)
# ------------------------------------------------------------------


@router.get("/agent-runs/{run_id}/events/v2")
async def stream_events_v2(
    run_id: str,
    request: Request,
    from_seq: int = Query(0, ge=0, description="Resume from this sequence number"),
) -> StreamingResponse:
    """Server-Sent Events stream using the new RunEvent system (Phase 8).

    Supports replay: pass ?from_seq=N to resume after a disconnect.
    """
    from app.messages.streaming import sse_generator

    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(404, "Run not found")

    async def guarded_generator() -> AsyncGenerator[str, None]:
        async for chunk in sse_generator(run_id, run.session_id, from_seq=from_seq):
            if await request.is_disconnected():
                break
            yield chunk

    return StreamingResponse(
        guarded_generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


# ------------------------------------------------------------------
# Snapshots (Phase 3 — Snapshot + Teleport)
# ------------------------------------------------------------------


@router.get("/agent-runs/{run_id}/snapshot")
async def get_snapshot(run_id: str) -> dict:
    """Get the latest snapshot for a run."""
    from app.snapshot import get_latest_snapshot

    snapshot = await get_latest_snapshot(run_id)
    if snapshot is None:
        raise HTTPException(404, "No snapshot found")
    return snapshot


@router.post("/agent-runs/{run_id}/teleport")
async def teleport_run(run_id: str, body: dict) -> dict:
    """Resume a run on a different worker from its last snapshot."""
    from app.snapshot import teleport
    from app.main import agent_service

    new_worker = body.get("worker_id", f"teleport-{run_id[:8]}")
    result = await teleport(run_id, new_worker)
    if result is None:
        raise HTTPException(404, "No snapshot available for teleport")

    run, conversation, turn_index = result
    # Re-execute from the snapshot
    asyncio.create_task(agent_service.execute_run(run.id))

    return {
        "run_id": run_id,
        "status": "teleporting",
        "resume_turn": turn_index,
        "worker_id": new_worker,
    }


# ------------------------------------------------------------------
# Run event history (Phase 0 — replay stored events)
# ------------------------------------------------------------------


@router.get("/agent-runs/{run_id}/event-history")
async def get_event_history(
    run_id: str,
    from_seq: int = Query(0, ge=0),
    event_type: str | None = Query(None),
    limit: int = Query(100, ge=1, le=1000),
) -> dict:
    """Retrieve stored RunEvents for a run."""
    from app.messages.store import replay, count_events

    events = await replay(
        run_id,
        from_seq=from_seq,
        event_type=event_type,
        limit=limit,
    )
    total = await count_events(run_id)

    return {
        "run_id": run_id,
        "events": [e.model_dump(mode="json") for e in events],
        "count": len(events),
        "total": total,
    }


# ------------------------------------------------------------------
# Built-in agents (Phase 5)
# ------------------------------------------------------------------


@router.get("/agents/builtin")
async def list_builtin_agents() -> list[dict]:
    """List all available built-in agent archetypes."""
    from app.builtin_agents import list_built_in_agents

    return [
        {
            "name": a.name,
            "description": a.description,
            "model": a.model,
            "max_turns": a.max_turns,
            "allowed_tools": a.allowed_tools,
        }
        for a in list_built_in_agents()
    ]


@router.get("/agents/builtin/{agent_name}")
async def get_builtin_agent(agent_name: str) -> dict:
    """Get a specific built-in agent by name."""
    from app.builtin_agents import get_built_in_agent

    agent = get_built_in_agent(agent_name)
    if agent is None:
        raise HTTPException(404, f"Built-in agent '{agent_name}' not found")
    return {
        "name": agent.name,
        "description": agent.description,
        "model": agent.model,
        "system_prompt": agent.system_prompt,
        "max_turns": agent.max_turns,
        "allowed_tools": agent.allowed_tools,
    }


# ------------------------------------------------------------------
# Sub-agent dispatch (Phase 5 — fork-based execution)
# ------------------------------------------------------------------


@router.post("/agent-runs/{run_id}/subagent")
async def spawn_subagent(run_id: str, body: dict) -> dict:
    """Fork a sub-agent from a parent run."""
    from app.subagent import fork_subagent, SubagentType

    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(404, "Run not found")

    agent_name = body.get("agent_name")
    prompt = body.get("prompt", "")
    agent_type = body.get("type", "local_agent")

    if not prompt:
        raise HTTPException(400, "prompt is required")

    try:
        agent_type_enum = SubagentType(agent_type)
    except ValueError:
        raise HTTPException(400, f"Invalid agent type: {agent_type}")

    from app.main import agent_service

    sub_state = await fork_subagent(
        parent_run=run,
        prompt=prompt,
        agent_name=agent_name,
        agent_type=agent_type_enum,
        llm_client=agent_service._llm,
        capability_client=agent_service._capability,
        execute_action_fn=agent_service._execute_action,
        publisher=agent_service._publisher,
    )

    return {
        "subagent_id": sub_state.agent_id,
        "status": sub_state.status.value,
        "agent_name": sub_state.agent_name,
        "parent_run_id": run_id,
    }
