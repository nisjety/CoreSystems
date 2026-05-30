"""GlobTool — async file pattern matching."""

from __future__ import annotations

import asyncio
import logging
from pathlib import Path
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)

MAX_RESULTS = 500


class GlobTool:
    """Find files matching a glob pattern."""

    name = "glob"
    description = "Search for files matching a glob pattern. Returns matching file paths."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "pattern": {"type": "string", "description": "Glob pattern (e.g., '**/*.py')."},
            "path": {"type": "string", "description": "Root directory to search from.", "default": "."},
        },
        "required": ["pattern"],
    }
    search_hint = "find files glob pattern list directory"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Find files by glob pattern. Use **/*.ext for recursive search."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        pattern = input_data.get("pattern")
        if not pattern or not isinstance(pattern, str):
            raise ValueError("'pattern' is required and must be a non-empty string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        pattern = input_data["pattern"]
        root = Path(input_data.get("path", "."))

        if not root.exists():
            return ToolResult(error=f"Path not found: {root}")

        # Run glob in thread pool to avoid blocking
        loop = asyncio.get_event_loop()

        def _do_glob() -> list[str]:
            matches = []
            for p in root.glob(pattern):
                matches.append(str(p))
                if len(matches) >= MAX_RESULTS:
                    break
            return sorted(matches)

        try:
            matches = await loop.run_in_executor(None, _do_glob)
        except OSError as exc:
            return ToolResult(error=f"Glob failed: {exc}")

        is_truncated = len(matches) >= MAX_RESULTS

        if not matches:
            return ToolResult(output="(no matching files)")

        return ToolResult(
            output="\n".join(matches),
            is_truncated=is_truncated,
            metadata={"count": len(matches)},
        )
