"""MCP server management routes — /v1/mcp."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.domain import MCPServerConfig, MCPScope
from app.mcp import registry as mcp_reg
from app.mcp import scoped_config

router = APIRouter(prefix="/v1/mcp", tags=["mcp"])


class OverrideRequest(BaseModel):
    server_id: str
    session_id: str | None = None
    org_id: str | None = None
    user_id: str | None = None
    enabled: bool = True


class EnabledCheckRequest(BaseModel):
    server_id: str
    session_id: str | None = None
    org_id: str | None = None
    user_id: str | None = None


@router.get("")
async def list_servers(scope: str | None = None) -> dict:
    scope_enum = MCPScope(scope) if scope else None
    servers = await mcp_reg.list_servers(scope=scope_enum)
    return {"servers": [s.model_dump(mode="json") for s in servers]}


@router.get("/{server_id}")
async def get_server(server_id: str) -> dict:
    server = await mcp_reg.get_server(server_id)
    if server is None:
        raise HTTPException(404, "MCP server not found")
    return server.model_dump(mode="json")


@router.put("/{server_id}")
async def register_server(server_id: str, body: MCPServerConfig) -> dict:
    cfg = body.model_copy(update={"server_id": server_id})
    saved = await mcp_reg.register(cfg)
    return saved.model_dump(mode="json")


@router.delete("/{server_id}", status_code=204)
async def deregister_server(server_id: str) -> None:
    ok = await mcp_reg.deregister(server_id)
    if not ok:
        raise HTTPException(404, "MCP server not found")


@router.post("/enabled")
async def check_enabled(req: EnabledCheckRequest) -> dict:
    enabled = await scoped_config.is_enabled(
        req.server_id,
        session_id=req.session_id,
        org_id=req.org_id,
        user_id=req.user_id,
    )
    return {"server_id": req.server_id, "enabled": enabled}


@router.post("/override")
async def set_override(req: OverrideRequest) -> dict:
    await scoped_config.set_override(
        req.server_id,
        session_id=req.session_id,
        org_id=req.org_id,
        user_id=req.user_id,
        enabled=req.enabled,
    )
    return {"ok": True}
