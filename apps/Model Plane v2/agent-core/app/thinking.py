"""Extended thinking — model-aware reasoning configuration.

Ported from CC's utils/thinking.ts: three thinking modes (adaptive, enabled,
disabled), per-model capability detection, and budget management.

The thinking config is threaded through the turn loop and passed to the
LLM client so the API call includes the correct `thinking` parameter.
"""

from __future__ import annotations

import logging
import os
import re
from dataclasses import dataclass
from enum import Enum
from typing import Any

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Thinking mode types (CC ThinkingConfig)
# ---------------------------------------------------------------------------

class ThinkingMode(str, Enum):
    """Extended thinking mode for LLM calls."""
    ADAPTIVE = "adaptive"   # Model decides when/how much to think (Opus 4.6+)
    ENABLED = "enabled"     # Fixed budget_tokens budget
    DISABLED = "disabled"   # No extended thinking


@dataclass(frozen=True)
class ThinkingConfig:
    """Configuration for extended thinking on a single LLM call.

    Attributes:
        mode: One of adaptive, enabled, disabled.
        budget_tokens: Token budget for thinking (only used when mode=enabled).
    """
    mode: ThinkingMode = ThinkingMode.DISABLED
    budget_tokens: int = 0

    def to_api_param(self) -> dict[str, Any] | None:
        """Convert to the API parameter dict for the LLM client.

        Returns None if thinking is disabled.
        """
        if self.mode == ThinkingMode.DISABLED:
            return None
        if self.mode == ThinkingMode.ADAPTIVE:
            return {"type": "enabled", "budget_tokens": self.budget_tokens or 10_000}
        # mode == ENABLED
        return {"type": "enabled", "budget_tokens": self.budget_tokens}

    @property
    def is_active(self) -> bool:
        return self.mode != ThinkingMode.DISABLED


# ---------------------------------------------------------------------------
# Model capability detection (CC modelSupportsThinking)
# ---------------------------------------------------------------------------

# Models that support extended thinking (any mode)
_THINKING_CAPABLE: frozenset[str] = frozenset({
    "claude-4-opus",
    "claude-opus-4",
    "claude-opus-4-0725",
    "claude-4-sonnet",
    "claude-sonnet-4",
    "claude-sonnet-4-0725",
    "claude-3.5-sonnet",
    "claude-3-5-sonnet-20241022",
    "claude-3-opus",
    "o1",
    "o1-preview",
    "o3-mini",
    "o3",
    "o4-mini",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
})

# Models that support adaptive thinking (model decides budget)
_ADAPTIVE_CAPABLE: frozenset[str] = frozenset({
    "claude-4-opus",
    "claude-opus-4",
    "claude-opus-4-0725",
    "claude-4-sonnet",
    "claude-sonnet-4",
    "claude-sonnet-4-0725",
})

# Default thinking budget when mode=enabled
_DEFAULT_BUDGET = 10_000

# Max thinking budget (CC: typically 32k)
_MAX_BUDGET = 32_000


def _normalize_model(model: str) -> str:
    """Normalize model name by stripping provider prefixes and version suffixes."""
    # Strip provider prefixes like "anthropic/" or "bedrock/"
    if "/" in model:
        model = model.split("/")[-1]
    # Strip region prefixes like "us." or "eu."
    if re.match(r"^[a-z]{2}\.", model):
        model = model[3:]
    return model.lower()


def model_supports_thinking(model: str) -> bool:
    """Return True if the model supports any form of extended thinking."""
    norm = _normalize_model(model)
    return any(norm.startswith(m) for m in _THINKING_CAPABLE)


def model_supports_adaptive(model: str) -> bool:
    """Return True if the model supports adaptive thinking (model-driven budget)."""
    norm = _normalize_model(model)
    return any(norm.startswith(m) for m in _ADAPTIVE_CAPABLE)


# ---------------------------------------------------------------------------
# Default thinking config resolution
# ---------------------------------------------------------------------------

def resolve_thinking_config(
    model: str,
    *,
    user_override: ThinkingMode | None = None,
    budget_override: int | None = None,
) -> ThinkingConfig:
    """Resolve the thinking config for a model and optional overrides.

    Priority chain (matches CC):
      1. User override from request
      2. MAX_THINKING_TOKENS env var
      3. alwaysThinkingEnabled setting (here: default enabled for capable models)
      4. Model capability detection
    """
    # 1. User override
    if user_override == ThinkingMode.DISABLED:
        return ThinkingConfig(mode=ThinkingMode.DISABLED)

    if not model_supports_thinking(model):
        return ThinkingConfig(mode=ThinkingMode.DISABLED)

    # 2. Env override for budget
    env_budget = os.environ.get("MAX_THINKING_TOKENS")
    budget = budget_override or (int(env_budget) if env_budget else _DEFAULT_BUDGET)
    budget = min(budget, _MAX_BUDGET)

    # 3. Determine mode
    if user_override == ThinkingMode.ADAPTIVE and model_supports_adaptive(model):
        return ThinkingConfig(mode=ThinkingMode.ADAPTIVE, budget_tokens=budget)

    if user_override == ThinkingMode.ENABLED:
        return ThinkingConfig(mode=ThinkingMode.ENABLED, budget_tokens=budget)

    # 4. Default: adaptive for capable models, else enabled
    if model_supports_adaptive(model):
        return ThinkingConfig(mode=ThinkingMode.ADAPTIVE, budget_tokens=budget)

    return ThinkingConfig(mode=ThinkingMode.ENABLED, budget_tokens=budget)


# ---------------------------------------------------------------------------
# Thinking block preservation in conversation history
# ---------------------------------------------------------------------------

def strip_thinking_blocks(messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Remove thinking blocks from message history when switching models.

    CC rule: thinking block signatures are model-bound. When replaying a
    conversation with a different model, thinking blocks must be stripped
    to avoid API errors.
    """
    cleaned = []
    for msg in messages:
        content = msg.get("content")
        if isinstance(content, list):
            # Filter out thinking blocks from content arrays
            filtered = [
                block for block in content
                if not (isinstance(block, dict) and block.get("type") == "thinking")
            ]
            if filtered:
                cleaned.append({**msg, "content": filtered})
        else:
            cleaned.append(msg)
    return cleaned


def has_thinking_blocks(messages: list[dict[str, Any]]) -> bool:
    """Check if any messages contain thinking blocks."""
    for msg in messages:
        content = msg.get("content")
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "thinking":
                    return True
    return False
