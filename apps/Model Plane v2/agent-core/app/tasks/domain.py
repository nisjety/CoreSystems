"""Task domain types — mirrors CC TaskRecord with Postgres backing."""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class TaskStatus(str, Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    FAILED = "failed"
    KILLED = "killed"
    DELETED = "deleted"


TERMINAL_STATUSES = frozenset(
    {TaskStatus.COMPLETED, TaskStatus.FAILED, TaskStatus.KILLED, TaskStatus.DELETED}
)


def is_terminal(status: TaskStatus) -> bool:
    return status in TERMINAL_STATUSES


class TaskRecord(BaseModel):
    """Persistent task stored in agent_tasks table."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    run_id: str
    session_id: str
    org_id: str | None = None
    subject: str
    description: str = ""
    status: TaskStatus = TaskStatus.PENDING
    owner_agent_id: str | None = None
    blocks: list[str] = Field(default_factory=list)
    blocked_by: list[str] = Field(default_factory=list)
    metadata: dict[str, Any] = Field(default_factory=dict)
    output: str | None = None
    error: str | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class TaskUpdate(BaseModel):
    """Partial update payload for a task."""

    subject: str | None = None
    description: str | None = None
    status: TaskStatus | None = None
    owner_agent_id: str | None = None
    add_blocks: list[str] | None = None
    add_blocked_by: list[str] | None = None
    output: str | None = None
    error: str | None = None
    metadata: dict[str, Any] | None = None


class ClaimResult(BaseModel):
    """Result of attempting to claim a task."""

    success: bool
    reason: str | None = None
    task: TaskRecord | None = None
    blocked_by_tasks: list[str] | None = None


class TaskType(str, Enum):
    """What kind of work the task performs.

    Mirrors CC's ``TaskType`` — used by the executor to dispatch to the
    correct handler (local bash, sub-agent, etc.).
    """

    GENERAL = "general"           # Standard agent turn loop
    LOCAL_BASH = "local_bash"     # Run a shell command locally
    SUB_AGENT = "sub_agent"       # Spawn a child agent run
    CODE_REVIEW = "code_review"   # Focused code review turn
    FILE_EDIT = "file_edit"       # Structured file-edit task
    REMOTE_AGENT = "remote_agent"       # CC: remote teammate session
    IN_PROCESS_TEAMMATE = "in_process"  # CC: in-process agent (shared memory)
    WORKFLOW = "workflow"                # CC: multi-step orchestration
    MONITOR_MCP = "monitor_mcp"         # CC: monitor an MCP connection
    DREAM = "dream"                     # CC: background "dream" task


class ExecutedTask(BaseModel):
    """Output produced by the task executor."""

    task_id: str
    task_type: TaskType
    success: bool
    output: str | None = None
    error: str | None = None
    exit_code: int | None = None  # for LOCAL_BASH tasks
    duration_ms: int | None = None


# ---------------------------------------------------------------------------
# CC-style agent task state models (Phase B2)
# ---------------------------------------------------------------------------


class TaskProgress(BaseModel):
    """Progress tracker embedded in local agent state."""

    tool_use_count: int = 0
    token_count: int = 0
    recent_activities: list[str] = Field(default_factory=list)


class LocalAgentTaskState(BaseModel):
    """State for a locally-executing agent task (CC: AgentToolUseState)."""

    agent_id: str
    task_id: str
    progress: TaskProgress = Field(default_factory=TaskProgress)
    pending_messages: list[dict[str, Any]] = Field(default_factory=list)
    is_backgrounded: bool = False
    started_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class RemoteAgentTaskState(BaseModel):
    """State for a remotely-executing agent task (CC: RemoteAgentTaskState)."""

    session_id: str
    task_id: str
    todo_list: list[dict[str, Any]] = Field(default_factory=list)
    log: list[str] = Field(default_factory=list)
    poll_started_at: datetime | None = None
    last_poll_at: datetime | None = None


class TaskNotification(BaseModel):
    """Structured notification about task state changes."""

    task_id: str
    event: str  # "spawned" | "completed" | "failed" | "killed" | "progress"
    summary: str = ""
    data: dict[str, Any] = Field(default_factory=dict)
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
