"""Token budget — parse user-specified budgets and manage auto-compact thresholds.

Ported from CC's utils/tokenBudget.ts and services/compact/autoCompact.ts.

Features:
- parseTokenBudget(): detect "+500k", "use 2M tokens" in user messages
- TokenEstimator: estimate token counts for messages before API calls
- AutoCompactConfig: context window management thresholds
"""

from __future__ import annotations

import re
from dataclasses import dataclass

# ---------------------------------------------------------------------------
# Token budget parsing (CC tokenBudget.ts)
# ---------------------------------------------------------------------------

# Shorthand: "+500k" or "+2M" at start/end of message
_SHORTHAND_START = re.compile(r"^\s*\+(\d+(?:\.\d+)?)\s*(k|m|b)\b", re.IGNORECASE)
_SHORTHAND_END = re.compile(r"\s\+(\d+(?:\.\d+)?)\s*(k|m|b)\s*[.!?]?\s*$", re.IGNORECASE)
# Verbose: "use 2M tokens" or "spend 500k tokens"
_VERBOSE = re.compile(r"\b(?:use|spend)\s+(\d+(?:\.\d+)?)\s*(k|m|b)\s*tokens?\b", re.IGNORECASE)

_MULTIPLIERS = {"k": 1_000, "m": 1_000_000, "b": 1_000_000_000}


def _parse_match(value: str, suffix: str) -> int:
    return int(float(value) * _MULTIPLIERS[suffix.lower()])


def parse_token_budget(text: str) -> int | None:
    """Parse a token budget from a user message.

    Supports:
        "+500k"          → 500,000
        "+2M"            → 2,000,000
        "use 1.5M tokens" → 1,500,000
    """
    m = _SHORTHAND_START.match(text)
    if m:
        return _parse_match(m.group(1), m.group(2))

    m = _SHORTHAND_END.search(text)
    if m:
        return _parse_match(m.group(1), m.group(2))

    m = _VERBOSE.search(text)
    if m:
        return _parse_match(m.group(1), m.group(2))

    return None


def budget_continuation_message(pct: int, turn_tokens: int, budget: int) -> str:
    """Generate a continuation prompt when budget is partially consumed."""
    fmt = lambda n: f"{n:,}"
    return f"Stopped at {pct}% of token target ({fmt(turn_tokens)} / {fmt(budget)}). Keep working — do not summarize."


# ---------------------------------------------------------------------------
# Token estimation  
# ---------------------------------------------------------------------------

# Average chars per token varies by model. 4 chars/token is a reasonable
# estimate for English text with code (CC uses similar heuristics).
_CHARS_PER_TOKEN = 4.0


@dataclass
class TokenEstimate:
    """Estimated token count for a piece of text or message batch."""

    chars: int
    estimated_tokens: int
    model: str = ""

    @property
    def is_large(self) -> bool:
        """True if the content exceeds 50k tokens."""
        return self.estimated_tokens > 50_000


def estimate_tokens(text: str, model: str = "") -> TokenEstimate:
    """Estimate the token count for a string."""
    chars = len(text)
    estimated = int(chars / _CHARS_PER_TOKEN)
    return TokenEstimate(chars=chars, estimated_tokens=estimated, model=model)


def estimate_messages_tokens(messages: list[dict[str, str]], model: str = "") -> int:
    """Estimate total tokens for a list of messages (role + content)."""
    total = 0
    for msg in messages:
        content = msg.get("content", "")
        total += estimate_tokens(content, model).estimated_tokens
        # Add overhead for message framing (~4 tokens per message)
        total += 4
    return total


# ---------------------------------------------------------------------------
# Auto-compact config (CC autoCompact.ts thresholds)
# ---------------------------------------------------------------------------

# Reserve for output during compaction (CC: MAX_OUTPUT_TOKENS_FOR_SUMMARY = 20_000)
MAX_OUTPUT_TOKENS_FOR_SUMMARY = 20_000

# Buffer before hitting context limit (CC: AUTOCOMPACT_BUFFER_TOKENS = 13_000)
AUTOCOMPACT_BUFFER_TOKENS = 13_000

# Warning threshold buffer (CC: WARNING_THRESHOLD_BUFFER_TOKENS = 20_000)
WARNING_BUFFER_TOKENS = 20_000

# Max consecutive autocompact failures before giving up (CC: 3)
MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3

# Context window sizes per model family
_CONTEXT_WINDOWS: dict[str, int] = {
    "gpt-4o": 128_000,
    "gpt-4o-mini": 128_000,
    "gpt-4-turbo": 128_000,
    "gpt-4": 8_192,
    "gpt-3.5-turbo": 16_385,
    "claude-3-opus": 200_000,
    "claude-3-sonnet": 200_000,
    "claude-3-haiku": 200_000,
    "claude-3.5-sonnet": 200_000,
    "claude-3.5-haiku": 200_000,
    "claude-4-opus": 200_000,
    "claude-4-sonnet": 200_000,
    "claude-sonnet-4": 200_000,
    "claude-opus-4": 200_000,
    "gemini-1.5-pro": 2_000_000,
    "gemini-1.5-flash": 1_000_000,
    "gemini-2.0-flash": 1_000_000,
    "o1": 128_000,
    "o1-mini": 128_000,
    "o3-mini": 200_000,
}


@dataclass
class AutoCompactConfig:
    """Thresholds for auto-compaction per model."""

    context_window: int
    effective_window: int        # context_window - reserved_for_output
    compact_threshold: int       # trigger compaction at this token count
    warning_threshold: int       # warn user they're approaching limit
    consecutive_failures: int = 0

    @property
    def should_compact(self) -> bool:
        return self.consecutive_failures < MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES

    def record_failure(self) -> None:
        self.consecutive_failures += 1

    def record_success(self) -> None:
        self.consecutive_failures = 0


def get_context_window(model: str) -> int:
    """Get the context window size for a model."""
    # Try exact match first
    if model in _CONTEXT_WINDOWS:
        return _CONTEXT_WINDOWS[model]
    # Try prefix match (e.g. "claude-3.5-sonnet-20241022" → "claude-3.5-sonnet")
    for prefix, window in _CONTEXT_WINDOWS.items():
        if model.startswith(prefix):
            return window
    # Default: 128k is a safe assumption for modern models
    return 128_000


def get_auto_compact_config(model: str, custom_window: int | None = None) -> AutoCompactConfig:
    """Build auto-compact thresholds for a model.

    Mirrors CC's getAutoCompactThreshold() + getEffectiveContextWindowSize().
    """
    context_window = custom_window or get_context_window(model)
    reserved = min(MAX_OUTPUT_TOKENS_FOR_SUMMARY, 20_000)  # max output tokens for summary
    effective = context_window - reserved
    compact_threshold = effective - AUTOCOMPACT_BUFFER_TOKENS
    warning_threshold = effective - WARNING_BUFFER_TOKENS

    return AutoCompactConfig(
        context_window=context_window,
        effective_window=effective,
        compact_threshold=compact_threshold,
        warning_threshold=warning_threshold,
    )
