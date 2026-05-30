"""Postgres repository — CRUD for runs, todos, plans, approvals."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

import asyncpg

from app.database import get_pool
from app.domain import (
    AgentAction,
    ApprovalRecord,
    ApprovalStatus,
    ExecutionPolicy,
    PlanRecord,
    PlanStatus,
    RunRecord,
    RunStatus,
    TodoItem,
    TodoStatus,
)

logger = logging.getLogger(__name__)


def _now() -> datetime:
    return datetime.now(timezone.utc)


# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------


async def create_run(run: RunRecord) -> RunRecord:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO agent_runs (
                id, session_id, parent_run_id, user_id, org_id,
                agent_type, mode, goal, status, policy,
                plan_state, actions, current_action_idx,
                checkpoint_index, checkpoint_state,
                tool_pool_version, loaded_tool_names, lease_owner,
                final_output, error, metadata, created_at, updated_at
            ) VALUES (
                $1,$2,$3,$4,$5,
                $6,$7,$8,$9,$10,
                $11,$12,$13,
                $14,$15,
                $16,$17,$18,
                $19,$20,$21,$22,$23
            )
            """,
            run.id,
            run.session_id,
            run.parent_run_id,
            run.user_id,
            run.org_id,
            run.agent_type.value,
            run.mode.value,
            run.goal,
            run.status.value,
            json.dumps(run.policy.model_dump()),
            json.dumps(run.plan_state) if run.plan_state else None,
            json.dumps([a.model_dump() for a in run.actions]),
            run.current_action_index,
            run.checkpoint_index,
            json.dumps(run.checkpoint_state) if run.checkpoint_state else None,
            run.tool_pool_version,
            run.loaded_tool_names,
            run.lease_owner,
            run.final_output,
            run.error,
            json.dumps(run.metadata),
            run.created_at,
            run.updated_at,
        )
    return run


async def get_run(run_id: str) -> RunRecord | None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow("SELECT * FROM agent_runs WHERE id = $1", run_id)
    if row is None:
        return None
    return _row_to_run(row)


async def update_run_status(
    run_id: str,
    status: RunStatus,
    *,
    final_output: str | None = None,
    error: str | None = None,
    actions: list[AgentAction] | None = None,
    current_action_index: int | None = None,
    checkpoint_index: int | None = None,
    checkpoint_state: dict[str, Any] | None = None,
    metadata: dict[str, Any] | None = None,
    lease_owner: str | None = ...,  # type: ignore[assignment]
) -> None:
    """Partial update on a run. Only non-None fields are written."""
    pool = await get_pool()
    parts: list[str] = ["status = $2"]
    args: list[Any] = [run_id, status.value]
    idx = 3

    for col, val in [
        ("final_output", final_output),
        ("error", error),
    ]:
        if val is not None:
            parts.append(f"{col} = ${idx}")
            args.append(val)
            idx += 1

    if actions is not None:
        parts.append(f"actions = ${idx}")
        args.append(json.dumps([a.model_dump() for a in actions]))
        idx += 1

    if current_action_index is not None:
        parts.append(f"current_action_idx = ${idx}")
        args.append(current_action_index)
        idx += 1

    if checkpoint_index is not None:
        parts.append(f"checkpoint_index = ${idx}")
        args.append(checkpoint_index)
        idx += 1

    if checkpoint_state is not None:
        parts.append(f"checkpoint_state = ${idx}")
        args.append(json.dumps(checkpoint_state))
        idx += 1

    if metadata is not None:
        parts.append(f"metadata = ${idx}")
        args.append(json.dumps(metadata))
        idx += 1

    if lease_owner is not ...:
        parts.append(f"lease_owner = ${idx}")
        args.append(lease_owner)
        idx += 1

    sql = f"UPDATE agent_runs SET {', '.join(parts)} WHERE id = $1"
    async with pool.acquire() as conn:
        await conn.execute(sql, *args)


async def list_runs_by_session(session_id: str, limit: int = 50) -> list[RunRecord]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM agent_runs WHERE session_id = $1 ORDER BY created_at DESC LIMIT $2",
            session_id,
            limit,
        )
    return [_row_to_run(r) for r in rows]


async def list_child_runs(parent_run_id: str) -> list[RunRecord]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM agent_runs WHERE parent_run_id = $1 ORDER BY created_at",
            parent_run_id,
        )
    return [_row_to_run(r) for r in rows]


