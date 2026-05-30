"""TaskCreateTool / TaskUpdateTool — LLM-invokable wrappers for the task subsystem."""

from __future__ import annotations

import logging
from typing import Any

from app.tasks.domain import TaskRecord, TaskStatus, TaskUpdate
from app.tasks import repository as task_repo
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)


class TaskCreateTool:
    """Create a persistent task in the agent task system."""

    name = "task_create"
    description = (
        "Create a tracked task in the agent task system. "
        "Use this to decompose a goal into discrete, observable units of work. "
        "Returns the new task ID and initial status."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "subject": {
                "type": "string",
                "description": "One-line task title.",
            },
            "description": {
                "type": "string",
                "description": "Detailed description of the work to perform.",
                "default": "",
            },
            "run_id": {
                "type": "string",
                "description": "Run ID this task belongs to.",
            },
            "session_id": {
                "type": "string",
                "description": "Session ID this task belongs to.",
            },
            "org_id": {
                "type": "string",
                "description": "Optional org ID.",
            },
            "blocked_by": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Task IDs that must complete before this task starts.",
                "default": [],
            },
        },
        "required": ["subject", "run_id", "session_id"],
    }
    search_hint = "task create todo work item track"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Create a persistent task to track a unit of work. "
            "Use to decompose complex goals into verifiable steps."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        subject = input_data.get("subject")
        if not subject or not isinstance(subject, str) or not subject.strip():
            raise ValueError("'subject' is required and must be a non-empty string")
        if not input_data.get("run_id"):
            raise ValueError("'run_id' is required")
        if not input_data.get("session_id"):
            raise ValueError("'session_id' is required")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        task = TaskRecord(
            run_id=input_data["run_id"],
            session_id=input_data["session_id"],
            org_id=input_data.get("org_id"),
            subject=input_data["subject"],
            description=input_data.get("description", ""),
            blocked_by=input_data.get("blocked_by", []),
        )
        try:
            created = await task_repo.create_task(task)
            logger.info("task_created", extra={"task_id": created.id, "subject": created.subject})
            return ToolResult(
                output=f"Task created: {created.id}",
                metadata={
                    "task_id": created.id,
                    "subject": created.subject,
                    "status": created.status.value,
                },
            )
        except Exception as exc:
            logger.exception("task_create_failed")
            return ToolResult(error=f"Failed to create task: {exc}")


class TaskUpdateTool:
    """Update an existing task's status, output, or metadata."""

    name = "task_update"
    description = (
        "Update a task's status, output, or description. "
        "Use to mark tasks in_progress, completed, or failed, "
        "and to attach output or error details."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "task_id": {
                "type": "string",
                "description": "ID of the task to update.",
            },
            "status": {
                "type": "string",
                "enum": ["pending", "in_progress", "completed", "failed", "killed"],
                "description": "New status for the task.",
            },
            "output": {
                "type": "string",
                "description": "Output or result to attach to the task.",
            },
            "error": {
                "type": "string",
                "description": "Error message if the task failed.",
            },
            "description": {
                "type": "string",
                "description": "Updated task description.",
            },
        },
        "required": ["task_id"],
    }
    search_hint = "task update status done complete fail"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Update task status and attach output. "
            "Mark tasks completed or failed once their work is done."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("task_id"):
            raise ValueError("'task_id' is required")
        status_raw = input_data.get("status")
        if status_raw and status_raw not in {s.value for s in TaskStatus}:
            raise ValueError(f"Invalid status: {status_raw}")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        task_id = input_data["task_id"]
        update = TaskUpdate(
            status=TaskStatus(input_data["status"]) if input_data.get("status") else None,
            output=input_data.get("output"),
            error=input_data.get("error"),
            description=input_data.get("description"),
        )
        try:
            updated = await task_repo.update_task(task_id, update)
            if updated is None:
                return ToolResult(error=f"Task not found: {task_id}")
            logger.info("task_updated", extra={"task_id": task_id, "status": updated.status.value})
            return ToolResult(
                output=f"Task {task_id} updated → {updated.status.value}",
                metadata={"task_id": task_id, "status": updated.status.value},
            )
        except Exception as exc:
            logger.exception("task_update_failed", extra={"task_id": task_id})
            return ToolResult(error=f"Failed to update task: {exc}")
