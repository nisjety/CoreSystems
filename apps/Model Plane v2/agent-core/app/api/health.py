"""Health, readiness, and self-diagnostic probes.

Enhanced beyond basic health checks to provide CC-level self-diagnostic
capabilities: circuit breaker states, event store stats, recovery queue
depth, active runs, and subsystem health.
"""

from __future__ import annotations

import time

from fastapi import APIRouter

from app.config import settings

router = APIRouter(tags=["health"])

_STARTUP_TIME = time.time()


@router.get("/health")
async def health() -> dict:
    return {
        "status": "ok",
        "service": settings.service_name,
        "version": settings.service_version,
        "uptime_seconds": int(time.time() - _STARTUP_TIME),
    }


@router.get("/ready")
async def readiness() -> dict:
    """Check downstream dependencies (best-effort)."""
    from app.database import get_pool
    from app.redis_client import get_redis

    checks: dict[str, str] = {}

    # Postgres
    try:
        pool = await get_pool()
        async with pool.acquire() as conn:
            await conn.fetchval("SELECT 1")
        checks["postgres"] = "ok"
    except Exception:
        checks["postgres"] = "error"

    # Redis
    try:
        r = await get_redis()
        await r.ping()
        checks["redis"] = "ok"
    except Exception:
        checks["redis"] = "error"

    all_ok = all(v == "ok" for v in checks.values())
    return {"status": "ok" if all_ok else "degraded", "checks": checks}


@router.get("/diagnostics")
async def diagnostics() -> dict:
    """Self-diagnostic endpoint — comprehensive subsystem health.

    Exposes: circuit breaker states, event store stats, active runs,
    recovery queue depth, MCP server states, and Letta connectivity.
    """
    diag: dict[str, object] = {
        "service": settings.service_name,
        "version": settings.service_version,
        "uptime_seconds": int(time.time() - _STARTUP_TIME),
    }

    # Circuit breaker states (from MCP client)
    try:
        from app.mcp.client import _server_breakers
        breaker_states = {
            name: {
                "state": cb.state.value if hasattr(cb.state, "value") else str(cb.state),
                "failure_count": cb.failure_count,
            }
            for name, cb in _server_breakers.items()
        }
        diag["circuit_breakers"] = breaker_states
    except Exception:
        diag["circuit_breakers"] = "unavailable"

    # Event store stats
    try:
        from app.database import get_pool

        pool = await get_pool()
        async with pool.acquire() as conn:
            event_count = await conn.fetchval("SELECT COUNT(*) FROM run_events")
            snapshot_count = await conn.fetchval("SELECT COUNT(*) FROM run_snapshots")
        diag["event_store"] = {
            "total_events": event_count,
            "total_snapshots": snapshot_count,
        }
    except Exception:
        diag["event_store"] = "unavailable"

    # Active runs
    try:
        from app.database import get_pool

        pool = await get_pool()
        async with pool.acquire() as conn:
            active = await conn.fetchval(
                "SELECT COUNT(*) FROM agent_runs WHERE status = 'running'"
            )
            queued = await conn.fetchval(
                "SELECT COUNT(*) FROM agent_runs WHERE status = 'queued'"
            )
        diag["runs"] = {"active": active, "queued": queued}
    except Exception:
        diag["runs"] = "unavailable"

    # Recovery queue depth
    try:
        from app.redis_client import get_redis

        r = await get_redis()
        stale_count = await r.llen("recovery:stale_runs")
        diag["recovery_queue"] = {"stale_runs": stale_count}
    except Exception:
        diag["recovery_queue"] = "unavailable"

    # Active event streams
    try:
        from app.messages.streaming import _streams
        diag["event_streams"] = {"active": len(_streams)}
    except Exception:
        diag["event_streams"] = "unavailable"

    # NATS connectivity
    try:
        from app.main import nats_mgr
        if nats_mgr:
            diag["nats"] = "connected" if nats_mgr.is_connected else "disconnected"
        else:
            diag["nats"] = "not_initialized"
    except Exception:
        diag["nats"] = "unavailable"

    # Letta connectivity
    try:
        from app.config import settings as cfg
        if cfg.letta_enabled:
            from app.letta.memory_bridge import get_memory_bridge
            bridge = get_memory_bridge()
            letta_ok = await bridge.initialize()
            diag["letta"] = "connected" if letta_ok else "disabled_or_unreachable"
        else:
            diag["letta"] = "disabled"
    except Exception:
        diag["letta"] = "unavailable"

    return diag
