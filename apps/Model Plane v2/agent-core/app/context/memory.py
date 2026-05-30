"""Agent memory loader — persistent memory files per org/session.

Implements the CC pattern of injecting memory context (from .claude/memories/)
into every run's system prompt. Reads from the agent_memory Postgres table.
"""

from __future__ import annotations

import logging
from typing import Any

from app.database import get_pool

logger = logging.getLogger(__name__)


async def load_memory_files(
    org_id: str | None,
    session_id: str | None = None,
) -> list[str]:
    """Load memory snippets for an org, optionally filtered to a session.

    Returns snippets in order: org-level first, then session-scoped.
    """
    if not org_id:
        return []

    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT key, content
            FROM agent_memory
            WHERE org_id = $1
              AND (session_id IS NULL OR session_id = $2)
            ORDER BY
                CASE WHEN session_id IS NULL THEN 0 ELSE 1 END,
                created_at ASC
            """,
            org_id,
            session_id or "",
        )

    snippets: list[str] = []
    for row in rows:
        key = row["key"]
        content = row["content"]
        if content:
            snippets.append(f"## {key}\n{content}")

    return snippets
