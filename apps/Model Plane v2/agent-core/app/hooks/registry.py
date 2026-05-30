"""Hook registry — load, cache, and match hooks by tool name pattern.

Supports wildcard matching compatible with Claude Code hook patterns:
  - "*"         → matches all tools
  - "bash:*"    → matches any tool starting with "bash:"
  - "mcp:*"     → matches any MCP tool
  - "exact_name" → matches only that tool
"""

from __future__ import annotations

import fnmatch
import logging
from functools import lru_cache
from typing import Any

import asyncpg

from app.database import get_pool
from app.hooks.domain import HookConfig, HookType

logger = logging.getLogger(__name__)

# In-memory cache: org_id → list[HookConfig].  Invalidated on mutation.
_cache: dict[str, list[HookConfig]] = {}


async def load_hooks_for_org(org_id: str) -> list[HookConfig]:
    """Load all enabled hooks for an org, using cache when available."""
    if org_id in _cache:
        return _cache[org_id]

    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, org_id, tool_name_pattern, hook_type, action,
                   reason, modify_input, modify_output, priority, enabled,
                   created_at, updated_at
            FROM hook_configs
            WHERE org_id = $1 AND enabled = true
            ORDER BY priority DESC, created_at ASC
            """,
            org_id,
        )

    hooks = [_row_to_hook(r) for r in rows]
    _cache[org_id] = hooks
    return hooks


def invalidate_cache(org_id: str) -> None:
    """Remove cached hooks for an org (call after create/update/delete)."""
    _cache.pop(org_id, None)


def invalidate_all() -> None:
    """Clear the entire hook cache."""
    _cache.clear()


def match_hooks(
    hooks: list[HookConfig],
    tool_name: str,
    hook_type: HookType,
) -> list[HookConfig]:
    """Return hooks matching tool_name pattern and hook_type, sorted by priority desc."""
    matched = [
        h
        for h in hooks
        if h.hook_type == hook_type and _pattern_matches(h.tool_name_pattern, tool_name)
    ]
    return sorted(matched, key=lambda h: h.priority, reverse=True)


def _pattern_matches(pattern: str, tool_name: str) -> bool:
    """Check if a hook's tool_name_pattern matches the given tool name.

    Uses fnmatch for glob-style matching.
    """
    return fnmatch.fnmatch(tool_name, pattern)


def _row_to_hook(row: asyncpg.Record) -> HookConfig:
    """Convert a DB row to a HookConfig."""
    import json

    return HookConfig(
        id=str(row["id"]),
        org_id=str(row["org_id"]),
        tool_name_pattern=row["tool_name_pattern"],
        hook_type=HookType(row["hook_type"]),
        action=row["action"],
        reason=row["reason"] or "",
        modify_input=json.loads(row["modify_input"]) if row["modify_input"] else None,
        modify_output=json.loads(row["modify_output"]) if row["modify_output"] else None,
        priority=row["priority"],
        enabled=row["enabled"],
        created_at=row["created_at"],
        updated_at=row["updated_at"],
    )
