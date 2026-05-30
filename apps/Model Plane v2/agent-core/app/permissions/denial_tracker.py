"""Denial tracker — per-session tool denial tracking.

Mirrors CC's denialTracking.ts: remembers which tools a user has
denied in the current session to avoid re-prompting.

Uses Redis with session-scoped keys (auto-expire after 24h).
"""

from __future__ import annotations

import logging
from typing import Any

from app.permissions.domain import DenialRecord

logger = logging.getLogger(__name__)

# In-memory fallback when Redis is unavailable
_denial_cache: dict[str, dict[str, DenialRecord]] = {}

# Redis key prefix and TTL
DENIAL_KEY_PREFIX = "perm:denied:"
DENIAL_TTL_SECONDS = 86400  # 24 hours


async def record_denial(
    session_id: str,
    tool_name: str,
    reason: str = "",
) -> DenialRecord:
    """Record that a user denied a tool in this session."""
    cache_key = f"{session_id}:{tool_name}"

    existing = _denial_cache.get(session_id, {}).get(tool_name)
    if existing:
        record = DenialRecord(
            session_id=session_id,
            tool_name=tool_name,
            reason=reason or existing.reason,
            denial_count=existing.denial_count + 1,
        )
    else:
        record = DenialRecord(
            session_id=session_id,
            tool_name=tool_name,
            reason=reason,
        )

    if session_id not in _denial_cache:
        _denial_cache[session_id] = {}
    _denial_cache[session_id][tool_name] = record

    # Try Redis persistence
    try:
        from app.redis_client import get_redis

        redis = await get_redis()
        if redis:
            redis_key = f"{DENIAL_KEY_PREFIX}{session_id}"
            await redis.hset(redis_key, tool_name, record.model_dump_json())
            await redis.expire(redis_key, DENIAL_TTL_SECONDS)
    except Exception:
        pass  # Memory-only fallback is fine

    logger.info(
        "tool_denial_recorded",
        extra={
            "session_id": session_id,
            "tool": tool_name,
            "count": record.denial_count,
        },
    )
    return record


async def is_denied(session_id: str, tool_name: str) -> bool:
    """Check if a tool has been denied in this session."""
    if session_id in _denial_cache and tool_name in _denial_cache[session_id]:
        return True

    # Check Redis
    try:
        from app.redis_client import get_redis

        redis = await get_redis()
        if redis:
            redis_key = f"{DENIAL_KEY_PREFIX}{session_id}"
            result = await redis.hget(redis_key, tool_name)
            return result is not None
    except Exception:
        pass

    return False


async def get_session_denials(session_id: str) -> list[DenialRecord]:
    """Get all denied tools for a session."""
    records = list(_denial_cache.get(session_id, {}).values())
    return records


async def clear_session_denials(session_id: str) -> None:
    """Clear all denials for a session (e.g. on reset)."""
    _denial_cache.pop(session_id, None)

    try:
        from app.redis_client import get_redis

        redis = await get_redis()
        if redis:
            redis_key = f"{DENIAL_KEY_PREFIX}{session_id}"
            await redis.delete(redis_key)
    except Exception:
        pass
