"""CronCreateTool — LLM-invokable wrapper for scheduling recurring cron tasks."""

from __future__ import annotations

import logging
from typing import Any

from app.cron.domain import CronTask
from app.cron import repository as cron_repo
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)


class CronCreateTool:
    """Schedule a recurring task using a cron expression."""

    name = "cron_create"
    description = (
        "Schedule a recurring task using a cron expression. "
        "The task will run on the defined schedule and invoke the agent with the given goal. "
        "Returns the cron task ID."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "description": "Human-readable name for this cron schedule.",
            },
            "cron_expr": {
                "type": "string",
                "description": "Standard cron expression (e.g. '0 9 * * 1' = every Monday at 9am).",
            },
            "goal": {
                "type": "string",
                "description": "What the agent should do when this cron fires.",
            },
            "org_id": {
                "type": "string",
                "description": "Organisation this cron belongs to.",
            },
            "session_id": {
                "type": "string",
                "description": "Session ID for context.",
            },
            "policy": {
                "type": "object",
                "description": "Optional scheduling policy (e.g. timeout, retries).",
                "default": {},
            },
        },
        "required": ["name", "cron_expr", "goal", "org_id", "session_id"],
    }
    search_hint = "cron schedule recurring timer repeat task"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Schedule a recurring agent task using a cron expression. "
            "Use to automate periodic work (reports, checks, reminders)."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        for field in ("name", "cron_expr", "goal", "org_id", "session_id"):
            if not input_data.get(field, "").strip():
                raise ValueError(f"'{field}' is required and must be non-empty")
        # Basic cron expression sanity: 5 or 6 space-separated fields
        cron_expr = input_data["cron_expr"].strip()
        parts = cron_expr.split()
        if len(parts) not in (5, 6):
            raise ValueError(
                f"Invalid cron expression '{cron_expr}': expected 5 or 6 fields, got {len(parts)}"
            )
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        cron = CronTask(
            org_id=input_data["org_id"],
            session_id=input_data["session_id"],
            name=input_data["name"],
            cron_expr=input_data["cron_expr"].strip(),
            goal=input_data["goal"],
            policy=input_data.get("policy", {}),
        )
        try:
            created = await cron_repo.create_cron(cron)
            logger.info(
                "cron_create_tool",
                extra={"cron_id": created.id, "expr": created.cron_expr},
            )
            return ToolResult(
                output=f"Cron scheduled: {created.id} ({created.cron_expr})",
                metadata={
                    "cron_id": created.id,
                    "name": created.name,
                    "cron_expr": created.cron_expr,
                    "enabled": created.enabled,
                },
            )
        except Exception as exc:
            logger.exception("cron_create_tool_failed")
            return ToolResult(error=f"Failed to create cron: {exc}")
