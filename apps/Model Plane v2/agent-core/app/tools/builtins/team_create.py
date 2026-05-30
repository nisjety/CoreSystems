"""TeamCreateTool / TeamDeleteTool — LLM-invokable wrappers for WorkerManager."""

from __future__ import annotations

import logging
from typing import Any

from app.coordinator.workers import WorkerManager
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Singleton WorkerManager shared across tool calls within a coordinator session.
# Injected at session startup via set_worker_manager().
_manager: WorkerManager | None = None


def set_worker_manager(mgr: WorkerManager) -> None:
    global _manager
    _manager = mgr


def get_worker_manager() -> WorkerManager:
    global _manager
    if _manager is None:
        _manager = WorkerManager()
    return _manager


class TeamCreateTool:
    """Assign a new worker task to the coordinator's team."""

    name = "team_create"
    description = (
        "Create a worker task entry in the coordinator team. "
        "Use in coordinator mode to track parallel sub-work distributed to worker agents. "
        "Returns the worker ID for subsequent status checks."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "objective": {
                "type": "string",
                "description": "Clear description of what the worker should accomplish.",
            },
            "context": {
                "type": "string",
                "description": "Additional context or constraints for the worker.",
                "default": "",
            },
            "success_criteria": {
                "type": "string",
                "description": "How to determine the worker succeeded.",
                "default": "",
            },
        },
        "required": ["objective"],
    }
    search_hint = "team worker create coordinator spawn parallel"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Create a worker task in the coordinator team. "
            "Use with send_message to dispatch work to parallel sub-agents."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("objective", "").strip():
            raise ValueError("'objective' is required and must be non-empty")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        mgr = get_worker_manager()
        worker = mgr.create_task(
            objective=input_data["objective"],
            context=input_data.get("context", ""),
            success_criteria=input_data.get("success_criteria", ""),
        )
        logger.info("team_create_tool", extra={"worker_id": worker.worker_id})
        return ToolResult(
            output=f"Worker task created: {worker.worker_id}",
            metadata={
                "worker_id": worker.worker_id,
                "objective": worker.objective,
                "state": worker.state.value,
            },
        )


class TeamDeleteTool:
    """Mark a worker task as completed or failed in the coordinator team."""

    name = "team_delete"
    description = (
        "Mark a coordinator worker task as completed or failed. "
        "Call when a worker agent finishes or errors out to update team state."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "worker_id": {
                "type": "string",
                "description": "Worker ID returned by team_create.",
            },
            "outcome": {
                "type": "string",
                "enum": ["completed", "failed"],
                "description": "Final outcome of the worker task.",
                "default": "completed",
            },
            "summary": {
                "type": "string",
                "description": "Brief summary of what was accomplished or what failed.",
                "default": "",
            },
        },
        "required": ["worker_id"],
    }
    search_hint = "team worker done complete fail coordinator"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Mark a coordinator worker task as completed or failed."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("worker_id"):
            raise ValueError("'worker_id' is required")
        outcome = input_data.get("outcome", "completed")
        if outcome not in {"completed", "failed"}:
            raise ValueError(f"Invalid outcome '{outcome}'. Must be 'completed' or 'failed'")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        mgr = get_worker_manager()
        worker_id = input_data["worker_id"]
        outcome = input_data.get("outcome", "completed")
        summary = input_data.get("summary", "")

        try:
            if outcome == "completed":
                updated = mgr.complete(worker_id, summary=summary)
            else:
                updated = mgr.fail(worker_id, error=summary or "Worker failed")

            logger.info(
                "team_delete_tool",
                extra={"worker_id": worker_id, "outcome": outcome},
            )
            return ToolResult(
                output=f"Worker {worker_id} marked {outcome}.",
                metadata={"worker_id": worker_id, "state": updated.state.value},
            )
        except KeyError:
            return ToolResult(error=f"Worker not found: {worker_id}")
        except Exception as exc:
            logger.exception("team_delete_tool_failed", extra={"worker_id": worker_id})
            return ToolResult(error=f"Failed to update worker: {exc}")
