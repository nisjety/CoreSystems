"""Agent definition and model selection — CC-compatible agent configuration.

Ported from CC's utils/model/agent.ts.

Features:
- AgentDefinition: typed agent config with model, tools, and policy
- get_agent_model(): resolve effective model for an agent (inherit, alias, override)
- Model alias resolution with provider awareness
"""

from __future__ import annotations

import logging
import os
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field

from app.domain import AgentType, ExecutionPolicy, PermissionMode

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Model aliases (CC MODEL_ALIASES)
# ---------------------------------------------------------------------------

class ModelAlias(str, Enum):
    """Shorthand model aliases."""

    SONNET = "sonnet"
    OPUS = "opus"
    HAIKU = "haiku"
    BEST = "best"
    FAST = "fast"
    INHERIT = "inherit"


# Alias → concrete model mapping per provider
_ALIAS_MAP: dict[str, dict[str, str]] = {
    "openai": {
        "sonnet": "gpt-4o",
        "opus": "gpt-4o",
        "haiku": "gpt-4o-mini",
        "best": "gpt-4o",
        "fast": "gpt-4o-mini",
    },
    "anthropic": {
        "sonnet": "claude-sonnet-4-20250514",
        "opus": "claude-opus-4-20250514",
        "haiku": "claude-3-5-haiku-20241022",
        "best": "claude-opus-4-20250514",
        "fast": "claude-3-5-haiku-20241022",
    },
    "google": {
        "sonnet": "gemini-2.0-flash",
        "opus": "gemini-1.5-pro",
        "haiku": "gemini-2.0-flash",
        "best": "gemini-1.5-pro",
        "fast": "gemini-2.0-flash",
    },
}

# Default provider when not specified
DEFAULT_PROVIDER = "anthropic"


# ---------------------------------------------------------------------------
# Agent definition
# ---------------------------------------------------------------------------

class AgentDefinition(BaseModel):
    """Full agent configuration — defines model, tools, policy, and behavior.

    Used when spawning sub-agents or configuring custom agent types.
    """

    name: str
    description: str = ""

    # Model selection (CC pattern: alias or full model ID, or "inherit")
    model: str = "inherit"
    provider: str | None = None  # override provider (auto-detect from model if None)

    # Agent identity
    agent_type: AgentType = AgentType.GENERAL
    system_prompt: str | None = None
    system_prompt_suffix: str | None = None

    # Tool configuration
    allowed_tools: list[str] = Field(default_factory=list)
    denied_tools: list[str] = Field(default_factory=list)
    mcp_servers: list[str] = Field(default_factory=list)  # MCP server names to connect

    # Execution policy
    policy: ExecutionPolicy = Field(default_factory=ExecutionPolicy)
    permission_mode: PermissionMode = PermissionMode.TRUST

    # Sub-agent spawning
    max_sub_agents: int = 5
    sub_agent_model: str = "inherit"

    # Behavior flags
    auto_compact: bool = True
    memory_enabled: bool = True
    skill_injection: bool = True

    class Config:
        use_enum_values = True


# ---------------------------------------------------------------------------
# Model resolution (CC getAgentModel)
# ---------------------------------------------------------------------------

def _alias_matches_parent_tier(alias: str, parent_model: str) -> bool:
    """Check if a bare family alias matches the parent model's tier.

    CC pattern: prevents surprising downgrades when a user on Opus
    spawns a subagent with model="opus" — should get the same Opus,
    not a different default.
    """
    alias_lower = alias.lower()
    parent_lower = parent_model.lower()

    if alias_lower == "opus":
        return "opus" in parent_lower
    if alias_lower == "sonnet":
        return "sonnet" in parent_lower
    if alias_lower == "haiku":
        return "haiku" in parent_lower

    return False


def resolve_model_alias(alias: str, provider: str = DEFAULT_PROVIDER) -> str:
    """Resolve a model alias to a concrete model ID.

    Returns the alias unchanged if it's not a known alias.
    """
    provider_map = _ALIAS_MAP.get(provider, _ALIAS_MAP[DEFAULT_PROVIDER])
    return provider_map.get(alias.lower(), alias)


def get_agent_model(
    agent_model: str | None,
    parent_model: str,
    *,
    tool_specified_model: str | None = None,
    provider: str = DEFAULT_PROVIDER,
) -> str:
    """Get the effective model string for an agent.

    Mirrors CC's getAgentModel() priority chain:
    1. AGENT_SUBAGENT_MODEL env override
    2. Tool-specified model
    3. Agent definition model
    4. "inherit" → parent model
    5. Alias resolution

    For provider-specific prefix inheritance (Bedrock region prefix),
    the parent's prefix is inherited by subagents using alias models.
    """
    # Priority 1: Environment override
    env_model = os.environ.get("AGENT_SUBAGENT_MODEL")
    if env_model:
        return env_model

    # Priority 2: Tool-specified model
    if tool_specified_model:
        if _alias_matches_parent_tier(tool_specified_model, parent_model):
            return parent_model
        return resolve_model_alias(tool_specified_model, provider)

    # Priority 3: Agent definition model
    effective = agent_model or "inherit"

    if effective == "inherit":
        return parent_model

    # Priority 4: Check if alias matches parent tier
    if _alias_matches_parent_tier(effective, parent_model):
        return parent_model

    # Priority 5: Resolve alias
    return resolve_model_alias(effective, provider)


def get_default_subagent_model() -> str:
    """Get the default model for subagents (inherit from parent)."""
    return "inherit"


def get_agent_model_display(model: str | None) -> str:
    """Human-readable display name for an agent model."""
    if not model:
        return "Inherit from parent (default)"
    if model == "inherit":
        return "Inherit from parent"
    return model.capitalize() if len(model) < 20 else model


def detect_provider(model: str) -> str:
    """Detect the provider from a model name."""
    model_lower = model.lower()
    if any(k in model_lower for k in ("gpt", "o1", "o3", "davinci", "curie")):
        return "openai"
    if any(k in model_lower for k in ("claude", "sonnet", "opus", "haiku")):
        return "anthropic"
    if any(k in model_lower for k in ("gemini", "palm")):
        return "google"
    return DEFAULT_PROVIDER
