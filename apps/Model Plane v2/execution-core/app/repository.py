"""Repository layer for runner_inventory and runner_tasks tables."""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

from app.database import get_pool
from app.domain import (
    ArtifactMetadata,
    RunnerRecord,
    RunnerStatus,
    TaskRecord,
)

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Runner inventory
# ---------------------------------------------------------------------------


async def upsert_runner(record: RunnerRecord) -> None:
    """Insert or update a runner in the inventory."""
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO runner_inventory (
            runner_id, capabilities, max_concurrent, labels,
            status, current_task_id, workspace_id, last_heartbeat, registered_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (runner_id) DO UPDATE SET
            capabilities    = EXCLUDED.capabilities,
            max_concurrent  = EXCLUDED.max_concurrent,
            labels          = EXCLUDED.labels,
            status          = EXCLUDED.status,
            current_task_id = EXCLUDED.current_task_id,
            workspace_id    = EXCLUDED.workspace_id,
            last_heartbeat  = EXCLUDED.last_heartbeat
        """,
        record.runner_id,
        json.dumps(record.capabilities),
        record.max_concurrent,
        json.dumps(record.labels),
        record.status.value,
        record.current_task_id,
        record.workspace_id,
        record.last_heartbeat,
        record.registered_at,
    )


async def get_runner(runner_id: str) -> RunnerRecord | None:
    """Fetch a single runner by id."""
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM runner_inventory WHERE runner_id = $1", runner_id
    )
    if row is None:
        return None
    return _row_to_runner(row)


async def list_runners(
    status: RunnerStatus | None = None,
    limit: int = 100,
) -> list[RunnerRecord]:
    """List runners, optionally filtered by status."""
    pool = await get_pool()
    if status is not None:
        rows = await pool.fetch(
            "SELECT * FROM runner_inventory WHERE status = $1 ORDER BY last_heartbeat DESC LIMIT $2",
            status.value,
            limit,
        )
    else:
        rows = await pool.fetch(
            "SELECT * FROM runner_inventory ORDER BY last_heartbeat DESC LIMIT $1",
            limit,
        )
    return [_row_to_runner(r) for r in rows]


async def update_runner_heartbeat(
    runner_id: str,
    status: RunnerStatus,
    current_task_id: str | None = None,
) -> bool:
    """Update heartbeat timestamp and status. Returns True if runner exists."""
    pool = await get_pool()
    result = await pool.execute(
        """
        UPDATE runner_inventory
        SET last_heartbeat = now(), status = $2, current_task_id = $3
        WHERE runner_id = $1
        """,
        runner_id,
        status.value,
        current_task_id,
    )
    return result == "UPDATE 1"


async def mark_runners_dead(stale_seconds: int = 600) -> int:
    """Mark runners as DEAD if their last heartbeat is older than stale_seconds."""
    pool = await get_pool()
    result = await pool.execute(
        """
        UPDATE runner_inventory
        SET status = 'dead'
        WHERE status NOT IN ('dead', 'cancelled')
          AND last_heartbeat < now() - ($1 || ' seconds')::interval
        """,
        str(stale_seconds),
    )
    # result is like "UPDATE N"
    count = int(result.split()[-1]) if result else 0
    if count > 0:
        logger.warning("runners_marked_dead", extra={"count": count})
    return count


async def delete_runner(runner_id: str) -> bool:
    pool = await get_pool()
    result = await pool.execute(
        "DELETE FROM runner_inventory WHERE runner_id = $1", runner_id
    )
    return result == "DELETE 1"


# ---------------------------------------------------------------------------
# Tasks
# ---------------------------------------------------------------------------


async def insert_task(record: TaskRecord) -> None:
    """Insert a new task."""
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO runner_tasks (
            task_id, run_id, session_id, workspace_id, runner_id,
            tool_name, tool_input, status, priority, timeout_seconds,
            output, error, duration_ms, created_at, started_at, completed_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        """,
        record.task_id,
        record.run_id,
        record.session_id,
        record.workspace_id,
        record.runner_id,
        record.tool_name,
        json.dumps(record.tool_input),
        record.status.value,
        record.priority,
        record.timeout_seconds,
        json.dumps(record.output) if record.output else None,
        record.error,
        record.duration_ms,
        record.created_at,
        record.started_at,
        record.completed_at,
    )


async def get_task(task_id: str) -> TaskRecord | None:
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM runner_tasks WHERE task_id = $1", task_id
    )
    if row is None:
        return None
    return _row_to_task(row)


