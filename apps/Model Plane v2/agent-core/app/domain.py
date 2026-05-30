"""Domain types for agent-core v2.

Evolved from v1 agent_schemas.py with additions for:
- Postgres-backed state (RunRecord, TodoItem, PlanRecord, ApprovalRecord)
- NATS command/event envelopes (SessionCommand, AgentEvent)
- Coordinator/worker model (AgentType, lease fields)
- Checkpoint support (checkpoint_index, checkpoint_state)
"""

from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field, field_validator

IDENTITY_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")
MAX_JSON_SIZE = 50_000
QUERY_DEPTH_KEY = "query_depth"


def _check_json_size(value: dict[str, Any], name: str) -> dict[str, Any]:
    if len(json.dumps(value).encode()) > MAX_JSON_SIZE:
        raise ValueError(f"{name} exceeds {MAX_JSON_SIZE} bytes")
    return value


def get_query_depth(metadata: dict[str, Any] | None) -> int:
    """Return a normalized lineage depth from run metadata."""
    if not metadata:
        return 0
    try:
        return max(0, int(metadata.get(QUERY_DEPTH_KEY, 0)))
    except (TypeError, ValueError):
        return 0


def with_query_depth(metadata: dict[str, Any] | None, depth: int) -> dict[str, Any]:
    """Return a copy of metadata with normalized lineage depth set."""
    merged = dict(metadata or {})
    merged[QUERY_DEPTH_KEY] = max(0, int(depth))
    return merged


# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------


class RunMode(str, Enum):
    PLAN = "plan"
    EXECUTE = "execute"
    REACTIVE = "reactive"


class StopReason(str, Enum):
    """Why a turn loop terminated — matches CC's stopReason field."""

    END_TURN = "end_turn"
    MAX_TURNS = "max_turns"
    TOOL_USE = "tool_use"
    INTERRUPT = "interrupt"
    ERROR = "error"
    TIMEOUT = "timeout"
    BUDGET_EXCEEDED = "budget_exceeded"
    FINAL_RESPONSE = "final_response"


class RunStatus(str, Enum):
    QUEUED = "queued"
    RUNNING = "running"
    PLANNED = "planned"
    COMPLETED = "completed"
    FAILED = "failed"
    AWAITING_APPROVAL = "awaiting_approval"
    CANCELLED = "cancelled"


class AgentType(str, Enum):
    GENERAL = "general"
    COORDINATOR = "coordinator"
    WORKER = "worker"


class ActionKind(str, Enum):
    REASONING = "reasoning"
    TOOL_CALL = "tool_call"
    CONTROL = "control"
    FINAL_RESPONSE = "final_response"


class ActionTarget(str, Enum):
    INTERNAL = "internal"
    AGENT_CORE = "agent_core"
    AI_CORE = "ai_core"  # v1 compat
    CAPABILITY_CORE = "capability_core"
    LLM_WORKER = "llm_worker"
    DOCUMENTS_WORKER = "documents_worker"  # legacy — routed to Data Plane v1
    DATA_PLANE = "data_plane"
    INTEGRATION_CORE = "integration_core"
    VOICE_CORE = "voice_core"


class ActionStatus(str, Enum):
    PLANNED = "planned"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    SKIPPED = "skipped"
    AWAITING_APPROVAL = "awaiting_approval"


class ApprovalMode(str, Enum):
    DEFAULT = "default"
    PLAN = "plan"
    AUTO = "auto"
    BYPASS = "bypass"


