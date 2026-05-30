"""MCP tool name normalization.

CC pattern: MCP tool names are normalized to a consistent format:
  ``mcp__<server>__<tool>``

This avoids collisions between tools from different servers that
share the same base name (e.g. two servers both exposing "search").
"""

from __future__ import annotations

import re

# Separator used in normalized names
_SEP = "__"

# Characters allowed in normalized segments
_CLEAN_RE = re.compile(r"[^a-zA-Z0-9_]")


def normalize_tool_name(server_name: str, tool_name: str) -> str:
    """Normalize an MCP tool name to ``mcp__<server>__<tool>`` format."""
    server = _CLEAN_RE.sub("_", server_name).strip("_")
    tool = _CLEAN_RE.sub("_", tool_name).strip("_")
    return f"mcp{_SEP}{server}{_SEP}{tool}"


def parse_normalized_name(name: str) -> tuple[str, str] | None:
    """Parse a normalized name back to (server_name, tool_name).

    Returns None if the name doesn't match the expected format.
    """
    if not name.startswith(f"mcp{_SEP}"):
        return None
    rest = name[len(f"mcp{_SEP}"):]
    parts = rest.split(_SEP, 1)
    if len(parts) != 2:
        return None
    return parts[0], parts[1]


def is_mcp_tool(name: str) -> bool:
    """Check if a tool name has MCP normalization prefix."""
    return name.startswith(f"mcp{_SEP}")
