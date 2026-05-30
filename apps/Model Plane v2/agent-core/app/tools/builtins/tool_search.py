"""ToolSearchTool — keyword search over the tool registry."""

from __future__ import annotations

import logging
from typing import Any

from app.tools.base import ToolDefinition, ToolResult
from app.tools.registry import ToolRegistry

logger = logging.getLogger(__name__)


class ToolSearchTool:
    """Search the tool registry to find tools matching a query."""

    name = "tool_search"
    description = "Search for tools by name or description. Returns matching tools with their descriptions."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Search query for finding tools."},
        },
        "required": ["query"],
    }
    search_hint = "tool search find list available tools"
    should_defer = False

    # Will be set by register_builtins or externally
    _registry: ToolRegistry | None = None

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Search for available tools by keyword."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        query = input_data.get("query")
        if not query or not isinstance(query, str):
            raise ValueError("'query' is required and must be a non-empty string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        query = input_data["query"]

        if self._registry is None:
            return ToolResult(error="No tool registry configured for ToolSearchTool")

        results = self._registry.search(query, limit=10)
        if not results:
            return ToolResult(output=f"No tools found matching '{query}'")

        lines = []
        for tool, score in results:
            ro = "read-only" if tool.is_read_only() else "read-write"
            lines.append(f"- {tool.name} ({ro}, score={score:.2f}): {tool.description}")

        return ToolResult(
            output="\n".join(lines),
            metadata={"count": len(results)},
        )
