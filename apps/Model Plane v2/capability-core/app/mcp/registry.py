"""MCP server registry — CRUD wrapper around repository + NATS events."""

from __future__ import annotations

import logging

from app import repository
from app.domain import MCPServerConfig, MCPScope

logger = logging.getLogger(__name__)


async def list_servers(
    *, scope: MCPScope | None = None
) -> list[MCPServerConfig]:
    return await repository.list_mcp_servers(scope=scope)


async def get_server(server_id: str) -> MCPServerConfig | None:
    return await repository.get_mcp_server(server_id)


async def register(config: MCPServerConfig) -> MCPServerConfig:
    """Register or update an MCP server config."""
    saved = await repository.upsert_mcp_server(config)
    logger.info(
        "mcp server registered: %s  scope=%s",
        saved.server_id,
        saved.scope.value,
    )
    return saved


async def deregister(server_id: str) -> bool:
    """Remove an MCP server config."""
    existing = await repository.get_mcp_server(server_id)
    if existing is None:
        return False
    await repository.delete_mcp_server(server_id)
    logger.info("mcp server deregistered: %s", server_id)
    return True
