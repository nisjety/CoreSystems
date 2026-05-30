"""Tool catalog routes — /v1/catalog."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app import repository
from app.catalog import pool as pool_mod
from app.catalog import search as search_mod
from app.domain import ToolDescriptor

router = APIRouter(prefix="/v1/catalog", tags=["catalog"])


# ── Request / response models ───────────────────────────────────

class ToolPoolRequest(BaseModel):
    session_id: str
    always_load: list[str] | None = None
    never_load: list[str] | None = None


class SearchRequest(BaseModel):
    query: str
    session_id: str | None = None
    limit: int = 20


# ── Endpoints ───────────────────────────────────────────────────

@router.get("")
async def list_tools(
    category: str | None = None,
    source: str | None = None,
    limit: int = 200,
    offset: int = 0,
) -> dict:
    tools = await repository.list_tools(
        category=category, source=source, limit=limit, offset=offset
    )
    return {"tools": [t.model_dump(mode="json") for t in tools]}


@router.get("/{name}")
async def get_tool(name: str) -> dict:
    tool = await repository.get_tool(name)
    if tool is None:
        raise HTTPException(404, "Tool not found")
    return tool.model_dump(mode="json")


@router.put("/{name}")
async def upsert_tool(name: str, body: ToolDescriptor) -> dict:
    tool = body.model_copy(update={"name": name})
    saved = await repository.upsert_tool(tool)
    return saved.model_dump(mode="json")


@router.delete("/{name}", status_code=204)
async def delete_tool(name: str) -> None:
    ok = await repository.delete_tool(name)
    if not ok:
        raise HTTPException(404, "Tool not found")


@router.post("/pool")
async def build_pool(req: ToolPoolRequest) -> dict:
    tools = await repository.list_tools()
    pool = pool_mod.build_pool(
        session_id=req.session_id,
        all_tools=tools,
        always_load=set(req.always_load or []),
        never_load=set(req.never_load or []),
    )
    return pool.model_dump(mode="json")


@router.post("/search")
async def search_tools(req: SearchRequest) -> dict:
    results = await search_mod.search(
        req.query, session_id=req.session_id, limit=req.limit
    )
    return {"tools": [t.model_dump(mode="json") for t in results]}
