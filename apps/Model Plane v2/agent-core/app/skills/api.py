"""Skills CRUD API — /api/v1/skills.

Manage agent skill definitions per org.
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
from app.skills.loader import invalidate_cache

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/skills", tags=["skills"])


# ---- Request / response schemas ----


class CreateSkillRequest(BaseModel):
    name: str = Field(..., min_length=1, max_length=128)
    description: str = Field(..., min_length=1, max_length=2000)
    content: str = Field(..., min_length=1, max_length=100_000)
    trigger_keywords: list[str] = Field(default_factory=list)
    trigger_file_patterns: list[str] = Field(default_factory=list)
    tool_restrictions: list[str] = Field(default_factory=list)
    enabled: bool = True


class UpdateSkillRequest(BaseModel):
    name: str | None = None
    description: str | None = None
    content: str | None = None
    trigger_keywords: list[str] | None = None
    trigger_file_patterns: list[str] | None = None
    tool_restrictions: list[str] | None = None
    enabled: bool | None = None


class SkillResponse(BaseModel):
    id: str
    org_id: str
    name: str
    description: str
    content: str
    trigger_keywords: list[str]
    trigger_file_patterns: list[str]
    tool_restrictions: list[str]
    enabled: bool
    created_at: datetime
    updated_at: datetime


# ---- Helpers ----


def _org_id(request: Request) -> str:
    org_id = getattr(request.state, "org_id", None)
    if not org_id:
        raise HTTPException(status_code=401, detail="org_id required")
    return org_id


# ---- Endpoints ----


@router.get("", response_model=list[SkillResponse])
async def list_skills(request: Request) -> list[SkillResponse]:
    org_id = _org_id(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            "SELECT * FROM agent_skills WHERE org_id = $1 ORDER BY name", org_id
        )
    return [_row_to_response(r) for r in rows]


@router.post("", response_model=SkillResponse, status_code=201)
async def create_skill(request: Request, body: CreateSkillRequest) -> SkillResponse:
    org_id = _org_id(request)
    skill_id = str(uuid4())
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO agent_skills
                (id, org_id, name, description, content,
                 trigger_keywords, trigger_file_patterns, tool_restrictions,
                 enabled, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            """,
            skill_id,
            org_id,
            body.name,
            body.description,
            body.content,
            json.dumps(body.trigger_keywords),
            json.dumps(body.trigger_file_patterns),
            json.dumps(body.tool_restrictions),
            body.enabled,
            now,
            now,
        )

    invalidate_cache(org_id)
    logger.info("skill_created", extra={"skill_id": skill_id, "skill_name": body.name})

    return SkillResponse(
        id=skill_id,
        org_id=org_id,
        name=body.name,
        description=body.description,
        content=body.content,
        trigger_keywords=body.trigger_keywords,
        trigger_file_patterns=body.trigger_file_patterns,
        tool_restrictions=body.tool_restrictions,
        enabled=body.enabled,
        created_at=now,
        updated_at=now,
    )


@router.patch("/{skill_id}", response_model=SkillResponse)
async def update_skill(
    request: Request, skill_id: str, body: UpdateSkillRequest
) -> SkillResponse:
    org_id = _org_id(request)
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        existing = await conn.fetchrow(
            "SELECT * FROM agent_skills WHERE id = $1 AND org_id = $2",
            skill_id,
            org_id,
        )
    if not existing:
        raise HTTPException(status_code=404, detail="Skill not found")

    # Build SET clause dynamically
    updates: dict[str, Any] = {"updated_at": now}
    if body.name is not None:
        updates["name"] = body.name
    if body.description is not None:
        updates["description"] = body.description
    if body.content is not None:
        updates["content"] = body.content
    if body.trigger_keywords is not None:
        updates["trigger_keywords"] = json.dumps(body.trigger_keywords)
    if body.trigger_file_patterns is not None:
        updates["trigger_file_patterns"] = json.dumps(body.trigger_file_patterns)
    if body.tool_restrictions is not None:
        updates["tool_restrictions"] = json.dumps(body.tool_restrictions)
    if body.enabled is not None:
        updates["enabled"] = body.enabled

    set_parts = [f"{k} = ${i+1}" for i, k in enumerate(updates.keys())]
    values = list(updates.values())
    values.append(skill_id)
    values.append(org_id)

    async with pool.acquire() as conn:
        await conn.execute(
            f"UPDATE agent_skills SET {', '.join(set_parts)} WHERE id = ${len(values)-1} AND org_id = ${len(values)}",
            *values,
        )
        row = await conn.fetchrow(
            "SELECT * FROM agent_skills WHERE id = $1", skill_id
        )

    invalidate_cache(org_id)
    return _row_to_response(row)


@router.delete("/{skill_id}", status_code=204)
async def delete_skill(request: Request, skill_id: str) -> None:
    org_id = _org_id(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute(
            "DELETE FROM agent_skills WHERE id = $1 AND org_id = $2",
            skill_id,
            org_id,
        )
    if result == "DELETE 0":
        raise HTTPException(status_code=404, detail="Skill not found")

    invalidate_cache(org_id)
    logger.info("skill_deleted", extra={"id": skill_id})


def _row_to_response(row: Any) -> SkillResponse:
    return SkillResponse(
        id=str(row["id"]),
        org_id=str(row["org_id"]),
        name=row["name"],
        description=row["description"],
        content=row["content"],
        trigger_keywords=json.loads(row["trigger_keywords"]) if row["trigger_keywords"] else [],
        trigger_file_patterns=json.loads(row["trigger_file_patterns"]) if row["trigger_file_patterns"] else [],
        tool_restrictions=json.loads(row["tool_restrictions"]) if row["tool_restrictions"] else [],
        enabled=row["enabled"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )
