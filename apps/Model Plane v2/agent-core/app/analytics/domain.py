"""Analytics domain models."""

from __future__ import annotations

import uuid
from datetime import datetime, timezone
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


class AnalyticsEventType(str, Enum):
    # Run lifecycle
    RUN_STARTED = "run.started"
    RUN_COMPLETED = "run.completed"
    RUN_FAILED = "run.failed"
    RUN_CANCELLED = "run.cancelled"

    # Actions
    ACTION_EXECUTED = "action.executed"
    ACTION_FAILED = "action.failed"
    ACTION_RETRIED = "action.retried"

    # Policy
    POLICY_VIOLATED = "policy.violated"
    POLICY_BLOCKED = "policy.blocked"

    # Tokens / cost
    TOKEN_BUDGET_WARNING = "token.budget_warning"
    COST_BUDGET_WARNING = "cost.budget_warning"
    TOKEN_USAGE = "token.usage"
    COST_RECORDED = "cost.recorded"

    # Skills
    SKILL_MATCHED = "skill.matched"
    SKILL_EXECUTED = "skill.executed"
    SKILL_LOADED = "skill.loaded"

    # Tools
    TOOL_CALLED = "tool.called"
    TOOL_RESULT = "tool.result"
    TOOL_BLOCKED = "tool.blocked"
    TOOL_DEFERRED_PROMOTED = "tool.deferred_promoted"

    # Hooks
    HOOK_EXECUTED = "hook.executed"
    HOOK_BLOCKED = "hook.blocked"

    # Tasks
    TASK_SPAWNED = "task.spawned"
    TASK_COMPLETED = "task.completed"
    TASK_KILLED = "task.killed"

    # MCP
    MCP_CONNECTED = "mcp.connected"
    MCP_DISCONNECTED = "mcp.disconnected"
    MCP_TOOL_DISCOVERED = "mcp.tool_discovered"

    # Compaction
    COMPACT_STARTED = "compact.started"
    COMPACT_COMPLETED = "compact.completed"

    # Turn loop
    TURN_STARTED = "turn.started"
    TURN_COMPLETED = "turn.completed"
    CACHE_BREAK_DETECTED = "cache.break_detected"

    # Session
    SESSION_CREATED = "session.created"
    SESSION_RESUMED = "session.resumed"

    # Experiment
    EXPERIMENT_EXPOSED = "experiment.exposed"


class AnalyticsEvent(BaseModel):
    """Immutable analytics event record."""

    event_id: str = Field(default_factory=lambda: str(uuid.uuid4()))
    event_type: AnalyticsEventType
    org_id: str
    run_id: str | None = None
    user_id: str | None = None
    agent_id: str | None = None
    props: dict[str, Any] = Field(default_factory=dict)
    ts: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))

    model_config = {"frozen": True}

    def nats_subject(self) -> str:
        """Return the NATS subject for this event: analytics.events.{org_id}."""
        return f"analytics.events.{self.org_id}"

    def to_json_bytes(self) -> bytes:
        """Serialize to JSON bytes for NATS publishing."""
        return self.model_dump_json().encode()
