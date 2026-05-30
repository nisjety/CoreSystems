"""Task executor — claim and execute agent tasks.

Mirrors the CC ``executeTask`` pattern:
  1. Claim the task atomically via ``claim_task``
  2. Dispatch to the correct handler based on ``TaskType``
  3. Mark the task completed or failed with output

For LOCAL_BASH tasks the executor runs the command in a subprocess with
a configurable timeout and captures stdout/stderr as output.

For GENERAL / SUB_AGENT tasks the caller is expected to use the agent
service; this module handles the low-level dispatch and recording.
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from app.tasks.domain import (
    ExecutedTask,
    TaskRecord,
    TaskStatus,
    TaskType,
    TaskUpdate,
)
from app.tasks.repository import claim_task, update_task

logger = logging.getLogger(__name__)

# Default timeout for LOCAL_BASH tasks (seconds)
DEFAULT_BASH_TIMEOUT = 60.0


class TaskExecutor:
    """Claim and run a single task.

    Usage::

        executor = TaskExecutor(pool)
        result = await executor.execute(task_id, agent_id="worker-abc")
    """

    def __init__(self, pool: Any, bash_timeout: float = DEFAULT_BASH_TIMEOUT) -> None:
        self._pool = pool
        self._bash_timeout = bash_timeout

    async def execute(
        self,
        task_id: str,
        agent_id: str,
    ) -> ExecutedTask:
        """Claim and execute a task.  Returns ExecutedTask with result.

        If claiming fails (task already taken, blocked, terminal), returns
        a failed ExecutedTask with a reason.
        """
        async with self._pool.acquire() as conn:
            claim = await claim_task(conn, task_id, agent_id)

        if not claim.success:
            return ExecutedTask(
                task_id=task_id,
                task_type=TaskType.GENERAL,
                success=False,
                error=f"claim_failed:{claim.reason}",
            )

        task = claim.task
        assert task is not None  # contract from claim_task

        task_type = _resolve_task_type(task)
        start = time.monotonic()
        result = await self._dispatch(task, task_type)
        result.duration_ms = int((time.monotonic() - start) * 1000)

        # Persist outcome
        final_status = TaskStatus.COMPLETED if result.success else TaskStatus.FAILED
        async with self._pool.acquire() as conn:
            await update_task(
                conn,
                task_id,
                TaskUpdate(
                    status=final_status,
                    output=result.output,
                    error=result.error,
                ),
            )

        logger.info(
            "task_executed",
            extra={
                "task_id": task_id,
                "task_type": task_type.value,
                "success": result.success,
                "duration_ms": result.duration_ms,
            },
        )
        return result

    # ----------------------------------------------------------------
    # Dispatcher
    # ----------------------------------------------------------------

    async def _dispatch(self, task: TaskRecord, task_type: TaskType) -> ExecutedTask:
        if task_type == TaskType.LOCAL_BASH:
            return await self._run_bash(task)
        # All other types are handled by the calling agent service
        # (the executor records the intent and returns immediately).
        return ExecutedTask(
            task_id=task.id,
            task_type=task_type,
            success=True,
            output=f"Task '{task.subject}' claimed and dispatched to agent service.",
        )

    # ----------------------------------------------------------------
    # LOCAL_BASH handler
    # ----------------------------------------------------------------

    async def _run_bash(self, task: TaskRecord) -> ExecutedTask:
        """Run a shell command extracted from the task metadata.

        Expected metadata format::

            {"command": "ls -la /tmp", "working_dir": "/opt/app"}
        """
        command = task.metadata.get("command", "")
        working_dir = task.metadata.get("working_dir") or None

        if not command:
            return ExecutedTask(
                task_id=task.id,
                task_type=TaskType.LOCAL_BASH,
                success=False,
                error="No 'command' key in task metadata for local_bash task.",
                exit_code=-1,
            )

        try:
            proc = await asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.STDOUT,
                cwd=working_dir,
            )
            try:
                stdout, _ = await asyncio.wait_for(
                    proc.communicate(), timeout=self._bash_timeout
                )
            except asyncio.TimeoutError:
                proc.kill()
                await proc.wait()
                return ExecutedTask(
                    task_id=task.id,
                    task_type=TaskType.LOCAL_BASH,
                    success=False,
                    error=f"Command timed out after {self._bash_timeout}s.",
                    exit_code=-1,
                )

            exit_code = proc.returncode or 0
            output = stdout.decode(errors="replace").strip()
            success = exit_code == 0
            return ExecutedTask(
                task_id=task.id,
                task_type=TaskType.LOCAL_BASH,
                success=success,
                output=output or None,
                error=None if success else f"Command exited with code {exit_code}",
                exit_code=exit_code,
            )

        except Exception as exc:
            logger.error(
                "task_bash_error",
                extra={"task_id": task.id, "error": str(exc)},
            )
            return ExecutedTask(
                task_id=task.id,
                task_type=TaskType.LOCAL_BASH,
                success=False,
                error=str(exc),
                exit_code=-1,
            )


# ────────────────────────────────────────────────────────────────────────────
# Helpers
# ────────────────────────────────────────────────────────────────────────────


def _resolve_task_type(task: TaskRecord) -> TaskType:
    """Determine the TaskType from the task's metadata or default to GENERAL."""
    raw = task.metadata.get("task_type", TaskType.GENERAL.value)
    try:
        return TaskType(raw)
    except ValueError:
        return TaskType.GENERAL
