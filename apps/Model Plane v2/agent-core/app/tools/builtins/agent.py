"""AgentTool — spawn a sub-agent run via the task system."""

from __future__ import annotations

import logging
from typing import Any

from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Context injected at session startup so the tool knows which run/session
# it belongs to.  Set via configure_agent_tool().
_RUN_ID: str = "unknown"
_SESSION_ID: str = "unknown"
_ORG_ID: str | None = None


def configure_agent_tool(run_id: str, session_id: str, org_id: str | None = None) -> None:
    """Inject session context so AgentTool can create tasks correctly."""
    global _RUN_ID, _SESSION_ID, _ORG_ID
    _RUN_ID = run_id
    _SESSION_ID = session_id
    _ORG_ID = org_id


class AgentTool:
    """Spawn a sub-agent to handle a complex, multi-step subtask autonomously.

    Creates a TaskRecord in PENDING state via the task repository.
    The task executor picks it up and runs it, returning the task ID
    so the caller can poll for completion.
    """

    name = "agent"
    description = (
        "Launch a sub-agent to handle a complex, multi-step subtask autonomously and return the result. "
        "Creates a tracked task and returns the task ID immediately. "
        "Use task_update to check status, or poll until the task is completed."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "prompt": {
                "type": "string",
                "description": "Detailed task description for the sub-agent.",
            },
            "agent_name": {
                "type": "string",
                "description": "Optional named agent to invoke (defaults to 'default').",
                "default": "default",
            },
            "run_id": {
                "type": "string",
                "description": "Parent run ID (auto-filled from session context if omitted).",
            },
            "session_id": {
                "type": "string",
                "description": "Session ID (auto-filled from session context if omitted).",
            },
        },
        "required": ["prompt"],
    }
    search_hint = "agent subagent spawn delegate task"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Launch a sub-agent to handle complex subtasks autonomously. "
            "Returns a task ID — poll via task_update to track progress."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        prompt_text = input_data.get("prompt")
        if not prompt_text or not isinstance(prompt_text, str) or not prompt_text.strip():
            raise ValueError("'prompt' is required and must be a non-empty string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        from app.tasks.domain import TaskRecord
        from app.tasks import repository as task_repo

        prompt_text = input_data["prompt"]
        agent_name = input_data.get("agent_name", "default")
        run_id = input_data.get("run_id") or _RUN_ID
        session_id = input_data.get("session_id") or _SESSION_ID
        org_id = _ORG_ID

        task = TaskRecord(
            run_id=run_id,
            session_id=session_id,
            org_id=org_id,
            subject=f"Sub-agent: {prompt_text[:80]}",
            description=prompt_text,
            metadata={"agent_name": agent_name, "spawned_by": "agent_tool"},
        )

        try:
            created = await task_repo.create_task(task)
            logger.info(
                "agent_tool_task_created",
                extra={
                    "task_id": created.id,
                    "agent_name": agent_name,
                    "prompt_len": len(prompt_text),
                },
            )
            return ToolResult(
                output=(
                    f"Sub-agent task created: {created.id}\n"
                    f"Agent: {agent_name}\n"
                    f"Status: {created.status.value}\n"
                    "Use task_update to poll for completion."
                ),
                metadata={
                    "task_id": created.id,
                    "agent_name": agent_name,
                    "status": created.status.value,
                },
            )
        except Exception as exc:
            logger.exception(
                "agent_tool_task_failed",
                extra={"agent_name": agent_name},
            )
            return ToolResult(error=f"Failed to create sub-agent task: {exc}")
