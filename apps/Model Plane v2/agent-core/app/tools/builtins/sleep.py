"""SleepTool — pause execution for a given number of seconds."""

from __future__ import annotations

import asyncio
import logging
from typing import Any

from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

MAX_SLEEP_SECONDS = 300  # 5 minutes hard cap


class SleepTool:
    """Pause the agent loop for N seconds.

    Useful in proactive / cron loops to rate-limit polling, wait for
    external state changes, or implement retry back-off.
    """

    name = "sleep"
    description = (
        "Pause execution for a specified number of seconds (max 300). "
        "Use in polling loops, retry back-off, or rate-limiting scenarios."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "seconds": {
                "type": "number",
                "description": "Number of seconds to sleep (0.1–300).",
            },
            "reason": {
                "type": "string",
                "description": "Optional human-readable reason for sleeping.",
                "default": "",
            },
        },
        "required": ["seconds"],
    }
    search_hint = "sleep pause wait delay rate-limit"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Pause execution for N seconds. Use for polling back-off and rate limiting."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        seconds = input_data.get("seconds")
        if seconds is None:
            raise ValueError("'seconds' is required")
        try:
            seconds = float(seconds)
        except (TypeError, ValueError):
            raise ValueError(f"'seconds' must be a number, got: {seconds!r}")
        if seconds < 0.1:
            raise ValueError("'seconds' must be at least 0.1")
        if seconds > MAX_SLEEP_SECONDS:
            raise ValueError(f"'seconds' must not exceed {MAX_SLEEP_SECONDS}")
        input_data["seconds"] = seconds
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        seconds = float(input_data["seconds"])
        reason = input_data.get("reason", "")
        logger.debug("sleep_tool", extra={"seconds": seconds, "reason": reason})
        await asyncio.sleep(seconds)
        return ToolResult(
            output=f"Slept {seconds}s." + (f" Reason: {reason}" if reason else ""),
            metadata={"seconds": seconds},
        )
