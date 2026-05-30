"""Hook CRUD API — /api/v1/hooks.

Allows orgs to manage hook configurations that intercept tool calls
with approve/block/modify semantics.
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
from app.hooks.domain import HookAction, HookConfig, HookType
from app.hooks.registry import invalidate_cache

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/hooks", tags=["hooks"])


# ---- Request / response schemas ----


class CreateHookRequest(BaseModel):
    tool_name_pattern: str = Field(..., min_length=1, max_length=256)
    hook_type: HookType
    action: HookAction
    reason: str = ""
    modify_input: dict[str, Any] | None = None
    modify_output: dict[str, Any] | None = None
    priority: int = Field(default=0, ge=0, le=1000)
    enabled: bool = True


class UpdateHookRequest(BaseModel):
    tool_name_pattern: str | None = None
    hook_type: HookType | None = None
    action: HookAction | None = None
    reason: str | None = None
    modify_input: dict[str, Any] | None = None
    modify_output: dict[str, Any] | None = None
    priority: int | None = Field(default=None, ge=0, le=1000)
    enabled: bool | None = None


class HookResponse(BaseModel):
    id: str
    org_id: str
    tool_name_pattern: str
    hook_type: HookType
    action: HookAction
    reason: str
    modify_input: dict[str, Any] | None
    modify_output: dict[str, Any] | None
    priority: int
    enabled: bool
    created_at: datetime
    updated_at: datetime


# ---- Helpers ----


def _org_id_from_request(request: Request) -> str:
    org_id = getattr(request.state, "org_id", None)
    if not org_id:
        raise HTTPException(status_code=401, detail="org_id required")
    return org_id


# ---- Endpoints ----


@router.get("", response_model=list[HookResponse])
async def list_hooks(request: Request) -> list[HookResponse]:
    """List all hook configs for the org."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, org_id, tool_name_pattern, hook_type, action,
                   reason, modify_input, modify_output, priority, enabled,
                   created_at, updated_at
            FROM hook_configs
            WHERE org_id = $1
            ORDER BY priority DESC, created_at ASC
            """,
            org_id,
        )
    return [_row_to_response(r) for r in rows]


@router.post("", response_model=HookResponse, status_code=201)
async def create_hook(request: Request, body: CreateHookRequest) -> HookResponse:
    """Create a new hook config."""
    org_id = _org_id_from_request(request)
    hook_id = str(uuid4())
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO hook_configs (
                id, org_id, tool_name_pattern, hook_type, action,
                reason, modify_input, modify_output, priority, enabled,
                created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
            """,
            hook_id,
            org_id,
            body.tool_name_pattern,
            body.hook_type.value,
            body.action.value,
            body.reason,
            json.dumps(body.modify_input) if body.modify_input else None,
            json.dumps(body.modify_output) if body.modify_output else None,
            body.priority,
            body.enabled,
            now,
            now,
        )

    invalidate_cache(org_id)
    logger.info("hook_created", extra={"hook_id": hook_id, "org_id": org_id})

    return HookResponse(
        id=hook_id,
        org_id=org_id,
        tool_name_pattern=body.tool_name_pattern,
        hook_type=body.hook_type,
        action=body.action,
        reason=body.reason,
        modify_input=body.modify_input,
        modify_output=body.modify_output,
        priority=body.priority,
        enabled=body.enabled,
        created_at=now,
        updated_at=now,
    )


@router.patch("/{hook_id}", response_model=HookResponse)
async def update_hook(
    request: Request, hook_id: str, body: UpdateHookRequest
) -> HookResponse:
    """Update a hook config (partial)."""
    org_id = _org_id_from_request(request)
    now = datetime.now(timezone.utc)

    pool = await get_pool()
    async with pool.acquire() as conn:
        existing = await conn.fetchrow(
            "SELECT * FROM hook_configs WHERE id = $1 AND org_id = $2",
            hook_id,
            org_id,
        )
        if not existing:
            raise HTTPException(status_code=404, detail="Hook not found")

        updates: dict[str, Any] = {"updated_at": now}
        if body.tool_name_pattern is not None:
            updates["tool_name_pattern"] = body.tool_name_pattern
        if body.hook_type is not None:
            updates["hook_type"] = body.hook_type.value
        if body.action is not None:
            updates["action"] = body.action.value
        if body.reason is not None:
            updates["reason"] = body.reason
        if body.modify_input is not None:
            updates["modify_input"] = json.dumps(body.modify_input)
        if body.modify_output is not None:
            updates["modify_output"] = json.dumps(body.modify_output)
        if body.priority is not None:
            updates["priority"] = body.priority
        if body.enabled is not None:
            updates["enabled"] = body.enabled

        set_clauses = ", ".join(f"{k} = ${i+2}" for i, k in enumerate(updates))
        values = [hook_id, *updates.values()]
        await conn.execute(
            f"UPDATE hook_configs SET {set_clauses} WHERE id = $1",
            *values,
        )

    invalidate_cache(org_id)

    # Re-read to return updated record
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT * FROM hook_configs WHERE id = $1", hook_id
        )
    return _row_to_response(row)


@router.delete("/{hook_id}", status_code=204)
async def delete_hook(request: Request, hook_id: str) -> None:
    """Delete a hook config."""
    org_id = _org_id_from_request(request)
    pool = await get_pool()
    async with pool.acquire() as conn:
        result = await conn.execute(
            "DELETE FROM hook_configs WHERE id = $1 AND org_id = $2",
            hook_id,
            org_id,
        )
    if result == "DELETE 0":
        raise HTTPException(status_code=404, detail="Hook not found")

    invalidate_cache(org_id)
    logger.info("hook_deleted", extra={"hook_id": hook_id, "org_id": org_id})


def _row_to_response(row: Any) -> HookResponse:
    """Convert a DB row to HookResponse."""
    return HookResponse(
        id=str(row["id"]),
        org_id=str(row["org_id"]),
        tool_name_pattern=row["tool_name_pattern"],
        hook_type=HookType(row["hook_type"]),
        action=HookAction(row["action"]),
        reason=row["reason"] or "",
        modify_input=json.loads(row["modify_input"]) if row["modify_input"] else None,
        modify_output=json.loads(row["modify_output"]) if row["modify_output"] else None,
        priority=row["priority"],
        enabled=row["enabled"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )
