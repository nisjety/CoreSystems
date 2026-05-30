"""Postgres repository for agent_cron_tasks."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

import asyncpg

from app.cron.domain import CronTask
from app.database import get_pool

logger = logging.getLogger(__name__)


def _now() -> datetime:
    return datetime.now(timezone.utc)


def _row_to_cron(row: asyncpg.Record) -> CronTask:
    policy_raw = row["policy"]
    policy = json.loads(policy_raw) if isinstance(policy_raw, str) else policy_raw
    return CronTask(
        id=row["id"],
        org_id=row["org_id"],
        session_id=row["session_id"],
        name=row["name"],
        cron_expr=row["cron_expr"],
        goal=row["goal"],
        policy=policy or {},
        enabled=row["enabled"],
        last_run_at=row["last_run_at"],
        next_run_at=row["next_run_at"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )


async def create_cron(task: CronTask) -> CronTask:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO agent_cron_tasks (
                id, org_id, session_id, name, cron_expr, goal,
                policy, enabled, last_run_at, next_run_at,
                created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
            """,
            task.id,
            task.org_id,
            task.session_id,
            task.name,
            task.cron_expr,
            task.goal,
            json.dumps(task.policy),
            task.enabled,
            task.last_run_at,
            task.next_run_at,
            task.created_at,
            task.updated_at,
        )
    return task


async def get_cron(cron_id: str) -> CronTask | None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow("SELECT * FROM agent_cron_tasks WHERE id = $1", cron_id)
    return _row_to_cron(row) if row else None


async def list_crons(org_id: str) -> list[CronTask]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM agent_cron_tasks WHERE org_id = $1 ORDER BY created_at",
            org_id,
        )
    return [_row_to_cron(r) for r in rows]


async def list_due_crons(now: datetime | None = None) -> list[CronTask]:
    """Return all enabled crons whose next_run_at <= now."""
    if now is None:
        now = _now()
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """SELECT * FROM agent_cron_tasks
               WHERE enabled = true AND next_run_at IS NOT NULL AND next_run_at <= $1
               ORDER BY next_run_at""",
            now,
        )
    return [_row_to_cron(r) for r in rows]


async def update_last_run(cron_id: str, last_run: datetime, next_run: datetime) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            "UPDATE agent_cron_tasks SET last_run_at = $1, next_run_at = $2, updated_at = $3 WHERE id = $4",
            last_run,
            next_run,
            _now(),
            cron_id,
        )


async def toggle_cron(cron_id: str, enabled: bool) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            "UPDATE agent_cron_tasks SET enabled = $1, updated_at = $2 WHERE id = $3",
            enabled,
            _now(),
            cron_id,
        )


async def delete_cron(cron_id: str) -> bool:
    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute("DELETE FROM agent_cron_tasks WHERE id = $1", cron_id)
    return result == "DELETE 1"
