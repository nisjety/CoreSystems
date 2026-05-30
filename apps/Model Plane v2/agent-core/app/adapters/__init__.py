"""Framework adapters — isolated integration modules, NOT in the hot path.

Each adapter provides a narrow interface to a specific framework.
They are loaded lazily and dispatched by ActionTarget + metadata.

Phase plan:
  - Phase 1: temporal_activity → real temporalio SDK
  - Phase 3: langgraph_workflow → in-process LangGraph
  - Phase 5: letta_memory → letta-client SDK
  - langchain_tool → capability-core tool dispatch (no LangChain)
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import AgentAction, RunRecord

logger = logging.getLogger(__name__)

# Registry of adapter names → loader functions
_ADAPTER_REGISTRY: dict[str, str] = {
    "letta_memory": "app.adapters.letta_memory_adapter",
    "langgraph_workflow": "app.adapters.langgraph_workflow_adapter",
    "temporal_activity": "app.adapters.temporal_activity_adapter",
    "langchain_tool": "app.adapters.langchain_tool_adapter",
}


async def dispatch_to_adapter(action: AgentAction, run: RunRecord) -> Any:
    """Route an action to the appropriate framework adapter.

    The adapter is determined by action.input.get("adapter") or
    inferred from the action name pattern. Falls back to a no-op
    if no adapter matches.
    """
    adapter_name = action.input.get("adapter", "")

    if not adapter_name:
        adapter_name = _infer_adapter(action)

    if not adapter_name or adapter_name not in _ADAPTER_REGISTRY:
        logger.warning(
            "no_adapter_matched",
            extra={"action": action.name, "adapter": adapter_name},
        )
        return {"warning": f"No adapter found for action '{action.name}'"}

    module_path = _ADAPTER_REGISTRY[adapter_name]
    try:
        import importlib

        mod = importlib.import_module(module_path)
        return await mod.execute(action, run)
    except ImportError:
        logger.error("adapter_import_failed", extra={"module": module_path})
        raise
    except Exception as exc:
        logger.error(
            "adapter_execution_failed",
            extra={"adapter": adapter_name, "error": str(exc)},
        )
        raise


def _infer_adapter(action: AgentAction) -> str:
    """Try to infer the adapter from the action name."""
    name = action.name.lower()
    if "memory" in name or "letta" in name:
        return "letta_memory"
    if "workflow" in name or "langgraph" in name:
        return "langgraph_workflow"
    if "temporal" in name or "batch" in name or "ingestion" in name:
        return "temporal_activity"
    if "langchain" in name:
        return "langchain_tool"
    return ""
