"""Coordinator mode — enter/exit coordinator mode for a session."""

from __future__ import annotations

import logging

from app.coordinator.prompt import (
    COORDINATOR_ALLOWED_TOOLS,
    COORDINATOR_SYSTEM_PROMPT,
)
from app.coordinator.workers import WorkerManager

logger = logging.getLogger(__name__)


class CoordinatorMode:
    """Manage coordinator mode state for a single session.

    When active, the system prompt is augmented and tool access
    is restricted to the coordination subset.
    """

    def __init__(self) -> None:
        self._active: bool = False
        self._workers: WorkerManager = WorkerManager()

    @property
    def active(self) -> bool:
        return self._active

    @property
    def workers(self) -> WorkerManager:
        return self._workers

    def enter(self) -> str:
        """Activate coordinator mode. Returns the system prompt addition."""
        if self._active:
            return ""
        self._active = True
        self._workers = WorkerManager()
        logger.info("Entered coordinator mode")
        return COORDINATOR_SYSTEM_PROMPT

    def exit(self) -> None:
        """Deactivate coordinator mode."""
        self._active = False
        logger.info("Exited coordinator mode")

    def filter_tools(self, tool_names: list[str]) -> list[str]:
        """Return only the tools allowed in coordinator mode.

        If coordinator mode is not active, returns all tools unchanged.
        """
        if not self._active:
            return tool_names
        return [t for t in tool_names if t in COORDINATOR_ALLOWED_TOOLS]

    def inject_system_prompt(self, base_prompt: str) -> str:
        """Append coordinator instructions to the base system prompt."""
        if not self._active:
            return base_prompt
        return f"{base_prompt}\n\n{COORDINATOR_SYSTEM_PROMPT}"
