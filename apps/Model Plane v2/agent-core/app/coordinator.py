"""Coordinator/worker model for multi-agent runs.

A coordinator run can spawn worker runs, each with their own lease.
Workers execute independently and report back via NATS events.
The coordinator monitors child completions and aggregates results.
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any
from uuid import uuid4

from app import repository as repo
from app.coordination_result import CoordinationResult
from app.domain import (
    AgentAction,
    AgentType,
    CreateRunRequest,
    ExecutionPolicy,
    RunMode,
    RunRecord,
    TodoItem,
    TodoStatus,
)

if TYPE_CHECKING:
    from app.agent_service import AgentService

logger = logging.getLogger(__name__)


async def handle_control_action(
    run: RunRecord,
    action: AgentAction,
    service: AgentService,
) -> dict[str, Any]:
    """Dispatch control-plane orchestration primitives.

    Control actions originate from the agent planner and are executed
    locally rather than via ai-core.  Supported names:

    - spawn_agent: create a child worker run
    - spawn_agents_parallel: spawn multiple workers concurrently (Phase I)
    - todo_write: create/update todo items
    - ask_user_question: publish a question event (blocks run)
    - send_message: publish a message event
    - synthesize: aggregate child results into a summary (Phase I)
    - run_in_background: spawn a fire-and-forget worker (Phase I)
    - exit_plan_mode: transition plan → execute
    """
    name = action.name
    inp = action.input

    if name == "spawn_agent":
        return await _spawn_worker(run, inp, service)
    elif name == "spawn_agents_parallel":
        return await _spawn_agents_parallel(run, inp, service)
    elif name == "todo_write":
        return await _write_todo(run, inp, service)
    elif name == "ask_user_question":
        return await _ask_user(run, inp, service)
    elif name == "send_message":
        return await _send_message(run, inp, service)
    elif name == "synthesize":
        return await _synthesize(run, inp, service)
    elif name == "run_in_background":
        return await _run_in_background(run, inp, service)
    elif name == "exit_plan_mode":
        return {"acknowledged": True}
    else:
        logger.warning("unknown_control_action", extra={"name": name})
        return {"error": f"Unknown control action: {name}"}


async def _spawn_worker(
    parent: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Spawn a child worker run under the coordinator."""
    request = CreateRunRequest(
        goal=inp.get("goal", parent.goal),
        mode=RunMode(inp.get("mode", "execute")),
        agent_type=AgentType.WORKER,
        policy=ExecutionPolicy(
            allowed_tools=inp.get("allowed_tools", parent.policy.allowed_tools),
            max_actions=inp.get("max_actions", parent.policy.max_actions),
            approval_mode=parent.policy.approval_mode,
        ),
        parent_run_id=parent.id,
        context=inp.get("context", {}),
    )

    child = await service.create_run(
        request, parent.session_id, parent.user_id, parent.org_id
    )
    logger.info(
        "worker_spawned",
        extra={"parent_id": parent.id, "child_id": child.id},
    )

    # Fire-and-forget execution (async task in production, inline here)
    # The child will acquire its own lease
    import asyncio

    asyncio.create_task(
        service.execute_run(child.id),
        name=f"worker-{child.id}",
    )

    return {"child_run_id": child.id, "status": "spawned"}


