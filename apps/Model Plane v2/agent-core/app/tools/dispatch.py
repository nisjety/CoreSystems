"""Tool dispatcher — look up tool → run hooks → validate → execute → hooks → result.

Replaces the ad-hoc action dispatch in turn_loop.py with a structured
pipeline matching CC's tool execution flow:

  1. Look up tool in registry
  2. Run pre-tool-use hooks (may block or modify)
  3. Validate input via tool.validate_input()
  4. Execute tool.call()
  5. Run post-tool-use hooks
  6. Return ToolResult
"""

from __future__ import annotations

import logging
import time
from typing import Any

from app.tools.base import ToolDefinition, ToolResult
from app.tools.registry import ToolRegistry

logger = logging.getLogger(__name__)


class ToolNotFoundError(Exception):
    """Raised when a tool is not found in the registry."""

    def __init__(self, name: str) -> None:
        self.name = name
        super().__init__(f"Tool not found: {name}")


class ToolValidationError(Exception):
    """Raised when tool input validation fails."""

    def __init__(self, name: str, detail: str) -> None:
        self.name = name
        self.detail = detail
        super().__init__(f"Validation failed for {name}: {detail}")


class ToolBlockedError(Exception):
    """Raised when a pre-hook blocks tool execution."""

    def __init__(self, name: str, reason: str) -> None:
        self.name = name
        self.reason = reason
        super().__init__(f"Tool blocked by hook: {name} — {reason}")


class ToolDispatcher:
    """Execute tools through the full pipeline: lookup → hooks → validate → call → hooks.

    Args:
        registry: The tool registry to look up tools.
        pre_hook_fn: Optional async callable(tool_name, input_data) -> (proceed: bool, modified_input, block_reason).
        post_hook_fn: Optional async callable(tool_name, input_data, result) -> ToolResult.
    """

    def __init__(
        self,
        registry: ToolRegistry,
        pre_hook_fn: Any | None = None,
        post_hook_fn: Any | None = None,
    ) -> None:
        self._registry = registry
        self._pre_hook_fn = pre_hook_fn
        self._post_hook_fn = post_hook_fn

    async def dispatch(
        self,
        tool_name: str,
        input_data: dict[str, Any],
    ) -> ToolResult:
        """Full dispatch pipeline for a tool call.

        Returns ToolResult on success, raises on not-found / blocked / validation.
        """
        start = time.monotonic()

        # 1. Look up
        tool = self._registry.get(tool_name)
        if tool is None:
            raise ToolNotFoundError(tool_name)

        # 2. Pre-hooks
        if self._pre_hook_fn is not None:
            proceed, input_data, block_reason = await self._pre_hook_fn(
                tool_name, input_data
            )
            if not proceed:
                raise ToolBlockedError(tool_name, block_reason or "blocked by hook")

        # 3. Validate
        try:
            input_data = tool.validate_input(input_data)
        except (ValueError, TypeError) as exc:
            raise ToolValidationError(tool_name, str(exc)) from exc

        # 4. Execute
        try:
            result = await tool.call(input_data)
        except Exception as exc:
            logger.error(
                "tool_execution_error",
                extra={"tool": tool_name, "error": str(exc)},
            )
            result = ToolResult(error=str(exc))

        # 5. Post-hooks
        if self._post_hook_fn is not None:
            result = await self._post_hook_fn(tool_name, input_data, result)

        elapsed = time.monotonic() - start
        logger.info(
            "tool_dispatched",
            extra={
                "tool": tool_name,
                "success": result.success,
                "elapsed_ms": round(elapsed * 1000, 1),
            },
        )

        return result