def _row_to_run(row: asyncpg.Record) -> RunRecord:
    policy_raw = row["policy"]
    policy_dict = json.loads(policy_raw) if isinstance(policy_raw, str) else policy_raw
    actions_raw = row["actions"]
    actions_list = json.loads(actions_raw) if isinstance(actions_raw, str) else actions_raw
    cp_raw = row["checkpoint_state"]
    cp_dict = json.loads(cp_raw) if isinstance(cp_raw, str) else cp_raw
    ps_raw = row["plan_state"]
    ps_dict = json.loads(ps_raw) if isinstance(ps_raw, str) else ps_raw
    meta_raw = row["metadata"]
    meta_dict = json.loads(meta_raw) if isinstance(meta_raw, str) else meta_raw

    return RunRecord(
        id=row["id"],
        session_id=row["session_id"],
        parent_run_id=row["parent_run_id"],
        user_id=row["user_id"],
        org_id=row["org_id"],
        agent_type=row["agent_type"],
        mode=row["mode"],
        goal=row["goal"],
        status=row["status"],
        policy=ExecutionPolicy(**policy_dict),
        plan_state=ps_dict,
        actions=[AgentAction(**a) for a in (actions_list or [])],
        current_action_index=row["current_action_idx"],
        checkpoint_index=row["checkpoint_index"],
        checkpoint_state=cp_dict,
        tool_pool_version=row["tool_pool_version"],
        loaded_tool_names=list(row["loaded_tool_names"] or []),
        lease_owner=row["lease_owner"],
        final_output=row["final_output"],
        error=row["error"],
        metadata=meta_dict or {},
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


# ---------------------------------------------------------------------------
# Todos
# ---------------------------------------------------------------------------


async def create_todo(todo: TodoItem) -> TodoItem:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO todos (id, session_id, run_id, content, status, owner_agent_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            """,
            todo.id,
            todo.session_id,
            todo.run_id,
            todo.content,
            todo.status.value,
            todo.owner_agent_id,
            todo.created_at,
            todo.updated_at,
        )
    return todo


async def update_todo_status(todo_id: str, status: TodoStatus) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            "UPDATE todos SET status = $1 WHERE id = $2",
            status.value,
            todo_id,
        )


async def list_todos(session_id: str, run_id: str | None = None) -> list[TodoItem]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        if run_id:
            rows = await conn.fetch(
                "SELECT * FROM todos WHERE session_id = $1 AND run_id = $2 ORDER BY created_at",
                session_id,
                run_id,
            )
        else:
            rows = await conn.fetch(
                "SELECT * FROM todos WHERE session_id = $1 ORDER BY created_at",
                session_id,
            )
    return [
        TodoItem(
            id=r["id"],
            session_id=r["session_id"],
            run_id=r["run_id"],
            content=r["content"],
            status=r["status"],
            owner_agent_id=r["owner_agent_id"],
            created_at=r["created_at"],
            updated_at=r["updated_at"],
        )
        for r in rows
    ]


# ---------------------------------------------------------------------------
# Plans
# ---------------------------------------------------------------------------


async def create_plan(plan: PlanRecord) -> PlanRecord:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO plans (id, session_id, run_id, status, steps, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            """,
            plan.id,
            plan.session_id,
            plan.run_id,
            plan.status.value,
            json.dumps(plan.steps),
            plan.created_at,
            plan.updated_at,
        )
    return plan


async def update_plan_status(plan_id: str, status: PlanStatus) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            "UPDATE plans SET status = $1 WHERE id = $2",
            status.value,
            plan_id,
        )


async def get_plan(plan_id: str) -> PlanRecord | None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow("SELECT * FROM plans WHERE id = $1", plan_id)
    if row is None:
        return None
    steps_raw = row["steps"]
    steps = json.loads(steps_raw) if isinstance(steps_raw, str) else steps_raw
    return PlanRecord(
        id=row["id"],
        session_id=row["session_id"],
        run_id=row["run_id"],
        status=row["status"],
        steps=steps or [],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


# ---------------------------------------------------------------------------
# Approvals
# ---------------------------------------------------------------------------


async def create_approval(approval: ApprovalRecord) -> ApprovalRecord:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO approvals (id, session_id, run_id, action_id, action_name, reason, status, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            """,
            approval.id,
            approval.session_id,
            approval.run_id,
            approval.action_id,
            approval.action_name,
            approval.reason,
            approval.status.value,
            approval.created_at,
        )
    return approval


async def resolve_approval(
    approval_id: str,
    status: ApprovalStatus,
    decided_by: str,
) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            "UPDATE approvals SET status = $1, decided_by = $2, decided_at = $3 WHERE id = $4",
            status.value,
            decided_by,
            _now(),
            approval_id,
        )


async def get_pending_approvals(run_id: str) -> list[ApprovalRecord]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM approvals WHERE run_id = $1 AND status = 'pending' ORDER BY created_at",
            run_id,
        )
    return [
        ApprovalRecord(
            id=r["id"],
            session_id=r["session_id"],
            run_id=r["run_id"],
            action_id=r["action_id"],
            action_name=r["action_name"],
            reason=r["reason"],
            status=r["status"],
            decided_by=r["decided_by"],
            created_at=r["created_at"],
            decided_at=r["decided_at"],
        )
        for r in rows
    ]
