"""asyncpg database connection pool and migration runner."""

from __future__ import annotations

import asyncio
import logging
import socket
from pathlib import Path

import asyncpg

from app.config import settings

logger = logging.getLogger(__name__)

MIGRATIONS_DIR = Path(__file__).resolve().parent.parent / "migrations"

_pool: asyncpg.Pool | None = None

# U2-18 (velion ui-ux-velion-gap.md §10): retry on first-boot races.
# When agent-core-v2 comes up alongside reasoning-v2-postgres via
# `docker compose up`, the postgres container can still be in its
# init script when asyncpg tries the first connect. DNS resolves but
# the listener isn't ready yet — surfaces as `gaierror` or
# `ConnectionRefusedError`. Retry for up to ~60 seconds so the
# FastAPI lifespan doesn't bail.
_POOL_CONNECT_MAX_ATTEMPTS = 30
_POOL_CONNECT_BACKOFF_SECONDS = 2.0


async def get_pool() -> asyncpg.Pool:
    """Return the global connection pool, creating it on first call.

    Retries transient connect failures (DNS races, port not yet open)
    on cold-start. See U2-18 in ui-ux-velion-gap.md.
    """
    global _pool
    if _pool is not None:
        return _pool

    last_error: BaseException | None = None
    for attempt in range(1, _POOL_CONNECT_MAX_ATTEMPTS + 1):
        try:
            _pool = await asyncpg.create_pool(
                dsn=settings.dsn,
                min_size=settings.postgres_pool_min,
                max_size=settings.postgres_pool_max,
                command_timeout=30,
            )
            logger.info(
                "database_pool_created",
                extra={"dsn_host": settings.postgres_host, "attempt": attempt},
            )
            return _pool
        except (
            asyncpg.exceptions.CannotConnectNowError,
            asyncpg.exceptions.PostgresConnectionError,
            ConnectionRefusedError,
            socket.gaierror,
            OSError,
        ) as exc:
            last_error = exc
            if attempt >= _POOL_CONNECT_MAX_ATTEMPTS:
                break
            logger.warning(
                "database_pool_connect_retry",
                extra={
                    "attempt": attempt,
                    "max_attempts": _POOL_CONNECT_MAX_ATTEMPTS,
                    "dsn_host": settings.postgres_host,
                    "error": str(exc),
                },
            )
            await asyncio.sleep(_POOL_CONNECT_BACKOFF_SECONDS)

    # Exhausted retries — re-raise so the lifespan terminates with a clear
    # message. The wrapper preserves the original exception for telemetry.
    raise RuntimeError(
        f"database_pool_create_failed after {_POOL_CONNECT_MAX_ATTEMPTS} attempts: {last_error!r}"
    )


async def close_pool() -> None:
    """Drain and close the pool."""
    global _pool
    if _pool is not None:
        await _pool.close()
        _pool = None
        logger.info("database_pool_closed")


async def run_migrations() -> None:
    """Apply pending SQL migrations in order.

    Uses a schema_migrations table to track which versions have been applied.
    Each migration file is expected to be named NNN_<desc>.up.sql where NNN
    is a zero-padded integer version.
    """
    pool = await get_pool()

    async with pool.acquire() as conn:
        # Ensure the tracking table exists (idempotent)
        await conn.execute("""
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version  INT PRIMARY KEY,
                applied  TIMESTAMPTZ NOT NULL DEFAULT now()
            )
        """)

        applied: set[int] = {
            row["version"]
            for row in await conn.fetch("SELECT version FROM schema_migrations")
        }

        migration_files = sorted(MIGRATIONS_DIR.glob("*.up.sql"))
        for mf in migration_files:
            version = int(mf.name.split("_", 1)[0])
            if version in applied:
                continue

            sql = mf.read_text(encoding="utf-8")
            async with conn.transaction():
                await conn.execute(sql)
                await conn.execute(
                    "INSERT INTO schema_migrations (version) VALUES ($1)", version
                )
            logger.info("migration_applied", extra={"version": version, "file": mf.name})
