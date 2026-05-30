"""Temporal activities — the actual work units that run outside the workflow sandbox.

Each activity is an async function that performs real I/O:
  - ``plan_actions``: call LLM to plan execution steps
  - ``execute_action``: dispatch a single tool/reasoning/control action
  - ``checkpoint_run``: persist run state to Postgres
  - ``finalize_run``: mark run completed and publish events

Activities are heartbeat-aware: long-running tool calls emit heartbeats
so Temporal can detect stalled workers and retry on a different one.
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel
from temporalio import activity

logger = logging.getLogger(__name__)


# ------------------------------------------------------------------
# Activity input / output models (Pydantic → Temporal serialisation)
# ------------------------------------------------------------------


class PlanInput(BaseModel):
    """Input for the plan_actions activity."""

    run_id: str
    goal: str
    agent_type: str
    org_id: str | None = None
    session_id: str = ""
    loaded_tool_names: list[str] = []
    max_actions: int = 10


class ActionInput(BaseModel):
    """Input for the execute_action activity."""

    run_id: str
    session_id: str
    org_id: str | None = None
    user_id: str = ""
    action_kind: str
    action_name: str
    action_input: dict[str, Any] = {}
    action_id: str = ""


class ActionOutput(BaseModel):
    """Result of an action execution."""

    action_id: str
    status: str  # "completed" | "failed" | "skipped"
    output: Any = None
    error: str | None = None


class CheckpointInput(BaseModel):
    """Input for the checkpoint_run activity."""

    run_id: str
    action_index: int
    status: str = "running"


class FinalizeInput(BaseModel):
    """Input for the finalize_run activity."""

    run_id: str
    status: str  # "completed" | "failed"
    final_output: str | None = None
    error: str | None = None


# ------------------------------------------------------------------
# Activity implementations
# ------------------------------------------------------------------


@activity.defn
async def plan_actions(input: PlanInput) -> list[dict[str, Any]]:
    """Call the LLM planner to produce an action plan.

    Returns a list of action dicts (serialisable) for the workflow to
    iterate over.
    """
    activity.heartbeat("planning_started")

    # Import inside activity to avoid sandbox issues
    from app.agent_service import AgentService
    from app import repository as repo
    from app.config import settings

    run = await repo.get_run(input.run_id)
    if run is None:
        raise ValueError(f"Run {input.run_id} not found")

    # Re-hydrate the agent service from the activity context
    svc: AgentService = activity.info().activity_type  # type: ignore[assignment]
    # Actually we need to get the service differently — use a module-level ref
    from app.workflows._state import get_agent_service

    svc = get_agent_service()

    if settings.pydantic_ai_enabled:
        actions = await svc._plan_actions_pydantic(run)
    else:
        actions = await svc._plan_actions(run)

    activity.heartbeat("planning_complete")

    # Serialise domain objects → plain dicts for Temporal
    return [
        {
            "id": a.id,
            "kind": a.kind.value,
            "target": a.target.value,
            "name": a.name,
            "description": a.description,
            "input": a.input,
        }
        for a in actions
    ]


@activity.defn
async def execute_action(input: ActionInput) -> ActionOutput:
    """Execute a single action via the agent service pipeline.

    Includes permission checks, hooks, tool dispatch, and cost tracking.
    """
    activity.heartbeat(f"executing_{input.action_name}")

    from app import repository as repo
    from app.domain import ActionKind, ActionTarget, AgentAction
    from app.workflows._state import get_agent_service

    svc = get_agent_service()
    run = await repo.get_run(input.run_id)
    if run is None:
        raise ValueError(f"Run {input.run_id} not found")

    action = AgentAction(
        kind=ActionKind(input.action_kind),
        target=ActionTarget.INTERNAL,
        name=input.action_name,
        input=input.action_input,
    )
    if input.action_id:
        action.id = input.action_id

    try:
        result = await svc._execute_action(run, action)
        return ActionOutput(
            action_id=result.id,
            status=result.status.value,
            output=result.output,
            error=result.error,
        )
    except Exception as exc:
        logger.error(
            "activity_execute_action_failed",
            extra={"run_id": input.run_id, "action": input.action_name, "error": str(exc)},
        )
        return ActionOutput(
            action_id=action.id,
            status="failed",
            error=str(exc),
        )


@activity.defn
async def checkpoint_run(input: CheckpointInput) -> None:
    """Persist current run progress to Postgres."""
    from app import repository as repo
    from app.domain import RunStatus

    await repo.update_run_status(
        input.run_id,
        RunStatus(input.status),
        current_action_index=input.action_index,
        checkpoint_index=input.action_index,
    )
    activity.heartbeat(f"checkpoint_{input.action_index}")


@activity.defn
async def finalize_run(input: FinalizeInput) -> None:
    """Mark a run completed/failed and publish completion events."""
    from app import repository as repo
    from app.domain import RunStatus
    from app.workflows._state import get_agent_service

    svc = get_agent_service()
    run = await repo.get_run(input.run_id)
    if run is None:
        return

    status = RunStatus(input.status)
    await repo.update_run_status(
        input.run_id,
        status,
        final_output=input.final_output,
        error=input.error,
        lease_owner=None,
    )

    run.status = status
    run.final_output = input.final_output
    run.error = input.error

    if status == RunStatus.COMPLETED:
        await svc._publisher.run_completed(run)
    else:
        await svc._publisher.run_failed(run)
