"""Prompt cache optimization — CC-style prompt caching strategy.

Manages cache_control injection on system prompts and tool definitions,
detects cache breaks via cache_read_input_tokens drops, and supports
environment overrides to disable caching per-model.
"""

from __future__ import annotations

import logging
import os
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)

# Environment overrides
ENV_DISABLE_ALL = "DISABLE_PROMPT_CACHING"


class CacheControl(BaseModel):
    """Cache control directive for LLM messages."""

    type: str = "ephemeral"
    ttl: str | None = None  # e.g. "1h", "5m"
    scope: str | None = None  # "global" | "session"


class CacheMetrics(BaseModel):
    """Cache performance metrics for a session."""

    cache_hits: int = 0
    cache_misses: int = 0
    total_cache_read_tokens: int = 0
    total_cache_creation_tokens: int = 0
    cache_breaks_detected: int = 0

    @property
    def hit_rate(self) -> float:
        total = self.cache_hits + self.cache_misses
        if total == 0:
            return 0.0
        return self.cache_hits / total


class CacheBreakEvent(BaseModel):
    """Recorded when cache utilization drops significantly."""

    turn_index: int
    previous_read_tokens: int
    current_read_tokens: int
    drop_ratio: float


class PromptCacheManager:
    """Manages prompt-cache optimization for LLM calls.

    Responsibilities:
    1. Inject cache_control on system prompts and tool definitions
    2. Detect cache breaks (>50% drop in cache_read_input_tokens)
    3. Track cache metrics
    4. Respect environment overrides
    """

    BREAK_THRESHOLD: float = 0.5  # >50% drop = cache break

    def __init__(self, enabled: bool = True) -> None:
        self._enabled = enabled and not _is_globally_disabled()
        self._metrics = CacheMetrics()
        self._disabled_models: set[str] = _load_disabled_models()
        self._last_cache_read: int | None = None
        self._breaks: list[CacheBreakEvent] = []
        self._turn_index: int = 0

    @property
    def enabled(self) -> bool:
        return self._enabled

    @property
    def metrics(self) -> CacheMetrics:
        return self._metrics

    @property
    def breaks(self) -> list[CacheBreakEvent]:
        return list(self._breaks)

    def is_enabled_for_model(self, model: str) -> bool:
        """Check if caching is enabled for a specific model."""
        if not self._enabled:
            return False
        env_key = f"DISABLE_PROMPT_CACHING_{model.upper().replace('-', '_').replace('.', '_')}"
        if os.environ.get(env_key, "").lower() in ("1", "true", "yes"):
            return False
        return model not in self._disabled_models

    def inject_cache_control(
        self,
        messages: list[dict[str, Any]],
        model: str,
        *,
        cache_tools: bool = True,
    ) -> list[dict[str, Any]]:
        """Inject cache_control on system messages and optionally tool defs.

        Returns a new list — does not mutate the original.
        """
        if not self.is_enabled_for_model(model):
            return messages

        result: list[dict[str, Any]] = []
        for msg in messages:
            new_msg = dict(msg)
            if msg.get("role") == "system" and "cache_control" not in msg:
                new_msg["cache_control"] = {"type": "ephemeral"}
            result.append(new_msg)
        return result

    def inject_tool_cache_control(
        self,
        tools: list[dict[str, Any]],
        model: str,
    ) -> list[dict[str, Any]]:
        """Inject cache_control on tool definitions.

        Caches the last tool definition in the list (CC pattern).
        Returns a new list.
        """
        if not self.is_enabled_for_model(model) or not tools:
            return tools

        result = [dict(t) for t in tools]
        # Cache the last tool definition per CC pattern
        last = result[-1]
        if "cache_control" not in last:
            result[-1] = {**last, "cache_control": {"type": "ephemeral"}}
        return result

    def record_usage(
        self,
        cache_read_tokens: int,
        cache_creation_tokens: int,
    ) -> CacheBreakEvent | None:
        """Record cache token usage from a turn and detect breaks.

        Returns a CacheBreakEvent if a break is detected.
        """
        self._turn_index += 1
        self._metrics.total_cache_read_tokens += cache_read_tokens
        self._metrics.total_cache_creation_tokens += cache_creation_tokens

        if cache_read_tokens > 0:
            self._metrics.cache_hits += 1
        else:
            self._metrics.cache_misses += 1

        # Detect cache break
        event: CacheBreakEvent | None = None
        if self._last_cache_read is not None and self._last_cache_read > 0:
            if cache_read_tokens == 0 or (
                cache_read_tokens / self._last_cache_read
            ) < self.BREAK_THRESHOLD:
                drop_ratio = (
                    1.0 - (cache_read_tokens / self._last_cache_read)
                    if self._last_cache_read > 0
                    else 1.0
                )
                event = CacheBreakEvent(
                    turn_index=self._turn_index,
                    previous_read_tokens=self._last_cache_read,
                    current_read_tokens=cache_read_tokens,
                    drop_ratio=drop_ratio,
                )
                self._breaks.append(event)
                self._metrics.cache_breaks_detected += 1
                logger.warning(
                    "cache_break_detected",
                    extra={
                        "turn": self._turn_index,
                        "prev": self._last_cache_read,
                        "curr": cache_read_tokens,
                        "drop": round(drop_ratio, 2),
                    },
                )

        self._last_cache_read = cache_read_tokens
        return event


def _is_globally_disabled() -> bool:
    return os.environ.get(ENV_DISABLE_ALL, "").lower() in ("1", "true", "yes")


def _load_disabled_models() -> set[str]:
    """Load model-specific disable flags from environment."""
    disabled: set[str] = set()
    prefix = "DISABLE_PROMPT_CACHING_"
    for key, val in os.environ.items():
        if key.startswith(prefix) and val.lower() in ("1", "true", "yes"):
            model_part = key[len(prefix):].lower().replace("_", "-")
            disabled.add(model_part)
    return disabled
