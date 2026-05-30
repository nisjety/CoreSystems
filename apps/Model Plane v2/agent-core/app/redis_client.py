"""Redis client — session cache, worker lease management, idempotency."""

from __future__ import annotations

import logging
import time
from typing import Any

from redis.asyncio import Redis

from app.config import settings

logger = logging.getLogger(__name__)

_redis: Redis | None = None


async def get_redis() -> Redis:
    """Return the global Redis client, creating it on first call."""
    global _redis
    if _redis is None:
        _redis = Redis.from_url(
            settings.redis_url,
            encoding="utf-8",
            decode_responses=True,
            max_connections=settings.redis_max_connections,
            socket_timeout=settings.redis_socket_timeout,
            socket_connect_timeout=5,
            socket_keepalive=True,
        )
        await _redis.ping()
        logger.info("redis_connected")
    return _redis


async def close_redis() -> None:
    global _redis
    if _redis is not None:
        await _redis.aclose()
        _redis = None


# ---------------------------------------------------------------------------
# Lease management (coordinator/worker model)
# ---------------------------------------------------------------------------

LEASE_PREFIX = "agent-core-v2:lease:"


async def acquire_lease(run_id: str, owner: str, ttl: int | None = None) -> bool:
    """Try to acquire an exclusive lease on a run.

    Returns True if acquired, False if already held by another owner.
    Uses SET NX with TTL for distributed mutual exclusion.
    """
    ttl = ttl or settings.lease_ttl_seconds
    r = await get_redis()
    key = f"{LEASE_PREFIX}{run_id}"
    acquired = await r.set(key, owner, nx=True, ex=ttl)
    if acquired:
        logger.debug("lease_acquired", extra={"run_id": run_id, "owner": owner})
    return bool(acquired)


async def renew_lease(run_id: str, owner: str, ttl: int | None = None) -> bool:
    """Renew a lease only if we still hold it (compare-and-refresh)."""
    ttl = ttl or settings.lease_ttl_seconds
    r = await get_redis()
    key = f"{LEASE_PREFIX}{run_id}"
    # Lua script: if key value matches owner, refresh expiry
    script = """
    if redis.call('get', KEYS[1]) == ARGV[1] then
        redis.call('expire', KEYS[1], ARGV[2])
        return 1
    end
    return 0
    """
    result = await r.eval(script, 1, key, owner, str(ttl))
    return result == 1


async def release_lease(run_id: str, owner: str) -> bool:
    """Release a lease only if we hold it."""
    r = await get_redis()
    key = f"{LEASE_PREFIX}{run_id}"
    script = """
    if redis.call('get', KEYS[1]) == ARGV[1] then
        redis.call('del', KEYS[1])
        return 1
    end
    return 0
    """
    result = await r.eval(script, 1, key, owner)
    if result == 1:
        logger.debug("lease_released", extra={"run_id": run_id, "owner": owner})
    return result == 1


async def get_lease_owner(run_id: str) -> str | None:
    """Check who holds the lease for a run."""
    r = await get_redis()
    return await r.get(f"{LEASE_PREFIX}{run_id}")


# ---------------------------------------------------------------------------
# Idempotency
# ---------------------------------------------------------------------------

IDEMPOTENCY_PREFIX = "agent-core-v2:idem:"
IDEMPOTENCY_TTL = 3600  # 1 hour


async def check_idempotency(key: str) -> bool:
    """Return True if this key was already processed (duplicate)."""
    r = await get_redis()
    full_key = f"{IDEMPOTENCY_PREFIX}{key}"
    was_set = await r.set(full_key, "1", nx=True, ex=IDEMPOTENCY_TTL)
    return not was_set  # True if NOT newly set → duplicate


# ---------------------------------------------------------------------------
# Run state cache (hot path supplement to Postgres)
# ---------------------------------------------------------------------------

CACHE_PREFIX = "agent-core-v2:run-cache:"
CACHE_TTL = 300  # 5 min


async def cache_run(run_id: str, data: dict[str, Any]) -> None:
    """Cache a run state snapshot for fast reads."""
    import json

    r = await get_redis()
    await r.set(f"{CACHE_PREFIX}{run_id}", json.dumps(data, default=str), ex=CACHE_TTL)


async def get_cached_run(run_id: str) -> dict[str, Any] | None:
    """Retrieve cached run state, or None if miss."""
    import json

    r = await get_redis()
    raw = await r.get(f"{CACHE_PREFIX}{run_id}")
    if raw:
        return json.loads(raw)
    return None


async def invalidate_run_cache(run_id: str) -> None:
    r = await get_redis()
    await r.delete(f"{CACHE_PREFIX}{run_id}")
