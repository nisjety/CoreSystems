"""Hook system domain types.

Implements the PreToolUse / PostToolUse / Stop hook pattern from Claude Code.
Hooks let orgs intercept tool calls with approve/block/modify semantics
without changing the agent execution code.
"""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class HookType(str, Enum):
    PRE_TOOL_USE = "pre_tool_use"
    POST_TOOL_USE = "post_tool_use"
    STOP = "stop"
    PRE_COMPACT = "pre_compact"
    POST_COMPACT = "post_compact"
    PERMISSION_CHECK = "permission_check"
    SESSION_START = "session_start"
    SESSION_END = "session_end"
    SUBAGENT_START = "subagent_start"
    SUBAGENT_STOP = "subagent_stop"


class HookAction(str, Enum):
    APPROVE = "approve"
    BLOCK = "block"
    MODIFY = "modify"


class HookConfig(BaseModel):
    """Persistent hook configuration stored in Postgres."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    org_id: str
    tool_name_pattern: str  # supports wildcards: "bash:*", "mcp:*", "*"
    hook_type: HookType
    action: HookAction
    reason: str = ""
    modify_input: dict[str, Any] | None = None
    modify_output: dict[str, Any] | None = None
    priority: int = Field(default=0, ge=0, le=1000)
    enabled: bool = True
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class PreToolUseResult(BaseModel):
    """Result of running pre-tool-use hooks against an action."""

    approved: bool = True
    modified_input: dict[str, Any] | None = None
    blocked_by: str | None = None  # hook id that blocked
    reason: str | None = None


class PostToolUseResult(BaseModel):
    """Result of running post-tool-use hooks against an action result."""

    modified_output: Any | None = None
    modified_by: str | None = None  # hook id that modified


class StopResult(BaseModel):
    """Result of running stop hooks at run termination."""

    should_continue: bool = True
    reason: str | None = None


class HookBlockedError(Exception):
    """Raised when a pre-tool-use hook blocks execution."""

    def __init__(self, hook_id: str, tool_name: str, reason: str = "") -> None:
        self.hook_id = hook_id
        self.tool_name = tool_name
        self.reason = reason
        super().__init__(f"Hook {hook_id} blocked tool '{tool_name}': {reason}")


class PreCompactResult(BaseModel):
    """Result of running pre-compact hooks."""

    proceed: bool = True
    modified_messages: list[dict[str, Any]] | None = None
    reason: str | None = None


class PostCompactResult(BaseModel):
    """Result of running post-compact hooks."""

    modified_summary: str | None = None
    inject_messages: list[dict[str, Any]] | None = None
    modified_by: str | None = None


class PermissionCheckResult(BaseModel):
    """Result of running permission-check hooks for a tool invocation."""

    allowed: bool = True
    reason: str | None = None
    requires_confirmation: bool = False
    blocked_by: str | None = None


class LifecycleHookResult(BaseModel):
    """Result of running lifecycle hooks around session or subagent events."""

    proceed: bool = True
    modified_payload: dict[str, Any] | None = None
    reason: str | None = None
    modified_by: str | None = None