class TodoStatus(str, Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    BLOCKED = "blocked"


class PlanStatus(str, Enum):
    PENDING = "pending"
    APPROVED = "approved"
    REJECTED = "rejected"


class ApprovalStatus(str, Enum):
    PENDING = "pending"
    APPROVED = "approved"
    DENIED = "denied"


# ---------------------------------------------------------------------------
# NATS command / event envelopes
# ---------------------------------------------------------------------------


class SessionCommand(BaseModel):
    """Inbound command from session-core via NATS velion.session.{id}.command."""

    command_type: str  # create_run | send_message | resume | cancel
    session_id: str
    user_id: str
    org_id: str | None = None
    payload: dict[str, Any] = Field(default_factory=dict)
    idempotency_key: str | None = None
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class AgentEvent(BaseModel):
    """Outbound event published via NATS velion.agent.run.{id}.event."""

    event_type: str  # run.started | run.completed | action.started | action.completed | approval.requested | todo.updated
    run_id: str
    session_id: str
    payload: dict[str, Any] = Field(default_factory=dict)
    sequence: int = 0
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


# ---------------------------------------------------------------------------
# Execution policy
# ---------------------------------------------------------------------------


class PermissionMode(str, Enum):
    """Tool permission evaluation mode (CC toolPermission pattern)."""

    TRUST = "trust"
    INTERACTIVE = "interactive"
    COORDINATOR = "coordinator"
    SWARM = "swarm"


class ExecutionPolicy(BaseModel):
    """Bounds and permission mode for an agent run."""

    allowed_tools: list[str] = Field(default_factory=list)
    max_actions: int = Field(default=10, ge=1, le=100)
    max_turns: int = Field(default=20, ge=1, le=200)
    token_budget: int = Field(default=100_000, ge=1000, le=1_000_000)
    turn_timeout: float = Field(default=120.0, ge=5.0, le=600.0)
    run_timeout: float = Field(default=600.0, ge=30.0, le=3600.0)
    approval_mode: ApprovalMode = ApprovalMode.DEFAULT
    permission_mode: PermissionMode = PermissionMode.TRUST
    memory_isolation: bool = Field(default=False)


# ---------------------------------------------------------------------------
# Action (single step within a run)
# ---------------------------------------------------------------------------


class AgentAction(BaseModel):
    """A single planned or executed step within a run."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    kind: ActionKind
    target: ActionTarget
    name: str
    description: str | None = None
    input: dict[str, Any] = Field(default_factory=dict)
    status: ActionStatus = ActionStatus.PLANNED
    output: Any | None = None
    error: str | None = None
    readonly: bool = Field(
        default=False,
        description="True for idempotent read-only tools eligible for parallel execution.",
    )

    @field_validator("input")
    @classmethod
    def _check_input(cls, v: dict[str, Any]) -> dict[str, Any]:
        return _check_json_size(v, "input")


# ---------------------------------------------------------------------------
# Run record (Postgres-backed)
# ---------------------------------------------------------------------------


class RunRecord(BaseModel):
    """Durable agent run stored in Postgres."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    session_id: str
    parent_run_id: str | None = None
    user_id: str
    org_id: str | None = None
    agent_type: AgentType = AgentType.GENERAL
    mode: RunMode = RunMode.EXECUTE
    goal: str
    status: RunStatus = RunStatus.QUEUED
    policy: ExecutionPolicy = Field(default_factory=ExecutionPolicy)
    plan_state: dict[str, Any] | None = None
    actions: list[AgentAction] = Field(default_factory=list)
    current_action_index: int = 0
    checkpoint_index: int = 0
    checkpoint_state: dict[str, Any] | None = None
    tool_pool_version: str | None = None
    loaded_tool_names: list[str] = Field(default_factory=list)
    lease_owner: str | None = None
    final_output: str | None = None
    error: str | None = None
    total_cost_usd: float = Field(default=0.0, ge=0.0)
    metadata: dict[str, Any] = Field(default_factory=dict)
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


# ---------------------------------------------------------------------------
# Todo, Plan, Approval records (Postgres-backed)
# ---------------------------------------------------------------------------


class TodoItem(BaseModel):
    """Todo entry attached to a session or run."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    session_id: str
    run_id: str | None = None
    content: str
    status: TodoStatus = TodoStatus.PENDING
    owner_agent_id: str | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class PlanRecord(BaseModel):
    """Plan metadata with ordered steps."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    session_id: str
    run_id: str
    status: PlanStatus = PlanStatus.PENDING
    steps: list[dict[str, Any]] = Field(default_factory=list)
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class ApprovalRecord(BaseModel):
    """Approval request gating a risky mutation."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    session_id: str
    run_id: str
    action_id: str
    action_name: str
    reason: str = ""
    status: ApprovalStatus = ApprovalStatus.PENDING
    decided_by: str | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    decided_at: datetime | None = None


# ---------------------------------------------------------------------------
# Request / Response DTOs (HTTP API)
# ---------------------------------------------------------------------------


class CreateRunRequest(BaseModel):
    """HTTP API request to create an agent run."""

    agent_id: str = "general-v1"
    goal: str = Field(..., min_length=1, max_length=5000)
    mode: RunMode = RunMode.EXECUTE
    context: dict[str, Any] = Field(default_factory=dict)
    policy: ExecutionPolicy = Field(default_factory=ExecutionPolicy)
    agent_type: AgentType = AgentType.GENERAL
    parent_run_id: str | None = None
    allowed_tools: list[str] = Field(default_factory=list)

    @field_validator("goal")
    @classmethod
    def _strip_goal(cls, v: str) -> str:
        v = v.strip()
        if not v:
            raise ValueError("goal must not be blank")
        return v

    @field_validator("context")
    @classmethod
    def _check_ctx(cls, v: dict[str, Any]) -> dict[str, Any]:
        return _check_json_size(v, "context")


class RunResponse(BaseModel):
    """HTTP API response for a run."""

    run_id: str
    status: RunStatus
    final_output: str | None = None
    actions: list[AgentAction] = Field(default_factory=list)
    plan_id: str | None = None
    checkpoint_index: int = 0
    metadata: dict[str, Any] = Field(default_factory=dict)


# ---------------------------------------------------------------------------
# Streaming turn-loop event types (Phase A1)
# ---------------------------------------------------------------------------


class TurnEventKind(str, Enum):
    """Discriminator for streaming turn events."""

    ASSISTANT_CHUNK = "assistant_chunk"
    TOOL_CALL_START = "tool_call_start"
    TOOL_RESULT = "tool_result"
    PROGRESS = "progress"
    USAGE_DELTA = "usage_delta"
    COMPACT_BOUNDARY = "compact_boundary"
    TURN_COMPLETE = "turn_complete"
    LOOP_FINISHED = "loop_finished"


class TurnEvent(BaseModel):
    """A single streaming event emitted from the turn loop."""

    kind: TurnEventKind
    turn_index: int = 0
    data: dict[str, Any] = Field(default_factory=dict)


class CacheSafeParams(BaseModel):
    """Prompt-cache parameters attached to LLM calls (CC cache_control)."""

    enabled: bool = True
    cache_type: str = "ephemeral"
    ttl: str | None = None  # e.g. "1h"
    scope: str | None = None  # e.g. "global"
    disabled_for_models: list[str] = Field(default_factory=list)
