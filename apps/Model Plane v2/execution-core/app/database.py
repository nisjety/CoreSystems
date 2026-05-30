"""asyncpg database connection pool and migration runner."""

from __future__ import annotations

import logging
from pathlib import Path

import asyncpg

from app.config import settings

logger = logging.getLogger(__name__)

MIGRATIONS_DIR = Path(__file__).resolve().parent.parent / "migrations"

_pool: asyncpg.Pool | None = None


async def get_pool() -> asyncpg.Pool:
    """Return the global connection pool, creating it on first call."""
    global _pool
    if _pool is None:
        _pool = await asyncpg.create_pool(
            dsn=settings.dsn,
            min_size=settings.postgres_pool_min,
            max_size=settings.postgres_pool_max,
            command_timeout=30,
        )
        logger.info("database_pool_created", extra={"dsn_host": settings.postgres_host})
    return _pool


async def close_pool() -> None:
    """Drain and close the pool."""
    global _pool
    if _pool is not None:
        await _pool.close()
        _pool = None
        logger.info("database_pool_closed")


async def run_migrations() -> None:
    """Apply pending SQL migrations in order."""
    pool = await get_pool()

    async with pool.acquire() as conn:
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
            version = int(mf.name.split("_")[0])
            if version in applied:
                continue

            sql = mf.read_text(encoding="utf-8")
            async with conn.transaction():
                await conn.execute(sql)
                await conn.execute(
                    "INSERT INTO schema_migrations (version) VALUES ($1)", version
                )
            logger.info("migration_applied", extra={"version": version, "file": mf.name})
