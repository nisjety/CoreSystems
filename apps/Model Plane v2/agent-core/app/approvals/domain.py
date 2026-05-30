"""Approval domain types — extends existing ApprovalRecord with elicitation flow."""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class ApprovalKind(str, Enum):
    """What triggered the approval request."""

    TOOL_EXECUTION = "tool_execution"
    PLAN_EXECUTION = "plan_execution"
    DESTRUCTIVE_ACTION = "destructive_action"
    CUSTOM = "custom"


class ApprovalRequest(BaseModel):
    """Approval request emitted by the run loop when a risky action is detected."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    run_id: str
    session_id: str
    action_id: str
    action_name: str
    kind: ApprovalKind = ApprovalKind.TOOL_EXECUTION
    reason: str = ""
    context: dict[str, Any] = Field(default_factory=dict)
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class ApprovalDecision(BaseModel):
    """Decision payload from the frontend/session-core."""

    approved: bool
    decided_by: str
    comment: str = ""
