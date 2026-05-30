"""Postgres repository for agent_tasks — CRUD, claiming, blocking."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

import asyncpg

from app.database import get_pool
from app.tasks.domain import (
    ClaimResult,
    TaskRecord,
    TaskStatus,
    TaskUpdate,
    is_terminal,
)

logger = logging.getLogger(__name__)


def _now() -> datetime:
    return datetime.now(timezone.utc)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _row_to_task(row: asyncpg.Record) -> TaskRecord:
    meta_raw = row["metadata"]
    meta = json.loads(meta_raw) if isinstance(meta_raw, str) else meta_raw
    return TaskRecord(
        id=row["id"],
        run_id=row["run_id"],
        session_id=row["session_id"],
        org_id=row["org_id"],
        subject=row["subject"],
        description=row["description"],
        status=row["status"],
        owner_agent_id=row["owner_agent_id"],
        blocks=list(row["blocks"] or []),
        blocked_by=list(row["blocked_by"] or []),
        metadata=meta or {},
        output=row["output"],
        error=row["error"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


# ---------------------------------------------------------------------------
# Create
# ---------------------------------------------------------------------------


async def create_task(task: TaskRecord) -> TaskRecord:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO agent_tasks (
                id, run_id, session_id, org_id,
                subject, description, status,
                owner_agent_id, blocks, blocked_by,
                metadata, output, error,
                created_at, updated_at
            ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15
            )
            """,
            task.id,
            task.run_id,
            task.session_id,
            task.org_id,
            task.subject,
            task.description,
            task.status.value,
            task.owner_agent_id,
            task.blocks,
            task.blocked_by,
            json.dumps(task.metadata),
            task.output,
            task.error,
            task.created_at,
            task.updated_at,
        )
    return task


# ---------------------------------------------------------------------------
# Read
# ---------------------------------------------------------------------------


async def get_task(task_id: str) -> TaskRecord | None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow("SELECT * FROM agent_tasks WHERE id = $1", task_id)
    return _row_to_task(row) if row else None


