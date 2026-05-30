"""Tool registry — central store for all available tools.

Mirrors CC's tool registry with:
  - register() / get() / list_all()
  - search(query) with keyword scoring (CC's ToolSearchTool pattern)
  - Deferred tool support (tools loaded lazily on first search match)
"""

from __future__ import annotations

import logging
import re
from typing import Any

from app.tools.base import ToolDefinition

logger = logging.getLogger(__name__)


def _tokenize(text: str) -> list[str]:
    """Split text into lowercase tokens for keyword search."""
    return re.findall(r"[a-z0-9]+", text.lower())


def _score_tool(tool: ToolDefinition, query_tokens: list[str]) -> float:
    """Score a tool against query tokens (0.0 – 1.0).

    Scoring heuristic:
      - Name exact match: +0.5
      - Name partial match: +0.3
      - Description keyword match: +0.1 per token
      - Search hint match: +0.2 per token
    """
    if not query_tokens:
        return 0.0

    score = 0.0
    name_lower = tool.name.lower()
    name_tokens = _tokenize(tool.name)

    for qt in query_tokens:
        if qt == name_lower:
            score += 0.5
        elif qt in name_lower or name_lower in qt:
            score += 0.3
        if qt in name_tokens:
            score += 0.2

    desc_tokens = _tokenize(tool.description)
    for qt in query_tokens:
        if qt in desc_tokens:
            score += 0.1

    hint_tokens = _tokenize(tool.search_hint)
    for qt in query_tokens:
        if qt in hint_tokens:
            score += 0.2

    return min(score, 1.0)


class ToolRegistry:
    """Central registry for all available tools.

    Thread-safe for reads (tools dict is only modified during startup).
    """

    def __init__(self) -> None:
        self._tools: dict[str, ToolDefinition] = {}
        self._deferred: dict[str, ToolDefinition] = {}
        self._aliases: dict[str, str] = {}

    def register(self, tool: ToolDefinition) -> None:
        """Register a tool. Raises ValueError if name already taken."""
        if tool.name in self._tools or tool.name in self._deferred:
            raise ValueError(f"Tool already registered: {tool.name}")

        if tool.should_defer:
            self._deferred[tool.name] = tool
            logger.debug("tool_deferred", extra={"name": tool.name})
        else:
            self._tools[tool.name] = tool
            logger.debug("tool_registered", extra={"name": tool.name})

    def register_alias(self, alias: str, tool_name: str) -> None:
        """Register an alias → tool_name mapping."""
        self._aliases[alias] = tool_name

    def get(self, name: str) -> ToolDefinition | None:
        """Get a tool by name or alias. Returns None if not found."""
        resolved = self._aliases.get(name, name)
        tool = self._tools.get(resolved)
        if tool:
            return tool
        # Check deferred — if found, promote to active
        deferred = self._deferred.pop(resolved, None)
        if deferred:
            self._tools[resolved] = deferred
            logger.info("tool_promoted_from_deferred", extra={"name": resolved})
            return deferred
        return None

    def list_all(self) -> list[ToolDefinition]:
        """List all non-deferred (active) tools."""
        return list(self._tools.values())

    def list_deferred(self) -> list[ToolDefinition]:
        """List deferred tools (not yet loaded)."""
        return list(self._deferred.values())

    def list_names(self) -> list[str]:
        """List names of all active tools."""
        return list(self._tools.keys())

    def search(self, query: str, limit: int = 10) -> list[tuple[ToolDefinition, float]]:
        """Search tools by keyword query.

        Returns list of (tool, score) sorted by score descending.
        Includes deferred tools in search results.
        """
        query_tokens = _tokenize(query)
        if not query_tokens:
            return []

        all_tools = list(self._tools.values()) + list(self._deferred.values())
        scored = [(t, _score_tool(t, query_tokens)) for t in all_tools]
        scored = [(t, s) for t, s in scored if s > 0.0]
        scored.sort(key=lambda x: x[1], reverse=True)
        return scored[:limit]

    @property
    def count(self) -> int:
        """Total number of registered tools (active + deferred)."""
        return len(self._tools) + len(self._deferred)

    @property
    def active_count(self) -> int:
        return len(self._tools)

    def clear(self) -> None:
        """Remove all tools (for testing)."""
        self._tools.clear()
        self._deferred.clear()
        self._aliases.clear()
