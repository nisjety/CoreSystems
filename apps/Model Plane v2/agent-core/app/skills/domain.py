"""Skills domain types — CC-style reusable skill definitions.

A skill is a reusable block of instructions, tools, and constraints
that can be loaded into an agent's system prompt dynamically.

CC skills provide:
  - Name + description for matching
  - Trigger conditions (file patterns, keywords)
  - SKILL.md content injected into system prompt
  - Optional tool restrictions
"""

from __future__ import annotations

from datetime import datetime, timezone
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class SkillSource(str, Enum):
    """Where the skill was loaded from."""

    DATABASE = "database"
    FILESYSTEM = "filesystem"
    BUNDLED = "bundled"
    MCP = "mcp"


class SkillConfig(BaseModel):
    """A skill definition registered for an org."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    org_id: str
    name: str
    description: str
    content: str  # The skill instructions (SKILL.md equivalent)
    trigger_keywords: list[str] = Field(default_factory=list)
    trigger_file_patterns: list[str] = Field(default_factory=list)
    tool_restrictions: list[str] = Field(default_factory=list)  # Allowed tools when skill active
    apply_to: list[str] = Field(default_factory=list)  # CC: glob patterns for file scoping
    when_to_use: str = ""  # CC: model-driven selection hint
    source: SkillSource = SkillSource.DATABASE
    enabled: bool = True
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class SkillMatch(BaseModel):
    """A matched skill ready for injection."""

    skill_id: str
    name: str
    content: str
    tool_restrictions: list[str] = Field(default_factory=list)
    match_score: float = 0.0  # 0.0 - 1.0 relevance
    source: SkillSource = SkillSource.DATABASE


class SkillInvocation(BaseModel):
    """Tracks that a skill was activated during a run (survives compaction)."""

    skill_id: str
    name: str
    invoked_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