async def _write_todo(
    run: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Create or update todo items for the session."""
    items = inp.get("items", [])
    created_ids: list[str] = []

    for item in items:
        todo = TodoItem(
            session_id=run.session_id,
            run_id=run.id,
            content=item.get("content", ""),
            status=TodoStatus(item.get("status", "pending")),
            owner_agent_id=item.get("owner_agent_id"),
        )
        await repo.create_todo(todo)
        created_ids.append(todo.id)

        await service._publisher.todo_updated(
            run.id, run.session_id, todo.id, todo.content, todo.status.value
        )

    return {"todo_ids": created_ids, "count": len(created_ids)}


async def _ask_user(
    run: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Publish a question event for the user and return immediately.

    The run should be paused (status=awaiting_approval) by the caller
    so the user can respond via the resume endpoint.
    """
    question = inp.get("question", "")
    from app.domain import AgentEvent, get_query_depth

    await service._publisher.publish(
        AgentEvent(
            event_type="user.question",
            run_id=run.id,
            session_id=run.session_id,
            payload={"question": question, "query_depth": get_query_depth(run.metadata)},
        )
    )
    return {"question_sent": True, "question": question}


async def _send_message(
    run: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Publish a notification/message event to the session."""
    message = inp.get("message", "")
    from app.domain import AgentEvent, get_query_depth

    await service._publisher.publish(
        AgentEvent(
            event_type="agent.message",
            run_id=run.id,
            session_id=run.session_id,
            payload={"message": message, "query_depth": get_query_depth(run.metadata)},
        )
    )
    return {"message_sent": True}


async def list_child_runs(parent_run_id: str) -> list[RunRecord]:
    """Retrieve all child runs spawned by a coordinator."""
    return await repo.list_child_runs(parent_run_id)


async def aggregate_worker_results(parent_run_id: str) -> CoordinationResult:
    """Summarize status of all child worker runs as a typed result."""
    children = await repo.list_child_runs(parent_run_id)
    return CoordinationResult.from_children(children)


# ---------------------------------------------------------------------------
# Phase I: Enhanced coordinator primitives
# ---------------------------------------------------------------------------


async def _spawn_agents_parallel(
    parent: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Spawn multiple child workers concurrently.

    Input format:
      {
        "agents": [
          {"agent_id": "security-reviewer", "goal": "...", "allowed_tools": [...]},
          {"agent_id": "performance-reviewer", "goal": "...", "mode": "reactive"},
        ]
      }

    Each agent gets a named ID for tracking. All are spawned in parallel
    via asyncio.gather, matching the CC multi-agent pattern.
    """
    import asyncio

    agents = inp.get("agents", [])
    if not agents:
        return {"error": "No agents specified"}

    child_ids: list[dict[str, str]] = []

    async def _spawn_one(agent_spec: dict[str, Any]) -> dict[str, str]:
        agent_id = agent_spec.get("agent_id", str(uuid4()))
        goal = agent_spec.get("goal", parent.goal)
        mode = RunMode(agent_spec.get("mode", "execute"))

        request = CreateRunRequest(
            goal=goal,
            mode=mode,
            agent_type=AgentType.WORKER,
            policy=ExecutionPolicy(
                allowed_tools=agent_spec.get("allowed_tools", parent.policy.allowed_tools),
                max_actions=agent_spec.get("max_actions", parent.policy.max_actions),
                max_turns=agent_spec.get("max_turns", parent.policy.max_turns),
                approval_mode=parent.policy.approval_mode,
            ),
            parent_run_id=parent.id,
            context={
                "agent_id": agent_id,
                **(agent_spec.get("context", {})),
            },
        )

        child = await service.create_run(
            request, parent.session_id, parent.user_id, parent.org_id
        )

        # Fire off execution
        asyncio.create_task(
            service.execute_run(child.id),
            name=f"worker-{agent_id}-{child.id}",
        )

        return {"agent_id": agent_id, "run_id": child.id}

    results = await asyncio.gather(
        *[_spawn_one(spec) for spec in agents],
        return_exceptions=True,
    )

    for r in results:
        if isinstance(r, Exception):
            logger.error("parallel_spawn_failed", extra={"error": str(r)})
            child_ids.append({"agent_id": "error", "run_id": str(r)})
        else:
            child_ids.append(r)

    logger.info(
        "agents_spawned_parallel",
        extra={"parent": parent.id, "count": len(child_ids)},
    )

    return {"children": child_ids, "count": len(child_ids)}


async def _synthesize(
    run: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Aggregate child worker results into a synthesis.

    If a custom synthesis prompt is provided, uses the LLM to combine
    child outputs. Otherwise returns raw aggregation.
    """
    result = await aggregate_worker_results(run.id)

    synthesis_prompt = inp.get("prompt")
    if synthesis_prompt and result.outputs:
        # Use LLM to synthesize
        outputs_block = "\n\n---\n\n".join(
            f"Agent {i+1}:\n{output}"
            for i, output in enumerate(result.outputs)
        )
        messages = [
            {"role": "system", "content": synthesis_prompt},
            {"role": "user", "content": outputs_block},
        ]
        synthesis = await service._llm.planner_complete(messages)
        return result.model_copy(update={"synthesis": synthesis}).model_dump()

    return result.model_dump()


async def _run_in_background(
    parent: RunRecord,
    inp: dict[str, Any],
    service: AgentService,
) -> dict[str, Any]:
    """Spawn a fire-and-forget background worker.

    Unlike spawn_agent which the coordinator may monitor, background
    runs are independent. The parent doesn't wait for them.
    """
    import asyncio

    agent_id = inp.get("agent_id", f"bg-{uuid4().hex[:8]}")
    goal = inp.get("goal", "")
    if not goal:
        return {"error": "goal is required"}

    request = CreateRunRequest(
        goal=goal,
        mode=RunMode(inp.get("mode", "execute")),
        agent_type=AgentType.WORKER,
        policy=ExecutionPolicy(
            allowed_tools=inp.get("allowed_tools", []),
            max_actions=inp.get("max_actions", 10),
            max_turns=inp.get("max_turns", 20),
            approval_mode=parent.policy.approval_mode,
        ),
        parent_run_id=parent.id,
        context={"agent_id": agent_id, "background": True},
    )

    child = await service.create_run(
        request, parent.session_id, parent.user_id, parent.org_id
    )

    asyncio.create_task(
        service.execute_run(child.id),
        name=f"bg-{agent_id}-{child.id}",
    )

    logger.info(
        "background_worker_spawned",
        extra={"parent": parent.id, "child": child.id, "agent_id": agent_id},
    )

    return {"child_run_id": child.id, "agent_id": agent_id, "background": True}
