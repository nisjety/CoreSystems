"""FileReadTool — async file reading with line ranges and encoding detection."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)

MAX_FILE_BYTES = 512 * 1024  # 512 KB


class FileReadTool:
    """Read file contents with optional line range."""

    name = "file_read"
    description = "Read the contents of a file, optionally specifying a line range."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Absolute or relative file path."},
            "start_line": {"type": "integer", "description": "1-based start line (inclusive)."},
            "end_line": {"type": "integer", "description": "1-based end line (inclusive)."},
        },
        "required": ["path"],
    }
    search_hint = "read file cat view contents lines"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Read file contents. Specify start_line/end_line for partial reads."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        file_path = input_data.get("path")
        if not file_path or not isinstance(file_path, str):
            raise ValueError("'path' is required and must be a non-empty string")

        start = input_data.get("start_line")
        end = input_data.get("end_line")
        if start is not None and start < 1:
            raise ValueError("start_line must be >= 1")
        if end is not None and end < 1:
            raise ValueError("end_line must be >= 1")
        if start is not None and end is not None and start > end:
            raise ValueError("start_line must be <= end_line")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        file_path = Path(input_data["path"])
        start_line = input_data.get("start_line")
        end_line = input_data.get("end_line")

        if not file_path.exists():
            return ToolResult(error=f"File not found: {file_path}")
        if not file_path.is_file():
            return ToolResult(error=f"Not a file: {file_path}")

        # Check size
        try:
            size = file_path.stat().st_size
        except OSError as exc:
            return ToolResult(error=f"Cannot stat file: {exc}")

        is_truncated = False

        # Try utf-8, fall back to latin-1
        for encoding in ("utf-8", "latin-1"):
            try:
                text = file_path.read_text(encoding=encoding)
                break
            except UnicodeDecodeError:
                continue
            except OSError as exc:
                return ToolResult(error=f"Cannot read file: {exc}")
        else:
            return ToolResult(error="Cannot decode file with utf-8 or latin-1")

        if len(text) > MAX_FILE_BYTES:
            text = text[:MAX_FILE_BYTES]
            is_truncated = True

        # Apply line range
        if start_line is not None or end_line is not None:
            lines = text.splitlines(keepends=True)
            s = (start_line or 1) - 1
            e = end_line or len(lines)
            text = "".join(lines[s:e])

        total_lines = text.count("\n") + (1 if text and not text.endswith("\n") else 0)

        return ToolResult(
            output=text,
            is_truncated=is_truncated,
            metadata={"lines": total_lines, "encoding": encoding, "size": size},
        )
