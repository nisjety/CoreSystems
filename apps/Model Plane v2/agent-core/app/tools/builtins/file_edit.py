"""FileEditTool — search-and-replace editing with diff validation."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)


class FileEditTool:
    """Edit a file by replacing an exact string occurrence."""

    name = "file_edit"
    description = "Replace an exact string in a file with a new string. The old string must appear exactly once."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Absolute or relative file path."},
            "old_string": {"type": "string", "description": "Exact text to find (must appear once)."},
            "new_string": {"type": "string", "description": "Replacement text."},
        },
        "required": ["path", "old_string", "new_string"],
    }
    search_hint = "edit replace modify change update file"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Replace exact text in a file. old_string must match exactly once. "
            "Include sufficient context (3+ lines before/after) to ensure uniqueness."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        for field in ("path", "old_string", "new_string"):
            val = input_data.get(field)
            if val is None or not isinstance(val, str):
                raise ValueError(f"'{field}' is required and must be a string")
        if not input_data["old_string"]:
            raise ValueError("'old_string' must not be empty")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        file_path = Path(input_data["path"])
        old_string = input_data["old_string"]
        new_string = input_data["new_string"]

        if not file_path.exists():
            return ToolResult(error=f"File not found: {file_path}")
        if not file_path.is_file():
            return ToolResult(error=f"Not a file: {file_path}")

        try:
            content = file_path.read_text(encoding="utf-8")
        except OSError as exc:
            return ToolResult(error=f"Cannot read file: {exc}")

        count = content.count(old_string)
        if count == 0:
            return ToolResult(error="old_string not found in file")
        if count > 1:
            return ToolResult(
                error=f"old_string found {count} times — must appear exactly once"
            )

        new_content = content.replace(old_string, new_string, 1)

        try:
            file_path.write_text(new_content, encoding="utf-8")
        except OSError as exc:
            return ToolResult(error=f"Cannot write file: {exc}")

        return ToolResult(
            output=f"Replaced 1 occurrence in {file_path}",
            metadata={"path": str(file_path)},
        )
