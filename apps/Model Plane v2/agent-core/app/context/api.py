"""Agent memory CRUD API — /api/v1/memory.

Allows orgs to manage persistent memory snippets that are injected
into every agent run's system prompt.
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel, Field

from app.database import get_pool
from app.context.injector import invalidate_prompt_cache

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/memory", tags=["memory"])


# ---- Request / response schemas ----


class CreateMemoryRequest(BaseModel):
    key: str = Field(..., min_length=1, max_length=256)
    content: str = Field(..., min_length=1, max_length=50_000)
    session_id: str | None = None  # None = org-level, set = session-scoped


class UpdateMemoryRequest(BaseModel):
    content: str = Field(..., min_length=1, max_length=50_000)


class MemoryResponse(BaseModel):
    id: str
    org_id: str
    session_id: str | None
    key: str
    content: str
    created_at: datetime
    updated_at: datetime


# ---- Helpers ----


def _org_id_from_request(request: Request) -> str:
    org_id = getattr(request.state, "org_id", None)
    if not org_id:
        raise HTTPException(status_code=401, detail="org_id required")
    return org_id


# ---- Endpoints ----


@router.get("", response_model=list[MemoryResponse])
async def list_memories(
    request: Request, session_id: str | None = None
) -> list[MemoryResponse]:
    """List memory entries for the org."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        if session_id:
            rows = await conn.fetch(
                """
                SELECT * FROM agent_memory
                WHERE org_id = $1 AND (session_id IS NULL OR session_id = $2)
                ORDER BY created_at
                """,
                org_id,
                session_id,
            )
        else:
            rows = await conn.fetch(
                """
                SELECT * FROM agent_memory
                WHERE org_id = $1
                ORDER BY created_at
                """,
                org_id,
            )
    return [_row_to_response(r) for r in rows]


@router.post("", response_model=MemoryResponse, status_code=201)
async def create_memory(request: Request, body: CreateMemoryRequest) -> MemoryResponse:
    """Create a new memory entry."""
    org_id = _org_id_from_request(request)
    mem_id = str(uuid4())
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO agent_memory (id, org_id, session_id, key, content, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            """,
            mem_id,
            org_id,
            body.session_id,
            body.key,
            body.content,
            now,
            now,
        )

    invalidate_prompt_cache()
    logger.info("memory_created", extra={"id": mem_id, "key": body.key})

    return MemoryResponse(
        id=mem_id,
        org_id=org_id,
        session_id=body.session_id,
        key=body.key,
        content=body.content,
        created_at=now,
        updated_at=now,
    )


@router.patch("/{memory_id}", response_model=MemoryResponse)
async def update_memory(
    request: Request, memory_id: str, body: UpdateMemoryRequest
) -> MemoryResponse:
    """Update a memory entry's content."""
    org_id = _org_id_from_request(request)
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute(
            """
            UPDATE agent_memory SET content = $1, updated_at = $2
            WHERE id = $3 AND org_id = $4
            """,
            body.content,
            now,
            memory_id,
            org_id,
        )
    if result == "UPDATE 0":
        raise HTTPException(status_code=404, detail="Memory not found")

    invalidate_prompt_cache()

    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT * FROM agent_memory WHERE id = $1", memory_id
        )
    return _row_to_response(row)


@router.delete("/{memory_id}", status_code=204)
async def delete_memory(request: Request, memory_id: str) -> None:
    """Delete a memory entry."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute(
            "DELETE FROM agent_memory WHERE id = $1 AND org_id = $2",
            memory_id,
            org_id,
        )
    if result == "DELETE 0":
        raise HTTPException(status_code=404, detail="Memory not found")

    invalidate_prompt_cache()
    logger.info("memory_deleted", extra={"id": memory_id})


def _row_to_response(row: Any) -> MemoryResponse:
    return MemoryResponse(
        id=str(row["id"]),
        org_id=str(row["org_id"]),
        session_id=row["session_id"],
        key=row["key"],
        content=row["content"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )
