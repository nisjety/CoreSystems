"""MCP server management API — /api/v1/mcp/servers.

CRUD for MCP server configurations + test endpoint + tool discovery.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from app.database import get_pool
from app.mcp.config import McpServerConfig, McpTool, McpTransport

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/mcp/servers", tags=["mcp"])


# ---- Request / response schemas ----


class CreateMcpServerRequest(BaseModel):
    name: str = Field(..., min_length=1, max_length=128)
    transport: McpTransport
    command: str | None = None
    args: list[str] = Field(default_factory=list)
    env: dict[str, str] = Field(default_factory=dict)
    url: str | None = None
    headers: dict[str, str] = Field(default_factory=dict)
    enabled: bool = True


class UpdateMcpServerRequest(BaseModel):
    name: str | None = None
    transport: McpTransport | None = None
    command: str | None = None
    args: list[str] | None = None
    env: dict[str, str] | None = None
    url: str | None = None
    headers: dict[str, str] | None = None
    enabled: bool | None = None


class McpServerResponse(BaseModel):
    id: str
    org_id: str
    name: str
    transport: McpTransport
    command: str | None
    args: list[str]
    env: dict[str, str]
    url: str | None
    headers: dict[str, str]
    enabled: bool
    created_at: datetime
    updated_at: datetime


class McpToolResponse(BaseModel):
    name: str
    description: str
    input_schema: dict[str, Any]
    server_name: str


class McpTestResult(BaseModel):
    connected: bool
    tools_count: int
    error: str | None = None


# ---- Helpers ----


def _org_id_from_request(request: Request) -> str:
    org_id = getattr(request.state, "org_id", None)
    if not org_id:
        raise HTTPException(status_code=401, detail="org_id required")
    return org_id


# ---- Endpoints ----


@router.get("", response_model=list[McpServerResponse])
async def list_mcp_servers(request: Request) -> list[McpServerResponse]:
    """List all MCP server configs for the org."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, org_id, name, transport, command,
                   args_json, env_json, url, headers_json, enabled,
                   created_at, updated_at
            FROM mcp_servers
            WHERE org_id = $1
            ORDER BY name
            """,
            org_id,
        )
    return [_row_to_response(r) for r in rows]


@router.post("", response_model=McpServerResponse, status_code=201)
async def create_mcp_server(
    request: Request, body: CreateMcpServerRequest
) -> McpServerResponse:
    """Create a new MCP server config."""
    org_id = _org_id_from_request(request)
    server_id = str(uuid4())
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO mcp_servers (
                id, org_id, name, transport, command,
                args_json, env_json, url, headers_json, enabled,
                created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
            """,
            server_id,
            org_id,
            body.name,
            body.transport.value,
            body.command,
            json.dumps(body.args),
            json.dumps(body.env),
            body.url,
            json.dumps(body.headers),
            body.enabled,
            now,
            now,
        )

    logger.info("mcp_server_created", extra={"id": server_id, "org_id": org_id})

    return McpServerResponse(
        id=server_id,
        org_id=org_id,
        name=body.name,
        transport=body.transport,
        command=body.command,
        args=body.args,
        env=body.env,
        url=body.url,
        headers=body.headers,
        enabled=body.enabled,
        created_at=now,
        updated_at=now,
    )


