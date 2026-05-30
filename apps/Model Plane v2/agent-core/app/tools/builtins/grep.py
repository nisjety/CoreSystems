"""GrepTool — async text search using ripgrep or fallback grep."""

from __future__ import annotations

import asyncio
import logging
import shutil
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)

MAX_OUTPUT_BYTES = 128 * 1024


class GrepTool:
    """Search file contents for a pattern using ripgrep (preferred) or grep."""

    name = "grep"
    description = "Search for a text pattern in files. Supports regex, include/exclude patterns."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "pattern": {"type": "string", "description": "Search pattern (regex supported)."},
            "path": {"type": "string", "description": "Directory or file to search.", "default": "."},
            "include": {"type": "string", "description": "Glob pattern for files to include."},
            "is_regex": {"type": "boolean", "description": "Whether pattern is a regex.", "default": True},
        },
        "required": ["pattern"],
    }
    search_hint = "search find grep text pattern regex"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Search files for text patterns. Use regex with alternation (|) for multi-word search."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        pattern = input_data.get("pattern")
        if not pattern or not isinstance(pattern, str):
            raise ValueError("'pattern' is required and must be a non-empty string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        pattern = input_data["pattern"]
        path = input_data.get("path", ".")
        include = input_data.get("include")
        is_regex = input_data.get("is_regex", True)

        # Prefer ripgrep, fall back to grep
        rg = shutil.which("rg")
        if rg:
            cmd = [rg, "--no-heading", "-n", "--max-count", "100"]
            if not is_regex:
                cmd.append("--fixed-strings")
            if include:
                cmd.extend(["--glob", include])
            cmd.extend([pattern, path])
        else:
            cmd = ["grep", "-rn", "--max-count=100"]
            if not is_regex:
                cmd.append("-F")
            if include:
                cmd.extend(["--include", include])
            cmd.extend([pattern, path])

        try:
            proc = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=30)
        except asyncio.TimeoutError:
            return ToolResult(error="Search timed out after 30s")
        except FileNotFoundError:
            return ToolResult(error="Neither rg nor grep found on system")

        output = stdout.decode("utf-8", errors="replace")
        is_truncated = len(output) > MAX_OUTPUT_BYTES
        if is_truncated:
            output = output[:MAX_OUTPUT_BYTES]

        if proc.returncode == 1 and not output:
            return ToolResult(output="(no matches found)")

        return ToolResult(
            output=output or "(no matches found)",
            is_truncated=is_truncated,
            metadata={"exit_code": proc.returncode},
        )
