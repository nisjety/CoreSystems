"""asyncpg pool + migration runner for cost-core v2."""
from __future__ import annotations

import glob
import logging
import os
from typing import Optional

import asyncpg

from app.config import get_settings

logger = logging.getLogger(__name__)

_pool: Optional[asyncpg.Pool] = None


async def get_pool() -> asyncpg.Pool:
    global _pool
    if _pool is None:
        settings = get_settings()
        _pool = await asyncpg.create_pool(
            dsn=settings.postgres_dsn,
            min_size=2,
            max_size=10,
            command_timeout=30,
        )
        logger.info("asyncpg pool created", extra={"db": settings.postgres_db})
    return _pool


async def close_pool() -> None:
    global _pool
    if _pool is not None:
        await _pool.close()
        _pool = None


async def run_migrations(migrations_dir: str = "migrations") -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            CREATE TABLE IF NOT EXISTS schema_migrations (
                filename TEXT PRIMARY KEY,
                applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            """
        )
        pattern = os.path.join(migrations_dir, "*.up.sql")
        files = sorted(glob.glob(pattern))
        for path in files:
            name = os.path.basename(path)
            row = await conn.fetchrow(
                "SELECT 1 FROM schema_migrations WHERE filename = $1",
                name,
            )
            if row:
                continue
            with open(path, "r", encoding="utf-8") as fh:
                sql = fh.read()
            logger.info("applying migration", extra={"file": name})
            async with conn.transaction():
                await conn.execute(sql)
                await conn.execute(
                    "INSERT INTO schema_migrations (filename) VALUES ($1)",
                    name,
                )
