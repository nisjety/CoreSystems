"""MCP server manager — manages per-org MCP client connections.

Responsible for:
- Loading MCP server configs from Postgres per org
- Maintaining connected MCPClient pool
- Providing aggregated tool lists for the capability pipeline
- Reconnecting on errors
"""

from __future__ import annotations

import json
import logging
from typing import Any

from app.database import get_pool
from app.mcp.client import MCPClient
from app.mcp.config import McpServerConfig, McpServerStatus, McpTool, McpTransport

logger = logging.getLogger(__name__)


class MCPServerManager:
    """Manages MCP server connections across all orgs."""

    def __init__(self) -> None:
        # org_id → server_name → MCPClient
        self._clients: dict[str, dict[str, MCPClient]] = {}

    async def connect_org(self, org_id: str) -> list[McpTool]:
        """Load configs for an org from DB, connect all enabled servers, return tools."""
        configs = await _load_configs(org_id)
        if not configs:
            return []

        org_clients: dict[str, MCPClient] = {}
        tools: list[McpTool] = []

        for config in configs:
            if not config.enabled:
                continue
            try:
                client = MCPClient(config)
                await client.connect()
                org_clients[config.name] = client
                tools.extend(client.tools)
            except Exception as exc:
                logger.error(
                    "mcp_server_connect_skip",
                    extra={"server": config.name, "org_id": org_id, "error": str(exc)},
                )
                continue

        self._clients[org_id] = org_clients
        return tools

    async def disconnect_org(self, org_id: str) -> None:
        """Disconnect all MCP servers for an org."""
        org_clients = self._clients.pop(org_id, {})
        for name, client in org_clients.items():
            try:
                await client.disconnect()
            except Exception as exc:
                logger.warning(
                    "mcp_disconnect_error",
                    extra={"server": name, "error": str(exc)},
                )

    async def disconnect_all(self) -> None:
        """Disconnect all MCP servers across all orgs."""
        org_ids = list(self._clients.keys())
        for org_id in org_ids:
            await self.disconnect_org(org_id)

    async def get_tools_for_org(self, org_id: str) -> list[McpTool]:
        """Get all tools from connected MCP servers for an org.

        Lazily connects if not already connected.
        """
        if org_id not in self._clients:
            return await self.connect_org(org_id)

        tools: list[McpTool] = []
        for client in self._clients.get(org_id, {}).values():
            if client.status == McpServerStatus.CONNECTED:
                tools.extend(client.tools)
        return tools

    async def get_client(self, org_id: str, server_name: str) -> MCPClient | None:
        """Get a specific MCP client by org and server name."""
        return self._clients.get(org_id, {}).get(server_name)

    async def reconnect_server(self, org_id: str, server_name: str) -> bool:
        """Reconnect a specific MCP server (e.g. after error)."""
        client = self._clients.get(org_id, {}).get(server_name)
        if client is None:
            return False

        try:
            await client.disconnect()
            await client.connect()
            return True
        except Exception as exc:
            logger.error(
                "mcp_reconnect_failed",
                extra={"server": server_name, "org_id": org_id, "error": str(exc)},
            )
            return False

    def status_for_org(self, org_id: str) -> dict[str, str]:
        """Return connection status for all servers in an org."""
        return {
            name: client.status.value
            for name, client in self._clients.get(org_id, {}).items()
        }


async def _load_configs(org_id: str) -> list[McpServerConfig]:
    """Load MCP server configs from Postgres."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, org_id, name, transport, command,
                   args_json, env_json, url, headers_json, enabled,
                   created_at, updated_at
            FROM mcp_servers
            WHERE org_id = $1 AND enabled = true
            ORDER BY name
            """,
            org_id,
        )

    return [_row_to_config(r) for r in rows]


def _row_to_config(row: Any) -> McpServerConfig:
    """Convert a DB row to McpServerConfig."""
    return McpServerConfig(
        id=str(row["id"]),
        org_id=str(row["org_id"]),
        name=row["name"],
        transport=McpTransport(row["transport"]),
        command=row["command"],
        args=json.loads(row["args_json"]) if row["args_json"] else [],
        env=json.loads(row["env_json"]) if row["env_json"] else {},
        url=row["url"],
        headers=json.loads(row["headers_json"]) if row["headers_json"] else {},
        enabled=row["enabled"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )
