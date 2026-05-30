"""MCP elicitation handler — MCP servers can ask questions mid-call.

CC pattern: during a tool call, an MCP server may send an
``elicitation`` request asking the agent to provide additional
information (e.g. "Which branch?" or "Confirm deletion?").

The handler queues these questions and provides answers from the
agent or user, then returns control to the MCP call.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any, Callable, Awaitable

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class ElicitationRequest(BaseModel):
    """An MCP server asking for more information."""

    request_id: str
    server_name: str
    tool_name: str
    message: str
    schema: dict[str, Any] | None = None  # JSON Schema for expected response
    timeout: float = 30.0


class ElicitationResponse(BaseModel):
    """Agent or user response to an elicitation."""

    request_id: str
    action: str = "provide"  # "provide" | "cancel" | "skip"
    content: dict[str, Any] = Field(default_factory=dict)


# Type for an elicitation handler callback
ElicitationCallback = Callable[
    [ElicitationRequest], Awaitable[ElicitationResponse]
]


class ElicitationHandler:
    """Handles MCP elicitation requests during tool calls.

    Register a callback that will be invoked when an MCP server
    requests additional input. If no callback is registered,
    elicitations are auto-cancelled.
    """

    def __init__(self) -> None:
        self._callback: ElicitationCallback | None = None
        self._pending: dict[str, asyncio.Future] = {}
        self._history: list[tuple[ElicitationRequest, ElicitationResponse]] = []

    def set_callback(self, callback: ElicitationCallback) -> None:
        """Register the handler callback."""
        self._callback = callback

    async def handle(self, request: ElicitationRequest) -> ElicitationResponse:
        """Process an elicitation request.

        If a callback is registered, delegates to it. Otherwise, returns
        a cancel response.
        """
        if self._callback is not None:
            try:
                response = await asyncio.wait_for(
                    self._callback(request),
                    timeout=request.timeout,
                )
            except asyncio.TimeoutError:
                logger.warning(
                    "elicitation_timeout",
                    extra={"request_id": request.request_id},
                )
                response = ElicitationResponse(
                    request_id=request.request_id,
                    action="cancel",
                )
        else:
            response = ElicitationResponse(
                request_id=request.request_id,
                action="cancel",
            )

        self._history.append((request, response))
        return response

    @property
    def history(self) -> list[tuple[ElicitationRequest, ElicitationResponse]]:
        return list(self._history)

    def clear_history(self) -> None:
        self._history.clear()
