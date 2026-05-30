"""Rich message types — CC-compatible event taxonomy.

Maps Claude Code's message union (UserMessage, AssistantMessage,
ToolUseMessage, ToolResultMessage, CompactMessage, …) into a
typed RunEvent envelope for backend streaming and persistence.

Sequence numbering: each run gets a monotonic seq counter.
Events are stored in the ``run_events`` Postgres table for
replay, debugging, and teleport operations.

Design decisions (CC-parity):
- MessageType covers all 15+ CC message kinds
- RunEvent is the single envelope for streaming + persistence
- RunEventBatch wraps a list for bulk sending
- build_event() factory ensures consistent field population
"""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


class MessageRole(str, Enum):
    """Who created this message."""

    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"


class MessageType(str, Enum):
    """Rich message type taxonomy — mirrors CC's message union.

    CC types → MessageType mapping:
    - UserMessage         → USER_MESSAGE
    - AssistantMessage    → ASSISTANT_TEXT / ASSISTANT_CHUNK
    - ToolUseMessage      → TOOL_USE
    - ToolResultMessage   → TOOL_RESULT
    - CompactMessage      → CONTEXT_COMPACT
    - ProgressMessage     → PROGRESS
    - CostMessage         → USAGE_DELTA
    - ErrorMessage        → ERROR
    - AttachmentMessage   → ATTACHMENT
    - PlanMessage         → PLAN_UPDATE
    - TodoMessage         → TODO_UPDATE
    - ApprovalMessage     → APPROVAL_REQUEST / APPROVAL_DECISION
    - ContextCollapse     → CONTEXT_COMPACT
    - DestructiveWarning  → DESTRUCTIVE_WARNING
    - ClassifierApproval  → CLASSIFIER_APPROVAL
    """

    # ---- Core message flow ----
    USER_MESSAGE = "user_message"
    ASSISTANT_TEXT = "assistant_text"
    ASSISTANT_CHUNK = "assistant_chunk"  # streaming delta

    # ---- Tool execution ----
    TOOL_USE = "tool_use"
    TOOL_RESULT = "tool_result"
    TOOL_ERROR = "tool_error"

    # ---- Progress / cost ----
    PROGRESS = "progress"
    USAGE_DELTA = "usage_delta"

    # ---- Context management ----
    CONTEXT_COMPACT = "context_compact"

    # ---- Plan / coordination ----
    PLAN_UPDATE = "plan_update"
    TODO_UPDATE = "todo_update"
    SPAWN_AGENT = "spawn_agent"
    AGENT_RESULT = "agent_result"

    # ---- Approval / permission ----
    APPROVAL_REQUEST = "approval_request"
    APPROVAL_DECISION = "approval_decision"
    DESTRUCTIVE_WARNING = "destructive_warning"
    CLASSIFIER_APPROVAL = "classifier_approval"

    # ---- Security / policy ----
    PERMISSION_DENIED = "permission_denied"
    RATE_LIMIT_HIT = "rate_limit_hit"

    # ---- Lifecycle ----
    RUN_STARTED = "run_started"
    RUN_COMPLETED = "run_completed"
    RUN_FAILED = "run_failed"
    TURN_START = "turn_start"
    TURN_END = "turn_end"

    # ---- Attachment / media ----
    ATTACHMENT = "attachment"

    # ---- MCP ----
    MCP_SERVER_CONNECTED = "mcp_server_connected"
    MCP_SERVER_DISCONNECTED = "mcp_server_disconnected"

    # ---- Recovery / teleport ----
    SNAPSHOT_CREATED = "snapshot_created"
    SESSION_RESTORED = "session_restored"

    # ---- Error ----
    ERROR = "error"


class RunEvent(BaseModel):
    """Single event in a run's event stream.

    Designed for:
    - Streaming via SSE / NATS to frontends
    - Storage in ``run_events`` Postgres table for replay
    - Webhook delivery to external listeners
    """

    run_id: str
    session_id: str
    seq: int = 0                       # monotonic per-run sequence
    type: MessageType
    role: MessageRole = MessageRole.SYSTEM
    turn_index: int = 0

    # Flexible payload — structure depends on `type`:
    # ASSISTANT_CHUNK  → {"delta": "partial text"}
    # TOOL_USE         → {"tool_name": "search", "input": {...}, "tool_use_id": "..."}
    # TOOL_RESULT      → {"tool_use_id": "...", "output": ..., "error": null}
    # USAGE_DELTA      → {"input_tokens": 500, "output_tokens": 50, "model": "..."}
    # DESTRUCTIVE_WARNING → {"command": "rm -rf /", "warning": "..."}
    # CLASSIFIER_APPROVAL → {"tool_use_id": "...", "classifier": "bash", "matched_rule": "..."}
    data: dict[str, Any] = Field(default_factory=dict)

    # Optional: full text for assistant messages, error messages
    content: str | None = None

    # Optional: token budget info for context management
    token_count: int | None = None

    # Timing
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))

    class Config:
        json_encoders = {
            datetime: lambda v: v.isoformat(),
        }


class RunEventBatch(BaseModel):
    """A batch of events for bulk transport (webhook, replay)."""

    run_id: str
    events: list[RunEvent]
    checkpoint_seq: int = 0  # seq of last event in the batch

    @property
    def count(self) -> int:
        return len(self.events)


# ---------------------------------------------------------------------------
# Factory
# ---------------------------------------------------------------------------

def build_event(
    run_id: str,
    session_id: str,
    msg_type: MessageType,
    *,
    seq: int = 0,
    role: MessageRole = MessageRole.SYSTEM,
    turn_index: int = 0,
    data: dict[str, Any] | None = None,
    content: str | None = None,
    token_count: int | None = None,
) -> RunEvent:
    """Create a RunEvent with consistent defaults."""
    return RunEvent(
        run_id=run_id,
        session_id=session_id,
        seq=seq,
        type=msg_type,
        role=role,
        turn_index=turn_index,
        data=data or {},
        content=content,
        token_count=token_count,
    )
