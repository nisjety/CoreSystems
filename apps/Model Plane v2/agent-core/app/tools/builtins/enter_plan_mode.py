"""EnterPlanModeTool / ExitPlanModeTool — LLM-invokable wrappers for plan_mode.py."""

from __future__ import annotations

import logging
from typing import Any, TYPE_CHECKING

from app.tools.base import ToolResult

if TYPE_CHECKING:
    from app.nats_publisher import EventPublisher

logger = logging.getLogger(__name__)

# Injected at session startup.
_publisher: "EventPublisher | None" = None


def set_publisher(pub: "EventPublisher") -> None:
    global _publisher
    _publisher = pub


class EnterPlanModeTool:
    """Switch the current run into PLAN mode to draft and review before execution."""

    name = "enter_plan_mode"
    description = (
        "Enter plan mode: actions will be collected and presented for approval "
        "before any execution occurs. Use when the task is complex and the user "
        "should review the steps before they run."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "run_id": {
                "type": "string",
                "description": "Run ID to switch to plan mode.",
            },
            "session_id": {
                "type": "string",
                "description": "Session ID of the run.",
            },
            "rationale": {
                "type": "string",
                "description": "Why plan mode is being entered.",
                "default": "",
            },
        },
        "required": ["run_id", "session_id"],
    }
    search_hint = "plan mode approve draft review steps"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Enter plan mode to draft a list of steps for user review "
            "before any action is executed."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("run_id"):
            raise ValueError("'run_id' is required")
        if not input_data.get("session_id"):
            raise ValueError("'session_id' is required")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        from app.domain import AgentEvent

        pub = _publisher
        run_id = input_data["run_id"]
        session_id = input_data["session_id"]

        try:
            if pub is not None:
                await pub.publish(
                    AgentEvent(
                        event_type="run.plan_mode.entered",
                        run_id=run_id,
                        session_id=session_id,
                        payload={"rationale": input_data.get("rationale", "")},
                    )
                )
            logger.info("enter_plan_mode_tool", extra={"run_id": run_id})
            return ToolResult(
                output=f"Run {run_id} entered plan mode. Actions will be collected for approval.",
                metadata={"run_id": run_id, "mode": "plan"},
            )
        except Exception as exc:
            logger.exception("enter_plan_mode_failed")
            return ToolResult(error=f"Failed to enter plan mode: {exc}")


class ExitPlanModeTool:
    """Exit plan mode and resume normal execution."""

    name = "exit_plan_mode"
    description = (
        "Exit plan mode and resume executing actions directly. "
        "Call after the user has approved the plan."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "run_id": {
                "type": "string",
                "description": "Run ID to exit plan mode for.",
            },
            "session_id": {
                "type": "string",
                "description": "Session ID of the run.",
            },
            "approved": {
                "type": "boolean",
                "description": "Whether the plan was approved (true) or rejected (false).",
                "default": True,
            },
        },
        "required": ["run_id", "session_id"],
    }
    search_hint = "plan mode exit resume execute approve"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Exit plan mode and resume normal execution after plan approval."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("run_id"):
            raise ValueError("'run_id' is required")
        if not input_data.get("session_id"):
            raise ValueError("'session_id' is required")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        from app.domain import AgentEvent

        pub = _publisher
        run_id = input_data["run_id"]
        session_id = input_data["session_id"]
        approved = input_data.get("approved", True)

        try:
            if pub is not None:
                await pub.publish(
                    AgentEvent(
                        event_type="run.plan_mode.exited",
                        run_id=run_id,
                        session_id=session_id,
                        payload={"approved": approved},
                    )
                )
            outcome = "approved" if approved else "rejected"
            logger.info("exit_plan_mode_tool", extra={"run_id": run_id, "approved": approved})
            return ToolResult(
                output=f"Run {run_id} exited plan mode ({outcome}).",
                metadata={"run_id": run_id, "mode": "execute", "approved": approved},
            )
        except Exception as exc:
            logger.exception("exit_plan_mode_failed")
            return ToolResult(error=f"Failed to exit plan mode: {exc}")