async def list_tasks(
    run_id: str | None = None,
    status: RunnerStatus | None = None,
    limit: int = 100,
) -> list[TaskRecord]:
    """List tasks with optional filters."""
    pool = await get_pool()
    conditions: list[str] = []
    params: list[Any] = []
    idx = 1

    if run_id is not None:
        conditions.append(f"run_id = ${idx}")
        params.append(run_id)
        idx += 1
    if status is not None:
        conditions.append(f"status = ${idx}")
        params.append(status.value)
        idx += 1

    where = f"WHERE {' AND '.join(conditions)}" if conditions else ""
    params.append(limit)

    rows = await pool.fetch(
        f"SELECT * FROM runner_tasks {where} ORDER BY created_at DESC LIMIT ${idx}",
        *params,
    )
    return [_row_to_task(r) for r in rows]


async def update_task_claimed(task_id: str, runner_id: str) -> bool:
    """Mark a task as claimed by a runner."""
    pool = await get_pool()
    result = await pool.execute(
        """
        UPDATE runner_tasks
        SET runner_id = $2, status = 'claimed', started_at = now()
        WHERE task_id = $1 AND status = 'idle'
        """,
        task_id,
        runner_id,
    )
    return result == "UPDATE 1"


async def update_task_completed(
    task_id: str,
    success: bool,
    output: dict[str, Any] | None = None,
    error: str | None = None,
    duration_ms: int = 0,
) -> bool:
    """Mark a task as completed (or failed)."""
    pool = await get_pool()
    new_status = "running" if False else ("idle" if not success else "idle")
    # Completed tasks effectively go back to idle from a runner perspective;
    # the task itself has its own status.
    result = await pool.execute(
        """
        UPDATE runner_tasks
        SET status = $2, output = $3, error = $4, duration_ms = $5, completed_at = now()
        WHERE task_id = $1
        """,
        task_id,
        "completing",
        json.dumps(output) if output else None,
        error,
        duration_ms,
    )
    return result == "UPDATE 1"


async def update_task_cancelled(task_id: str, reason: str) -> bool:
    pool = await get_pool()
    result = await pool.execute(
        """
        UPDATE runner_tasks SET status = 'cancelled', error = $2, completed_at = now()
        WHERE task_id = $1 AND status NOT IN ('completing', 'cancelled')
        """,
        task_id,
        reason,
    )
    return result == "UPDATE 1"


# ---------------------------------------------------------------------------
# Artifacts
# ---------------------------------------------------------------------------


async def insert_artifact(meta: ArtifactMetadata) -> None:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO runner_artifacts (
            artifact_id, task_id, workspace_id, kind,
            filename, size_bytes, content_type, storage_key, created_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
        """,
        meta.artifact_id,
        meta.task_id,
        meta.workspace_id,
        meta.kind.value,
        meta.filename,
        meta.size_bytes,
        meta.content_type,
        meta.storage_key,
        meta.created_at,
    )


async def list_artifacts(task_id: str) -> list[ArtifactMetadata]:
    pool = await get_pool()
    rows = await pool.fetch(
        "SELECT * FROM runner_artifacts WHERE task_id = $1 ORDER BY created_at",
        task_id,
    )
    return [
        ArtifactMetadata(
            artifact_id=r["artifact_id"],
            task_id=r["task_id"],
            workspace_id=r["workspace_id"],
            kind=r["kind"],
            filename=r["filename"],
            size_bytes=r["size_bytes"],
            content_type=r["content_type"],
            storage_key=r["storage_key"],
            created_at=r["created_at"],
        )
        for r in rows
    ]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _row_to_runner(row) -> RunnerRecord:
    caps = row["capabilities"]
    if isinstance(caps, str):
        caps = json.loads(caps)
    labels = row["labels"]
    if isinstance(labels, str):
        labels = json.loads(labels)
    return RunnerRecord(
        runner_id=row["runner_id"],
        capabilities=caps,
        max_concurrent=row["max_concurrent"],
        labels=labels,
        status=row["status"],
        current_task_id=row.get("current_task_id"),
        workspace_id=row.get("workspace_id"),
        last_heartbeat=row["last_heartbeat"],
        registered_at=row["registered_at"],
    )


def _row_to_task(row) -> TaskRecord:
    tool_input = row["tool_input"]
    if isinstance(tool_input, str):
        tool_input = json.loads(tool_input)
    output = row.get("output")
    if isinstance(output, str):
        output = json.loads(output)
    return TaskRecord(
        task_id=row["task_id"],
        run_id=row["run_id"],
        session_id=row["session_id"],
        workspace_id=row["workspace_id"],
        runner_id=row.get("runner_id"),
        tool_name=row["tool_name"],
        tool_input=tool_input,
        status=row["status"],
        priority=row["priority"],
        timeout_seconds=row["timeout_seconds"],
        output=output,
        error=row.get("error"),
        duration_ms=row.get("duration_ms", 0),
        created_at=row["created_at"],
        started_at=row.get("started_at"),
        completed_at=row.get("completed_at"),
    )
