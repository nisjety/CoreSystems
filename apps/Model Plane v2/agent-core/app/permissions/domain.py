"""Permission domain types — modes, decisions, and denial tracking.

CC's permission system uses:
- PermissionMode to control overall policy
- ToolPermission to define per-tool rules
- DenialRecord to track what the user has rejected this session
"""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


class PermissionMode(str, Enum):
    """How tool permissions are evaluated for a run.

    - TRUST: all tools auto-approved (development, low-risk)
    - INTERACTIVE: dangerous tools require user confirmation
    - COORDINATOR: inherit permissions from parent coordinator run
    - SWARM: coordinator pre-approves a tool allowlist for workers
    """

    TRUST = "trust"
    INTERACTIVE = "interactive"
    COORDINATOR = "coordinator"
    SWARM = "swarm"


class PermissionDecision(str, Enum):
    """Result of a permission evaluation."""

    ALLOW = "allow"
    DENY = "deny"
    ASK_USER = "ask_user"


class ToolRiskLevel(str, Enum):
    """Risk classification for tools — determines permission behavior."""

    SAFE = "safe"       # Read-only, no side effects
    MODERATE = "moderate"  # Reversible mutations
    DANGEROUS = "dangerous"  # Irreversible: file delete, git push, deploy


class ToolPermissionRule(BaseModel):
    """Per-tool permission override."""

    tool_pattern: str  # fnmatch pattern (e.g. "bash:*", "mcp:*")
    risk_level: ToolRiskLevel = ToolRiskLevel.MODERATE
    auto_approve: bool = False
    reason: str = ""


class DenialRecord(BaseModel):
    """Tracks a user's denial of a specific tool in a session.

    CC's denialTracking.ts prevents re-asking the user about tools
    they've already rejected in the current session.
    """

    session_id: str
    tool_name: str
    denied_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    reason: str = ""
    denial_count: int = 1


class PermissionEvalResult(BaseModel):
    """Result of evaluating permissions for a tool call."""

    decision: PermissionDecision
    tool_name: str
    risk_level: ToolRiskLevel = ToolRiskLevel.MODERATE
    reason: str = ""
    was_previously_denied: bool = False
