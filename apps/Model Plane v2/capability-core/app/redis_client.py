"""Redis client for capability-core.

DB 4: budget counters, tool search cache, provider health cache, rate limits.
"""

from __future__ import annotations

import json
import logging
from typing import Any

import redis.asyncio as aioredis

from app.config import settings

logger = logging.getLogger(__name__)

_redis: aioredis.Redis | None = None


async def get_redis() -> aioredis.Redis:
    global _redis
    if _redis is None:
        _redis = aioredis.from_url(
            settings.redis_url,
            max_connections=settings.redis_max_connections,
            socket_timeout=settings.redis_socket_timeout,
            decode_responses=True,
        )
        await _redis.ping()
        logger.info("redis_connected", extra={"url": settings.redis_url})
    return _redis


async def close_redis() -> None:
    global _redis
    if _redis is not None:
        await _redis.aclose()
        _redis = None
        logger.info("redis_closed")


async def init_redis() -> None:
    """Eagerly initialize the Redis connection."""
    await get_redis()


# ── Budget keys ──────────────────────────────────────────────────

def _daily_key(org_id: str) -> str:
    from datetime import date
    return f"cap:budget:daily:{org_id}:{date.today().isoformat()}"


def _monthly_key(org_id: str) -> str:
    from datetime import date
    d = date.today()
    return f"cap:budget:monthly:{org_id}:{d.year}-{d.month:02d}"


async def record_usage(org_id: str, session_id: str, cost_nok: float) -> None:
    r = await get_redis()
    pipe = r.pipeline(transaction=False)
    dk = _daily_key(org_id)
    mk = _monthly_key(org_id)
    pipe.incrbyfloat(dk, cost_nok)
    pipe.expire(dk, 86_400 * 2)
    pipe.incrbyfloat(mk, cost_nok)
    pipe.expire(mk, 86_400 * 35)
    if session_id:
        sk = f"cap:budget:session:{session_id}"
        pipe.incrbyfloat(sk, cost_nok)
        pipe.expire(sk, 86_400)
    await pipe.execute()


async def check_budget(org_id: str, daily_limit: float, monthly_limit: float) -> dict[str, Any]:
    r = await get_redis()
    pipe = r.pipeline(transaction=False)
    pipe.get(_daily_key(org_id))
    pipe.get(_monthly_key(org_id))
    results = await pipe.execute()
    daily_used = float(results[0] or 0)
    monthly_used = float(results[1] or 0)

    allowed = True
    reason = ""
    if daily_limit > 0 and daily_used >= daily_limit:
        allowed = False
        reason = f"daily budget exceeded: {daily_used:.2f}/{daily_limit:.2f} NOK"
    elif monthly_limit > 0 and monthly_used >= monthly_limit:
        allowed = False
        reason = f"monthly budget exceeded: {monthly_used:.2f}/{monthly_limit:.2f} NOK"

    return {
        "allowed": allowed,
        "daily_used_nok": daily_used,
        "monthly_used_nok": monthly_used,
        "daily_limit_nok": daily_limit,
        "monthly_limit_nok": monthly_limit,
        "reason": reason,
    }


# ── Tool search cache ────────────────────────────────────────────

async def get_cached_search(session_id: str, query: str) -> list[str] | None:
    r = await get_redis()
    key = f"cap:toolsearch:{session_id}:{query}"
    raw = await r.get(key)
    if raw:
        return json.loads(raw)
    return None


async def set_cached_search(
    session_id: str, query: str, tool_names: list[str], ttl: int | None = None
) -> None:
    r = await get_redis()
    key = f"cap:toolsearch:{session_id}:{query}"
    await r.set(key, json.dumps(tool_names), ex=ttl or settings.tool_search_cache_ttl)


# ── Provider health cache ────────────────────────────────────────

async def get_provider_health(provider: str) -> dict[str, Any] | None:
    r = await get_redis()
    raw = await r.get(f"cap:provhealth:{provider}")
    if raw:
        return json.loads(raw)
    return None


async def set_provider_health(provider: str, data: dict[str, Any]) -> None:
    r = await get_redis()
    await r.set(
        f"cap:provhealth:{provider}",
        json.dumps(data),
        ex=settings.provider_health_cache_ttl,
    )
