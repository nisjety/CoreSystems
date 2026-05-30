"""Agent dependencies — injected into every PydanticAI run via RunContext.

Carries the v2 service clients, run metadata, and cost tracker so tool
functions and system-prompt callbacks can access them without globals.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from app.capability_client import CapabilityClient
    from app.cost_client import CostClient
    from app.cost_tracker import CostTracker
    from app.documents_client import DocumentsClient
    from app.domain import RunRecord
    from app.llm_client import LLMClient
    from app.nats_publisher import EventPublisher
    from app.tools.registry import ToolRegistry


@dataclass
class AgentDeps:
    """Dependencies available to PydanticAI tools and system prompts.

    Fields mirror the objects that ``AgentService.__init__`` already holds,
    plus per-run context that changes with each invocation.
    """

    # --- v2 service clients (long-lived, shared across runs) ---
    llm: "LLMClient"
    capability: "CapabilityClient"
    documents: "DocumentsClient"
    publisher: "EventPublisher"
    cost_client: "CostClient | None" = None

    # --- Per-run context (set before each Agent.run()) ---
    run: "RunRecord | None" = None
    tool_registry: "ToolRegistry | None" = None
    cost_tracker: "CostTracker | None" = None

    # --- Scratch space for tool results within a single run ---
    scratch: dict[str, Any] = field(default_factory=dict)
