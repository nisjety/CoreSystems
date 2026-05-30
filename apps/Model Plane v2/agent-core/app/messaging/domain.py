"""Messaging domain types — discriminated union for inter-agent messages."""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class MessageKind(str, Enum):
    """Discriminator for inter-agent message types."""

    MESSAGE = "message"
    TASK_ASSIGNMENT = "task_assignment"
    TASK_RESULT = "task_result"
    SHUTDOWN_REQUEST = "shutdown_request"
    SHUTDOWN_RESPONSE = "shutdown_response"
    PLAN_APPROVAL_RESPONSE = "plan_approval_response"


class AgentMessage(BaseModel):
    """Envelope for inter-agent communication over NATS mailbox."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    kind: MessageKind
    from_agent_id: str
    from_agent_name: str | None = None
    to_run_id: str
    text: str = ""
    payload: dict[str, Any] = Field(default_factory=dict)
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class SendMessageRequest(BaseModel):
    """API request to send a message to another agent run."""

    target_run_id: str
    text: str
    kind: MessageKind = MessageKind.MESSAGE
    payload: dict[str, Any] = Field(default_factory=dict)


class InboxMessage(BaseModel):
    """Message as seen by the receiving agent."""

    id: str
    kind: MessageKind
    from_agent_id: str
    from_agent_name: str | None = None
    text: str
    payload: dict[str, Any] = Field(default_factory=dict)
    timestamp: datetime
    read: bool = False
