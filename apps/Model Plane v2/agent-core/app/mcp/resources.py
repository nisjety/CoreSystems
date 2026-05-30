"""MCP resource listing and reading.

CC pattern: MCP servers can expose resources (files, database rows,
configuration) that the agent can list and read without tool calls.

Provides:
  - ``list_resources()`` — enumerate available resources from an MCP server
  - ``read_resource()``  — fetch content of a specific resource
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class McpResource(BaseModel):
    """An MCP resource descriptor."""

    uri: str
    name: str
    description: str = ""
    mime_type: str = "text/plain"


class McpResourceContent(BaseModel):
    """Content of a read resource."""

    uri: str
    content: str
    mime_type: str = "text/plain"


class ResourceManager:
    """Manages resource listing and reading for MCP servers.

    Each MCP client can register a resource provider. The manager
    aggregates them across all connected servers.
    """

    def __init__(self) -> None:
        self._providers: dict[str, ResourceProvider] = {}

    def register_provider(self, server_name: str, provider: "ResourceProvider") -> None:
        self._providers[server_name] = provider

    def unregister_provider(self, server_name: str) -> None:
        self._providers.pop(server_name, None)

    async def list_resources(
        self, server_name: str | None = None
    ) -> list[McpResource]:
        """List resources from a specific server or all servers."""
        if server_name:
            provider = self._providers.get(server_name)
            if provider is None:
                return []
            return await provider.list_resources()

        all_resources: list[McpResource] = []
        for provider in self._providers.values():
            try:
                resources = await provider.list_resources()
                all_resources.extend(resources)
            except Exception as exc:
                logger.warning(
                    "resource_list_failed",
                    extra={"error": str(exc)},
                )
        return all_resources

    async def read_resource(
        self, server_name: str, uri: str
    ) -> McpResourceContent | None:
        """Read a specific resource from a server."""
        provider = self._providers.get(server_name)
        if provider is None:
            return None
        try:
            return await provider.read_resource(uri)
        except Exception as exc:
            logger.warning(
                "resource_read_failed",
                extra={"server": server_name, "uri": uri, "error": str(exc)},
            )
            return None


class ResourceProvider:
    """Base resource provider. Subclass or pass callables."""

    def __init__(
        self,
        list_fn: Any | None = None,
        read_fn: Any | None = None,
    ) -> None:
        self._list_fn = list_fn
        self._read_fn = read_fn

    async def list_resources(self) -> list[McpResource]:
        if self._list_fn:
            return await self._list_fn()
        return []

    async def read_resource(self, uri: str) -> McpResourceContent | None:
        if self._read_fn:
            return await self._read_fn(uri)
        return None
