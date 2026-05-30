"""FileWriteTool — create or overwrite files with automatic directory creation."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)


class FileWriteTool:
    """Write content to a file, creating directories as needed."""

    name = "file_write"
    description = "Create a new file or overwrite an existing file with the provided content."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Absolute or relative file path."},
            "content": {"type": "string", "description": "Content to write to the file."},
        },
        "required": ["path", "content"],
    }
    search_hint = "write create file new save"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return True

    def prompt(self) -> str:
        return "Create or overwrite a file. Directories are created automatically."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        file_path = input_data.get("path")
        if not file_path or not isinstance(file_path, str):
            raise ValueError("'path' is required and must be a non-empty string")
        content = input_data.get("content")
        if content is None or not isinstance(content, str):
            raise ValueError("'content' is required and must be a string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        file_path = Path(input_data["path"])
        content = input_data["content"]

        try:
            file_path.parent.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            return ToolResult(error=f"Cannot create directory: {exc}")

        try:
            file_path.write_text(content, encoding="utf-8")
        except OSError as exc:
            return ToolResult(error=f"Cannot write file: {exc}")

        return ToolResult(
            output=f"Wrote {len(content)} bytes to {file_path}",
            metadata={"path": str(file_path), "bytes": len(content)},
        )
