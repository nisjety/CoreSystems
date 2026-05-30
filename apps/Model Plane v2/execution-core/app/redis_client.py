"""Redis client — runner lease management and heartbeat tracking."""

from __future__ import annotations

import logging
import time

from redis.asyncio import Redis

from app.config import settings

logger = logging.getLogger(__name__)

_redis: Redis | None = None

LEASE_PREFIX = "execution-core:lease:"
HEARTBEAT_PREFIX = "execution-core:hb:"


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
# Runner lease management (TTL-based, agent-core monitors)
# ---------------------------------------------------------------------------


async def acquire_task_lease(
    task_id: str,
    runner_id: str,
    ttl: int | None = None,
) -> bool:
    """Try to acquire an exclusive lease on a task.

    Uses SET NX with TTL for distributed mutual exclusion.
    Returns True if acquired, False if already held by another runner.
    """
    ttl = ttl or settings.runner_lease_ttl
    r = await get_redis()
    key = f"{LEASE_PREFIX}{task_id}"
    acquired = await r.set(key, runner_id, nx=True, ex=ttl)
    if acquired:
        logger.info("task_lease_acquired", extra={"task_id": task_id, "runner_id": runner_id})
    return bool(acquired)


async def renew_task_lease(task_id: str, runner_id: str, ttl: int | None = None) -> bool:
    """Renew a lease only if the current holder matches runner_id."""
    ttl = ttl or settings.runner_lease_ttl
    r = await get_redis()
    key = f"{LEASE_PREFIX}{task_id}"
    current = await r.get(key)
    if current != runner_id:
        return False
    await r.expire(key, ttl)
    return True


async def release_task_lease(task_id: str, runner_id: str) -> bool:
    """Release a lease only if the current holder matches runner_id.

    Uses a Lua script for atomicity (check-and-delete).
    """
    r = await get_redis()
    key = f"{LEASE_PREFIX}{task_id}"
    lua = """
    if redis.call("GET", KEYS[1]) == ARGV[1] then
        return redis.call("DEL", KEYS[1])
    else
        return 0
    end
    """
    released = await r.eval(lua, 1, key, runner_id)
    if released:
        logger.info("task_lease_released", extra={"task_id": task_id, "runner_id": runner_id})
    return bool(released)


async def get_lease_holder(task_id: str) -> str | None:
    """Return the current lease holder for a task, or None."""
    r = await get_redis()
    return await r.get(f"{LEASE_PREFIX}{task_id}")


# ---------------------------------------------------------------------------
# Runner heartbeat tracking
# ---------------------------------------------------------------------------


async def record_heartbeat(runner_id: str, ttl: int | None = None) -> None:
    """Record a heartbeat for a runner.

    TTL is set to 2x the heartbeat interval (so missed heartbeats expire).
    """
    ttl = ttl or (settings.runner_heartbeat_interval * 2)
    r = await get_redis()
    key = f"{HEARTBEAT_PREFIX}{runner_id}"
    await r.set(key, str(int(time.time())), ex=ttl)


async def is_runner_alive(runner_id: str) -> bool:
    """Check if a runner has a recent heartbeat."""
    r = await get_redis()
    return bool(await r.exists(f"{HEARTBEAT_PREFIX}{runner_id}"))


async def get_last_heartbeat_ts(runner_id: str) -> int | None:
    """Return the unix timestamp of the last heartbeat, or None."""
    r = await get_redis()
    val = await r.get(f"{HEARTBEAT_PREFIX}{runner_id}")
    return int(val) if val else None
