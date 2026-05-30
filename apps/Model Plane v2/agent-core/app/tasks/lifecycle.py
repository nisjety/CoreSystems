"""Task lifecycle manager — spawn, register, update, notify, evict, kill.

Matches CC's full task lifecycle with in-memory state tracking
and notification dispatch. Does NOT require a DB connection —
all state lives in memory (the executor handles persistence).
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from typing import Any

from app.tasks.domain import (
    LocalAgentTaskState,
    RemoteAgentTaskState,
    TaskNotification,
    TaskProgress,
    TaskRecord,
    TaskStatus,
    TaskType,
    is_terminal,
)

logger = logging.getLogger(__name__)

MAX_TASKS = 20
MAX_RECENT_ACTIVITIES = 50


class TaskLimitError(Exception):
    """Raised when the maximum number of active tasks is exceeded."""

    def __init__(self, limit: int) -> None:
        self.limit = limit
        super().__init__(f"Task limit reached: {limit}")


class TaskManager:
    """In-memory lifecycle manager for agent tasks.

    Tracks local and remote agent states, dispatches notifications,
    and enforces concurrency limits.
    """

    def __init__(self, max_tasks: int = MAX_TASKS) -> None:
        self._max_tasks = max_tasks
        self._tasks: dict[str, TaskRecord] = {}
        self._local_states: dict[str, LocalAgentTaskState] = {}
        self._remote_states: dict[str, RemoteAgentTaskState] = {}
        self._notifications: list[TaskNotification] = []

    # ------------------------------------------------------------------
    # Spawn
    # ------------------------------------------------------------------

    def spawn(
        self,
        run_id: str,
        session_id: str,
        subject: str,
        task_type: TaskType = TaskType.GENERAL,
        org_id: str | None = None,
        description: str = "",
        metadata: dict[str, Any] | None = None,
    ) -> TaskRecord:
        """Create and register a new task. Raises TaskLimitError if at capacity."""
        active = sum(1 for t in self._tasks.values() if not is_terminal(t.status))
        if active >= self._max_tasks:
            raise TaskLimitError(self._max_tasks)

        task = TaskRecord(
            run_id=run_id,
            session_id=session_id,
            org_id=org_id,
            subject=subject,
            description=description,
            metadata=metadata or {},
        )
        self._tasks[task.id] = task
        self._emit(task.id, "spawned", f"Task created: {subject}")
        logger.info("task_spawned", extra={"task_id": task.id, "type": task_type.value})
        return task

    # ------------------------------------------------------------------
    # Register agent state
    # ------------------------------------------------------------------

    def register_local(self, task_id: str, agent_id: str) -> LocalAgentTaskState:
        """Register a local agent execution for a task."""
        state = LocalAgentTaskState(agent_id=agent_id, task_id=task_id)
        self._local_states[task_id] = state
        return state

    def register_remote(self, task_id: str, session_id: str) -> RemoteAgentTaskState:
        """Register a remote agent session for a task."""
        state = RemoteAgentTaskState(session_id=session_id, task_id=task_id)
        self._remote_states[task_id] = state
        return state

    # ------------------------------------------------------------------
    # Update
    # ------------------------------------------------------------------

    def update_status(
        self,
        task_id: str,
        status: TaskStatus,
        output: str | None = None,
        error: str | None = None,
    ) -> TaskRecord | None:
        """Update a task's status. Returns None if task not found."""
        task = self._tasks.get(task_id)
        if task is None:
            return None

        task = task.model_copy(
            update={
                "status": status,
                "output": output if output is not None else task.output,
                "error": error if error is not None else task.error,
                "updated_at": datetime.now(timezone.utc),
            }
        )
        self._tasks[task_id] = task

        if is_terminal(status):
            event = "completed" if status == TaskStatus.COMPLETED else status.value
            self._emit(task_id, event, f"Task {status.value}: {task.subject}")

        return task

    def update_progress(
        self,
        task_id: str,
        tool_use_delta: int = 0,
        token_delta: int = 0,
        activity: str | None = None,
    ) -> None:
        """Update progress for a local agent task."""
        state = self._local_states.get(task_id)
        if state is None:
            return

        progress = state.progress.model_copy(
            update={
                "tool_use_count": state.progress.tool_use_count + tool_use_delta,
                "token_count": state.progress.token_count + token_delta,
            }
        )
        if activity:
            recent = state.progress.recent_activities + [activity]
            progress = progress.model_copy(
                update={"recent_activities": recent[-MAX_RECENT_ACTIVITIES:]}
            )

        state = state.model_copy(update={"progress": progress})
        self._local_states[task_id] = state

    # ------------------------------------------------------------------
    # Query
    # ------------------------------------------------------------------

    def get(self, task_id: str) -> TaskRecord | None:
        return self._tasks.get(task_id)

    def list_active(self) -> list[TaskRecord]:
        return [t for t in self._tasks.values() if not is_terminal(t.status)]

    def get_local_state(self, task_id: str) -> LocalAgentTaskState | None:
        return self._local_states.get(task_id)

    def get_remote_state(self, task_id: str) -> RemoteAgentTaskState | None:
        return self._remote_states.get(task_id)

    # ------------------------------------------------------------------
    # Notifications
    # ------------------------------------------------------------------

    def drain_notifications(self) -> list[TaskNotification]:
        """Return and clear all pending notifications."""
        notifications = list(self._notifications)
        self._notifications.clear()
        return notifications

    # ------------------------------------------------------------------
    # Kill / Evict
    # ------------------------------------------------------------------

    def kill(self, task_id: str, reason: str = "") -> bool:
        """Kill a running task. Returns False if already terminal."""
        task = self._tasks.get(task_id)
        if task is None or is_terminal(task.status):
            return False

        self.update_status(task_id, TaskStatus.KILLED, error=reason)
        self._local_states.pop(task_id, None)
        self._remote_states.pop(task_id, None)
        self._emit(task_id, "killed", reason or "Task killed")
        return True

    def evict(self, task_id: str) -> bool:
        """Remove a terminal task from memory. Returns False if still active."""
        task = self._tasks.get(task_id)
        if task is None:
            return False
        if not is_terminal(task.status):
            return False

        self._tasks.pop(task_id, None)
        self._local_states.pop(task_id, None)
        self._remote_states.pop(task_id, None)
        return True

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _emit(self, task_id: str, event: str, summary: str) -> None:
        self._notifications.append(
            TaskNotification(task_id=task_id, event=event, summary=summary)
        )
