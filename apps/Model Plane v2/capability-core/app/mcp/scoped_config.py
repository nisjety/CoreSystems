"""MCP scoped configuration — session / org / user overrides."""

from __future__ import annotations

import logging

from app import repository

logger = logging.getLogger(__name__)


async def is_enabled(
    server_id: str,
    *,
    session_id: str | None = None,
    org_id: str | None = None,
    user_id: str | None = None,
) -> bool:
    """Check if an MCP server is enabled for a given scope chain.

    Priority: session override > server global setting.
    """
    return await repository.is_mcp_enabled(
        server_id,
        session_id=session_id,
        org_id=org_id,
        user_id=user_id,
    )


async def set_override(
    server_id: str,
    *,
    session_id: str | None = None,
    org_id: str | None = None,
    user_id: str | None = None,
    enabled: bool = True,
) -> None:
    """Create or update a scoped override for an MCP server."""
    await repository.set_mcp_override(
        server_id,
        session_id=session_id,
        org_id=org_id,
        user_id=user_id,
        enabled=enabled,
    )
    logger.info(
        "mcp override set: server=%s session=%s enabled=%s",
        server_id,
        session_id,
        enabled,
    )