async def list_tasks(run_id: str) -> list[TaskRecord]:
    """List all non-deleted tasks for a run."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM agent_tasks WHERE run_id = $1 AND status != 'deleted' ORDER BY created_at",
            run_id,
        )
    return [_row_to_task(r) for r in rows]


async def list_tasks_by_owner(owner_agent_id: str, run_id: str) -> list[TaskRecord]:
    """Tasks owned by a specific agent within a run."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """SELECT * FROM agent_tasks
               WHERE run_id = $1 AND owner_agent_id = $2 AND status NOT IN ('deleted','completed','failed','killed')
               ORDER BY created_at""",
            run_id,
            owner_agent_id,
        )
    return [_row_to_task(r) for r in rows]


# ---------------------------------------------------------------------------
# Update
# ---------------------------------------------------------------------------


async def update_task(task_id: str, update: TaskUpdate) -> TaskRecord | None:
    """Apply a partial update to a task. Returns updated task or None."""
    pool = await get_pool()
    now = _now()

    async with pool.acquire() as conn:
        async with conn.transaction():
            row = await conn.fetchrow(
                "SELECT * FROM agent_tasks WHERE id = $1 FOR UPDATE",
                task_id,
            )
            if row is None:
                return None

            task = _row_to_task(row)
            parts: list[str] = ["updated_at = $2"]
            args: list[Any] = [task_id, now]
            idx = 3

            if update.subject is not None:
                parts.append(f"subject = ${idx}")
                args.append(update.subject)
                idx += 1

            if update.description is not None:
                parts.append(f"description = ${idx}")
                args.append(update.description)
                idx += 1

            if update.status is not None:
                parts.append(f"status = ${idx}")
                args.append(update.status.value)
                idx += 1

            if update.owner_agent_id is not None:
                parts.append(f"owner_agent_id = ${idx}")
                args.append(update.owner_agent_id)
                idx += 1

            if update.output is not None:
                parts.append(f"output = ${idx}")
                args.append(update.output)
                idx += 1

            if update.error is not None:
                parts.append(f"error = ${idx}")
                args.append(update.error)
                idx += 1

            if update.metadata is not None:
                merged = {**task.metadata, **update.metadata}
                parts.append(f"metadata = ${idx}")
                args.append(json.dumps(merged))
                idx += 1

            # Blocking: additive merge
            new_blocks = list(task.blocks)
            new_blocked_by = list(task.blocked_by)

            if update.add_blocks:
                new_blocks = list(set(new_blocks) | set(update.add_blocks))
                parts.append(f"blocks = ${idx}")
                args.append(new_blocks)
                idx += 1

            if update.add_blocked_by:
                new_blocked_by = list(set(new_blocked_by) | set(update.add_blocked_by))
                parts.append(f"blocked_by = ${idx}")
                args.append(new_blocked_by)
                idx += 1

            # Handle deletion cascade: remove refs from other tasks
            if update.status == TaskStatus.DELETED:
                await _cascade_delete_refs(conn, task_id, task.run_id)

            sql = f"UPDATE agent_tasks SET {', '.join(parts)} WHERE id = $1"
            await conn.execute(sql, *args)

            # Bidirectional block sync
            if update.add_blocks:
                for target_id in update.add_blocks:
                    await conn.execute(
                        """UPDATE agent_tasks
                           SET blocked_by = array_append(blocked_by, $1)
                           WHERE id = $2 AND NOT ($1 = ANY(blocked_by))""",
                        task_id,
                        target_id,
                    )

            if update.add_blocked_by:
                for blocker_id in update.add_blocked_by:
                    await conn.execute(
                        """UPDATE agent_tasks
                           SET blocks = array_append(blocks, $1)
                           WHERE id = $2 AND NOT ($1 = ANY(blocks))""",
                        task_id,
                        blocker_id,
                    )

    return await get_task(task_id)


async def _cascade_delete_refs(
    conn: asyncpg.Connection, task_id: str, run_id: str
) -> None:
    """Remove task_id from all blocks/blocked_by arrays in the same run."""
    await conn.execute(
        """UPDATE agent_tasks
           SET blocks = array_remove(blocks, $1)
           WHERE run_id = $2 AND $1 = ANY(blocks)""",
        task_id,
        run_id,
    )
    await conn.execute(
        """UPDATE agent_tasks
           SET blocked_by = array_remove(blocked_by, $1)
           WHERE run_id = $2 AND $1 = ANY(blocked_by)""",
        task_id,
        run_id,
    )


# ---------------------------------------------------------------------------
# Claim (atomic ownership acquisition — CC claimTask pattern)
# ---------------------------------------------------------------------------


async def claim_task(
    task_id: str,
    agent_id: str,
    run_id: str,
) -> ClaimResult:
    """Atomically claim a task for an agent.

    Checks:
    1. Task exists and is pending.
    2. Not already owned.
    3. All blockers are terminal.
    4. Agent doesn't own other non-terminal tasks in same run.
    """
    pool = await get_pool()

    async with pool.acquire() as conn:
        async with conn.transaction():
            row = await conn.fetchrow(
                "SELECT * FROM agent_tasks WHERE id = $1 FOR UPDATE",
                task_id,
            )
            if row is None:
                return ClaimResult(success=False, reason="task_not_found")

            task = _row_to_task(row)

            if is_terminal(task.status):
                return ClaimResult(success=False, reason="already_resolved", task=task)

            if task.owner_agent_id and task.owner_agent_id != agent_id:
                return ClaimResult(success=False, reason="already_claimed", task=task)

            # Check blockers
            if task.blocked_by:
                blocker_rows = await conn.fetch(
                    "SELECT id, status FROM agent_tasks WHERE id = ANY($1::text[])",
                    task.blocked_by,
                )
                non_terminal = [
                    r["id"]
                    for r in blocker_rows
                    if not is_terminal(TaskStatus(r["status"]))
                ]
                if non_terminal:
                    return ClaimResult(
                        success=False,
                        reason="blocked",
                        task=task,
                        blocked_by_tasks=non_terminal,
                    )

            # Check agent not busy
            busy = await conn.fetch(
                """SELECT id FROM agent_tasks
                   WHERE run_id = $1 AND owner_agent_id = $2
                     AND status NOT IN ('completed','failed','killed','deleted')
                     AND id != $3""",
                run_id,
                agent_id,
                task_id,
            )
            if busy:
                return ClaimResult(
                    success=False,
                    reason="agent_busy",
                    task=task,
                )

            # Claim it
            now = _now()
            await conn.execute(
                "UPDATE agent_tasks SET owner_agent_id = $1, status = 'in_progress', updated_at = $2 WHERE id = $3",
                agent_id,
                now,
                task_id,
            )

            task.owner_agent_id = agent_id
            task.status = TaskStatus.IN_PROGRESS
            task.updated_at = now
            return ClaimResult(success=True, task=task)


# ---------------------------------------------------------------------------
# Delete
# ---------------------------------------------------------------------------


async def delete_task(task_id: str) -> bool:
    """Soft-delete a task (set status to deleted, clean up refs)."""
    result = await update_task(
        task_id,
        TaskUpdate(status=TaskStatus.DELETED),
    )
    return result is not None
