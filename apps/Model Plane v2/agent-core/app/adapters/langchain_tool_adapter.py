"""LangChain tool adapter — thin wrapper for non-core tool integrations.

Routes tool calls through capability-core's tool execution.
LangChain itself is NOT used in v2; this adapter now delegates to
the v2-native CapabilityClient for any tools that were previously
backed by LangChain in v1.
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import AgentAction, RunRecord

logger = logging.getLogger(__name__)

_capability: Any | None = None  # CapabilityClient injected at startup


def wire(capability_client: Any) -> None:
    """Wire in the CapabilityClient singleton from main.py lifespan."""
    global _capability
    _capability = capability_client
    logger.info("langchain_adapter_wired")


async def execute(action: AgentAction, run: RunRecord) -> Any:
    """Execute a tool previously backed by LangChain.

    In v2, delegate to capability-core's tool dispatch instead.
    """
    if _capability is None:
        msg = (
            "LangChain tool adapter is not yet wired to v2. "
            "Capability-core tool dispatch will replace LangChain tool execution."
        )
        logger.warning("langchain_adapter_not_ready", extra={"run_id": run.id})
        return {"error": msg, "adapter": "langchain_tool", "status": "not_wired"}

    tool_name: str = action.name
    parameters: dict[str, Any] = action.input or {}

    return await _capability.execute_tool(
        tool_name,
        parameters,
        user_id=run.user_id,
        org_id=run.org_id,
        session_id=run.session_id,
        run_id=run.id,
        action_id=action.id,
    )