@router.patch("/{server_id}", response_model=McpServerResponse)
async def update_mcp_server(
    request: Request, server_id: str, body: UpdateMcpServerRequest
) -> McpServerResponse:
    """Update an MCP server config (partial)."""
    org_id = _org_id_from_request(request)
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        existing = await conn.fetchrow(
            "SELECT * FROM mcp_servers WHERE id = $1 AND org_id = $2",
            server_id,
            org_id,
        )
        if not existing:
            raise HTTPException(status_code=404, detail="MCP server not found")

        updates: dict[str, Any] = {"updated_at": now}
        if body.name is not None:
            updates["name"] = body.name
        if body.transport is not None:
            updates["transport"] = body.transport.value
        if body.command is not None:
            updates["command"] = body.command
        if body.args is not None:
            updates["args_json"] = json.dumps(body.args)
        if body.env is not None:
            updates["env_json"] = json.dumps(body.env)
        if body.url is not None:
            updates["url"] = body.url
        if body.headers is not None:
            updates["headers_json"] = json.dumps(body.headers)
        if body.enabled is not None:
            updates["enabled"] = body.enabled

        set_clauses = ", ".join(f"{k} = ${i+2}" for i, k in enumerate(updates))
        values = [server_id, *updates.values()]
        await conn.execute(
            f"UPDATE mcp_servers SET {set_clauses} WHERE id = $1",
            *values,
        )

    # Re-read
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT * FROM mcp_servers WHERE id = $1", server_id
        )
    return _row_to_response(row)


@router.delete("/{server_id}", status_code=204)
async def delete_mcp_server(request: Request, server_id: str) -> None:
    """Delete an MCP server config."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute(
            "DELETE FROM mcp_servers WHERE id = $1 AND org_id = $2",
            server_id,
            org_id,
        )
    if result == "DELETE 0":
        raise HTTPException(status_code=404, detail="MCP server not found")

    # Disconnect from manager if running
    from app.mcp.server_manager import MCPServerManager

    # Manager will be on app.state — disconnect handled lazily
    logger.info("mcp_server_deleted", extra={"id": server_id, "org_id": org_id})


@router.post("/{server_id}/test", response_model=McpTestResult)
async def test_mcp_server(request: Request, server_id: str) -> McpTestResult:
    """Test connection to an MCP server."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()

    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT * FROM mcp_servers WHERE id = $1 AND org_id = $2",
            server_id,
            org_id,
        )
    if not row:
        raise HTTPException(status_code=404, detail="MCP server not found")

    from app.mcp.client import MCPClient
    from app.mcp.server_manager import _row_to_config

    config = _row_to_config(row)
    client = MCPClient(config)

    try:
        await client.connect()
        tools_count = len(client.tools)
        await client.disconnect()
        return McpTestResult(connected=True, tools_count=tools_count)
    except Exception as exc:
        return McpTestResult(connected=False, tools_count=0, error=str(exc))


@router.get("/{server_id}/tools", response_model=list[McpToolResponse])
async def list_mcp_server_tools(request: Request, server_id: str) -> list[McpToolResponse]:
    """List tools discovered from a connected MCP server."""
    org_id = _org_id_from_request(request)

    # Try to get from running manager first
    mcp_manager = getattr(request.app.state, "mcp_manager", None)
    if mcp_manager:
        pool = await get_pool()
        async with pool.acquire() as conn:
            row = await conn.fetchrow(
                "SELECT name FROM mcp_servers WHERE id = $1 AND org_id = $2",
                server_id,
                org_id,
            )
        if not row:
            raise HTTPException(status_code=404, detail="MCP server not found")

        client = await mcp_manager.get_client(org_id, row["name"])
        if client:
            return [
                McpToolResponse(
                    name=t.name,
                    description=t.description,
                    input_schema=t.input_schema,
                    server_name=t.server_name,
                )
                for t in client.tools
            ]

    # Fall back to fresh connection for tool discovery
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT * FROM mcp_servers WHERE id = $1 AND org_id = $2",
            server_id,
            org_id,
        )
    if not row:
        raise HTTPException(status_code=404, detail="MCP server not found")

    from app.mcp.client import MCPClient
    from app.mcp.server_manager import _row_to_config

    config = _row_to_config(row)
    client = MCPClient(config)
    try:
        await client.connect()
        tools = [
            McpToolResponse(
                name=t.name,
                description=t.description,
                input_schema=t.input_schema,
                server_name=t.server_name,
            )
            for t in client.tools
        ]
        await client.disconnect()
        return tools
    except Exception as exc:
        raise HTTPException(status_code=502, detail=f"Failed to connect: {exc}")


def _row_to_response(row: Any) -> McpServerResponse:
    return McpServerResponse(
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
