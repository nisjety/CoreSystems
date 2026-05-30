"""Tool search — Postgres trigram + Redis memoization."""

from __future__ import annotations

import logging

from app import repository
from app.domain import ToolDescriptor
from app.redis_client import get_cached_search, set_cached_search

logger = logging.getLogger(__name__)


async def search(
    query: str,
    *,
    session_id: str | None = None,
    limit: int = 20,
) -> list[ToolDescriptor]:
    """Search tools by name/description/tags with caching."""

    if session_id:
        cached = await get_cached_search(session_id, query)
        if cached is not None:
            tools = [await repository.get_tool(name) for name in cached]
            return [t for t in tools if t is not None]

    results = await repository.search_tools(query, limit=limit)

    if session_id:
        await set_cached_search(
            session_id, query, [t.name for t in results]
        )

    return results
