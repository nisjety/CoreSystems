"""MCP tool proxy — wraps MCP tools as async callables for the action executor.

The proxy creates a function that routes tool calls through the MCPClient,
allowing MCP tools to be dispatched like any built-in tool in the action loop.
"""

from __future__ import annotations

import logging
from typing import Any, Callable, Awaitable

from app.mcp.client import MCPClient
from app.mcp.config import McpToolCallResponse

logger = logging.getLogger(__name__)

# Type alias for the callable produced by build_mcp_tool_action
McpToolAction = Callable[..., Awaitable[dict[str, Any]]]


def build_mcp_tool_action(
    server_name: str,
    tool_name: str,
    client: MCPClient,
) -> McpToolAction:
    """Build an async callable that invokes an MCP tool via the client.

    Returns a function matching the signature expected by the action executor:
        async def(parameters: dict, **kwargs) -> dict
    """

    async def _call_mcp_tool(
        parameters: dict[str, Any] | None = None,
        **kwargs: Any,
    ) -> dict[str, Any]:
        params = parameters or {}
        logger.debug(
            "mcp_tool_call",
            extra={
                "server": server_name,
                "tool": tool_name,
                "params_keys": list(params.keys()),
            },
        )

        response: McpToolCallResponse = await client.call_tool(tool_name, params)

        if response.is_error:
            return {
                "error": True,
                "message": response.error_message or "MCP tool call failed",
                "server": server_name,
                "tool": tool_name,
            }

        return {
            "content": response.content,
            "server": server_name,
            "tool": tool_name,
        }

    return _call_mcp_tool


def parse_mcp_tool_name(prefixed_name: str) -> tuple[str, str] | None:
    """Parse 'mcp:server_name:tool_name' into (server_name, tool_name).

    Returns None if the name doesn't match the MCP tool naming convention.
    """
    if not prefixed_name.startswith("mcp:"):
        return None

    parts = prefixed_name.split(":", 2)
    if len(parts) != 3:
        return None

    return parts[1], parts[2]
