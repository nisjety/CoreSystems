"""Domain types for execution-core.

Runner lifecycle, workspace bootstrap, artifact transfer, and lease management.
"""

from __future__ import annotations

import re
from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field, field_validator

IDENTITY_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")


# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------


class RunnerStatus(str, Enum):
    IDLE = "idle"
    CLAIMED = "claimed"
    RUNNING = "running"
    COMPLETING = "completing"
    CANCELLED = "cancelled"
    DEAD = "dead"


class RunnerCapability(str, Enum):
    """Capabilities a runner can advertise."""

    TOOL_EXEC = "tool_exec"
    CODE_EXEC = "code_exec"
    FILE_IO = "file_io"
    WEB_FETCH = "web_fetch"
    MCP_CLIENT = "mcp_client"
    SANDBOX = "sandbox"


class ArtifactKind(str, Enum):
    INPUT = "input"
    OUTPUT = "output"
    LOG = "log"
    CHECKPOINT = "checkpoint"


# ---------------------------------------------------------------------------
# Runner identity and registration
# ---------------------------------------------------------------------------


class RunnerRegistration(BaseModel):
    """Published on velion.runner.register by a runner joining the pool."""

    runner_id: str = Field(default_factory=lambda: f"runner-{uuid4().hex[:12]}")
    capabilities: list[RunnerCapability] = Field(default_factory=list)
    max_concurrent: int = Field(default=1, ge=1, le=50)
    labels: dict[str, str] = Field(default_factory=dict)
    workspace_id: str | None = None

    @field_validator("runner_id")
    @classmethod
    def validate_runner_id(cls, v: str) -> str:
        if not IDENTITY_RE.match(v):
            raise ValueError(f"Invalid runner_id: {v!r}")
        return v


class RunnerHeartbeat(BaseModel):
    """Published periodically on velion.runner.heartbeat.{runner_id}."""

    runner_id: str
    status: RunnerStatus
    current_task_id: str | None = None
    cpu_percent: float | None = None
    memory_mb: float | None = None
    ts: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


# ---------------------------------------------------------------------------
# Task assignment / claim
# ---------------------------------------------------------------------------


class TaskClaim(BaseModel):
    """Published on velion.runner.claim by agent-core to assign work."""

    task_id: str = Field(default_factory=lambda: f"task-{uuid4().hex[:12]}")
    run_id: str
    session_id: str
    workspace_id: str
    tool_name: str
    tool_input: dict[str, Any] = Field(default_factory=dict)
    timeout_seconds: int = Field(default=300, ge=10, le=3600)
    priority: int = Field(default=0, ge=0, le=10)

    @field_validator("task_id", "run_id", "session_id", "workspace_id")
    @classmethod
    def validate_ids(cls, v: str) -> str:
        if not IDENTITY_RE.match(v):
            raise ValueError(f"Invalid id: {v!r}")
        return v


class TaskResult(BaseModel):
    """Published on velion.runner.complete.{task_id} by a runner."""

    task_id: str
    runner_id: str
    success: bool
    output: dict[str, Any] = Field(default_factory=dict)
    error: str | None = None
    duration_ms: int = 0
    artifacts: list[str] = Field(default_factory=list)
    ts: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class TaskCancel(BaseModel):
    """Published on velion.runner.cancel.{task_id} to abort a task."""

    task_id: str
    reason: str = "cancelled_by_user"


# ---------------------------------------------------------------------------
# Artifact metadata
# ---------------------------------------------------------------------------


class ArtifactMetadata(BaseModel):
    """Stored in Postgres for each uploaded/downloaded artifact."""

    artifact_id: str = Field(default_factory=lambda: f"art-{uuid4().hex[:12]}")
    task_id: str
    workspace_id: str
    kind: ArtifactKind
    filename: str
    size_bytes: int = 0
    content_type: str = "application/octet-stream"
    storage_key: str = ""
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


# ---------------------------------------------------------------------------
# Database records (Postgres)
# ---------------------------------------------------------------------------


class RunnerRecord(BaseModel):
    """Row in runner_inventory table."""

    runner_id: str
    capabilities: list[str] = Field(default_factory=list)
    max_concurrent: int = 1
    labels: dict[str, str] = Field(default_factory=dict)
    status: RunnerStatus = RunnerStatus.IDLE
    current_task_id: str | None = None
    workspace_id: str | None = None
    last_heartbeat: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    registered_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class TaskRecord(BaseModel):
    """Row in runner_tasks table."""

    task_id: str
    run_id: str
    session_id: str
    workspace_id: str
    runner_id: str | None = None
    tool_name: str
    tool_input: dict[str, Any] = Field(default_factory=dict)
    status: RunnerStatus = RunnerStatus.IDLE
    priority: int = 0
    timeout_seconds: int = 300
    output: dict[str, Any] | None = None
    error: str | None = None
    duration_ms: int = 0
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    started_at: datetime | None = None
    completed_at: datetime | None = None
