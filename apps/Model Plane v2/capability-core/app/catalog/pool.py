"""Tool pool builder — eager / deferred separation.

Builds a session-scoped pool from tool descriptors stored in Postgres.
Eager tools go into every LLM call; deferred tools are loaded only
when semantic search matches them to the current user intent.
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import ToolDescriptor, ToolPool

logger = logging.getLogger(__name__)


def build_pool(
    session_id: str,
    all_tools: list[ToolDescriptor],
    *,
    always_load: set[str] | None = None,
    never_load: set[str] | None = None,
    model_context_window: int = 128_000,
) -> ToolPool:
    """Partition tools into eager / deferred / mcp sets.

    Parameters
    ----------
    model_context_window:
        Max tokens available; if too small the pool can be trimmed in future.
    """
    always = always_load or set()
    never = never_load or set()

    eager: list[ToolDescriptor] = []
    deferred: list[ToolDescriptor] = []
    mcp: list[ToolDescriptor] = []

    for tool in all_tools:
        if tool.name in never:
            continue

        if tool.mcp_server_id:
            mcp.append(tool)

        if tool.name in always:
            eager.append(tool)
        elif tool.is_eager():
            eager.append(tool)
        else:
            deferred.append(tool)

    logger.info(
        "tool_pool_built",
        extra={
            "session_id": session_id,
            "eager": len(eager),
            "deferred": len(deferred),
            "mcp": len(mcp),
        },
    )
    return ToolPool(
        session_id=session_id,
        eager=eager,
        deferred=deferred,
        mcp=mcp,
    )
