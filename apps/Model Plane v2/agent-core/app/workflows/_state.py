"""Module-level state for Temporal activities.

Activities run in a Temporal worker thread/task but need access to the
AgentService singleton (which holds LLMClient, CapabilityClient, etc.).
This module provides a simple getter/setter so activities can retrieve
the service without importing app.main (which would create circular deps).
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.agent_service import AgentService

_agent_service: "AgentService | None" = None


def set_agent_service(svc: "AgentService") -> None:
    """Called once at startup from main.py to register the service."""
    global _agent_service
    _agent_service = svc


def get_agent_service() -> "AgentService":
    """Retrieve the AgentService singleton. Raises if not set."""
    if _agent_service is None:
        raise RuntimeError(
            "AgentService not registered — call set_agent_service() first"
        )
    return _agent_service
