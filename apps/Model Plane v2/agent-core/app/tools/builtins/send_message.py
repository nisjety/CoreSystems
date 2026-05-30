"""SendMessageTool — LLM-invokable wrapper for the NATS inter-agent messaging system."""

from __future__ import annotations

import logging
from typing import Any

from app.messaging.domain import AgentMessage, MessageKind
from app.messaging.publisher import publish_message
from app.nats_client import NatsManager
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Lazy reference — injected at startup via set_nats() or resolved from the app state.
_nats: NatsManager | None = None


def set_nats(nats: NatsManager) -> None:
    global _nats
    _nats = nats


class SendMessageTool:
    """Send a message to another agent's NATS mailbox."""

    name = "send_message"
    description = (
        "Send a message to another agent's mailbox. "
        "Use in coordinator or swarm mode to delegate subtasks, "
        "pass results, or notify peer agents."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "to_run_id": {
                "type": "string",
                "description": "Run ID of the target agent.",
            },
            "from_agent_id": {
                "type": "string",
                "description": "Identifier of this agent (sender).",
            },
            "kind": {
                "type": "string",
                "enum": ["task", "result", "error", "ping", "status"],
                "description": "Message type.",
                "default": "task",
            },
            "body": {
                "type": "object",
                "description": "Message payload (arbitrary JSON).",
                "default": {},
            },
            "reply_to": {
                "type": "string",
                "description": "Optional run ID for the reply target.",
            },
        },
        "required": ["to_run_id", "from_agent_id"],
    }
    search_hint = "message send notify agent mailbox nats"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Send a message to another agent. "
            "Use in multi-agent workflows to coordinate work."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if not input_data.get("to_run_id"):
            raise ValueError("'to_run_id' is required")
        if not input_data.get("from_agent_id"):
            raise ValueError("'from_agent_id' is required")
        kind_raw = input_data.get("kind", "task")
        valid_kinds = {k.value for k in MessageKind}
        if kind_raw not in valid_kinds:
            raise ValueError(f"Invalid kind '{kind_raw}'. Must be one of: {valid_kinds}")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        nats = _nats
        if nats is None:
            return ToolResult(error="NATS not initialised — SendMessageTool unavailable.")

        message = AgentMessage(
            to_run_id=input_data["to_run_id"],
            from_agent_id=input_data["from_agent_id"],
            kind=MessageKind(input_data.get("kind", "task")),
            body=input_data.get("body", {}),
            reply_to=input_data.get("reply_to"),
        )
        try:
            await publish_message(nats, message)
            logger.info(
                "send_message_tool_ok",
                extra={"to": message.to_run_id, "kind": message.kind.value},
            )
            return ToolResult(
                output=f"Message sent to {message.to_run_id} ({message.kind.value})",
                metadata={"to_run_id": message.to_run_id, "kind": message.kind.value},
            )
        except Exception as exc:
            logger.exception("send_message_tool_failed")
            return ToolResult(error=f"Failed to send message: {exc}")
