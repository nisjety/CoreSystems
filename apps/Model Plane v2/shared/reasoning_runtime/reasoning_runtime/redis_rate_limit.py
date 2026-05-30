"""Redis rate limiting — sliding-window RPM check per org.

BUG FIX: Renamed from ``init_redis`` / ``close_redis`` to ``init`` / ``close``
to match the caller convention used in main.py lifespan.
"""

from __future__ import annotations

import logging
import time

import redis.asyncio as redis

from reasoning_runtime.config import get_config

logger = logging.getLogger(__name__)

_pool: redis.Redis | None = None


async def init() -> None:
    """Connect to Redis using the runtime config URL."""
    global _pool
    cfg = get_config()
    _pool = redis.from_url(cfg.redis_url, decode_responses=True)
    logger.info("redis connected (rate limiting)")


async def close() -> None:
    """Gracefully close the Redis pool."""
    global _pool
    if _pool:
        await _pool.aclose()
        _pool = None


def _r() -> redis.Redis:
    if _pool is None:
        raise RuntimeError("redis not initialised — call init() first")
    return _pool


async def check_rate_limit(org_id: str) -> bool:
    """Sliding-window RPM check.  Returns ``True`` if within limit."""
    cfg = get_config()
    key = f"llm:rl:{org_id}:{int(time.time()) // 60}"
    r = _r()

    count = await r.incr(key)
    if count == 1:
        await r.expire(key, 120)  # 2-minute TTL for safety

    return count <= cfg.rate_limit_rpm
