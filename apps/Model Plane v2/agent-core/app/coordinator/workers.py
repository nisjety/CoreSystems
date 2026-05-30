"""Worker manager — spawn, track, and collect results from worker agents."""

from __future__ import annotations

import enum
import logging
import uuid
from datetime import datetime, timezone
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class WorkerState(str, enum.Enum):
    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"


class WorkerTask(BaseModel):
    """A task dispatched to a worker agent."""

    worker_id: str = Field(default_factory=lambda: uuid.uuid4().hex[:12])
    objective: str
    context: str = ""
    success_criteria: str = ""
    state: WorkerState = WorkerState.PENDING
    created_at: datetime = Field(
        default_factory=lambda: datetime.now(timezone.utc)
    )
    completed_at: datetime | None = None
    result_summary: str = ""
    artifacts: list[str] = Field(default_factory=list)
    error: str | None = None
    retry_count: int = 0


class WorkerManager:
    """Track worker lifecycle in coordinator mode.

    Immutable updates — each mutation returns a new WorkerTask
    and replaces the internal dict entry.
    """

    MAX_RETRIES: int = 1

    def __init__(self) -> None:
        self._workers: dict[str, WorkerTask] = {}

    # ── Create ──────────────────────────────────────────────

    def create_task(
        self,
        objective: str,
        *,
        context: str = "",
        success_criteria: str = "",
    ) -> WorkerTask:
        task = WorkerTask(
            objective=objective,
            context=context,
            success_criteria=success_criteria,
        )
        self._workers = {**self._workers, task.worker_id: task}
        logger.info("Created worker task %s: %s", task.worker_id, objective)
        return task

    # ── State transitions ───────────────────────────────────

    def start(self, worker_id: str) -> WorkerTask:
        return self._transition(worker_id, WorkerState.RUNNING)

    def complete(
        self,
        worker_id: str,
        summary: str = "",
        artifacts: list[str] | None = None,
    ) -> WorkerTask:
        task = self._get_or_raise(worker_id)
        updated = task.model_copy(
            update={
                "state": WorkerState.COMPLETED,
                "completed_at": datetime.now(timezone.utc),
                "result_summary": summary,
                "artifacts": artifacts or [],
            }
        )
        self._workers = {**self._workers, worker_id: updated}
        return updated

    def fail(self, worker_id: str, error: str) -> WorkerTask:
        task = self._get_or_raise(worker_id)
        updated = task.model_copy(
            update={
                "state": WorkerState.FAILED,
                "completed_at": datetime.now(timezone.utc),
                "error": error,
            }
        )
        self._workers = {**self._workers, worker_id: updated}
        return updated

    def retry(self, worker_id: str) -> WorkerTask | None:
        """Reset a failed task for retry. Returns None if max retries exceeded."""
        task = self._get_or_raise(worker_id)
        if task.retry_count >= self.MAX_RETRIES:
            return None
        updated = task.model_copy(
            update={
                "state": WorkerState.PENDING,
                "retry_count": task.retry_count + 1,
                "error": None,
                "completed_at": None,
            }
        )
        self._workers = {**self._workers, worker_id: updated}
        return updated

    # ── Queries ─────────────────────────────────────────────

    def get(self, worker_id: str) -> WorkerTask | None:
        return self._workers.get(worker_id)

    def list_all(self, state: WorkerState | None = None) -> list[WorkerTask]:
        tasks = list(self._workers.values())
        if state is not None:
            tasks = [t for t in tasks if t.state == state]
        return tasks

    @property
    def pending_count(self) -> int:
        return sum(
            1 for t in self._workers.values()
            if t.state == WorkerState.PENDING
        )

    @property
    def completed_count(self) -> int:
        return sum(
            1 for t in self._workers.values()
            if t.state == WorkerState.COMPLETED
        )

    @property
    def failed_count(self) -> int:
        return sum(
            1 for t in self._workers.values()
            if t.state == WorkerState.FAILED
        )

    @property
    def all_done(self) -> bool:
        return all(
            t.state in (WorkerState.COMPLETED, WorkerState.FAILED)
            for t in self._workers.values()
        ) and len(self._workers) > 0

    def summary_xml(self) -> str:
        """Generate an XML summary of all worker tasks for the coordinator prompt."""
        parts: list[str] = ["<worker_summary>"]
        for t in self._workers.values():
            parts.append(f"<worker id='{t.worker_id}' state='{t.state.value}'>")
            parts.append(f"  <objective>{t.objective}</objective>")
            if t.result_summary:
                parts.append(f"  <result>{t.result_summary}</result>")
            if t.error:
                parts.append(f"  <error>{t.error}</error>")
            parts.append("</worker>")
        parts.append("</worker_summary>")
        return "\n".join(parts)

    # ── Internals ───────────────────────────────────────────

    def _get_or_raise(self, worker_id: str) -> WorkerTask:
        task = self._workers.get(worker_id)
        if task is None:
            raise KeyError(f"Unknown worker: {worker_id}")
        return task

    def _transition(self, worker_id: str, state: WorkerState) -> WorkerTask:
        task = self._get_or_raise(worker_id)
        updated = task.model_copy(update={"state": state})
        self._workers = {**self._workers, worker_id: updated}
        return updated
