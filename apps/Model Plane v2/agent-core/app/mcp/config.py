"""MCP integration domain types.

Defines configuration and tool schemas for connecting to external MCP servers.
Supports stdio (subprocess + JSON-RPC), HTTP, SSE, and WebSocket transports.
"""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class McpTransport(str, Enum):
    STDIO = "stdio"
    HTTP = "http"
    SSE = "sse"     # HTTP Server-Sent Events (EventSource / httpx-sse)
    WS = "ws"       # WebSocket


class McpOAuthConfig(BaseModel):
    """OAuth 2.0 / PKCE config for authenticating with an MCP server."""

    client_id: str
    authorization_url: str
    token_url: str
    scopes: list[str] = Field(default_factory=list)
    # Optional: pre-seeded client_secret for confidential clients
    client_secret: str | None = None


class McpServerStatus(str, Enum):
    DISCONNECTED = "disconnected"
    CONNECTED = "connected"
    ERROR = "error"


class McpServerConfig(BaseModel):
    """Persistent MCP server configuration stored in Postgres."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    org_id: str
    name: str = Field(..., min_length=1, max_length=128)
    transport: McpTransport
    # stdio fields
    command: str | None = None
    args: list[str] = Field(default_factory=list)
    env: dict[str, str] = Field(default_factory=dict)
    # http / sse / ws fields
    url: str | None = None
    headers: dict[str, str] = Field(default_factory=dict)
    # OAuth (optional — any transport may require OAuth tokens)
    oauth: McpOAuthConfig | None = None
    enabled: bool = True
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class McpTool(BaseModel):
    """A tool discovered from a connected MCP server."""

    name: str
    description: str = ""
    input_schema: dict[str, Any] = Field(default_factory=dict)
    server_name: str = ""  # which MCP server provides this tool


class McpToolCallRequest(BaseModel):
    """Request to invoke an MCP tool."""

    tool_name: str
    arguments: dict[str, Any] = Field(default_factory=dict)


class McpToolCallResponse(BaseModel):
    """Response from an MCP tool invocation."""

    content: Any = None
    is_error: bool = False
    error_message: str | None = None
