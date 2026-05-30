"""Token estimation service — count tokens and track cost per session.

Uses tiktoken for OpenAI-compatible models; falls back to character-based
heuristics for other providers (Anthropic, Cohere, etc.).

Design:
- Stateless `estimate()` function for one-off counts
- `SessionCostTracker` for accumulating per-session token+cost state
- No external calls — pure CPU, safe to call in hot paths
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Pricing table (USD per 1M tokens, May 2024 — update as needed)
# ---------------------------------------------------------------------------

_COST_PER_M_INPUT: dict[str, float] = {
    "gpt-4o": 5.00,
    "gpt-4o-mini": 0.15,
    "gpt-4-turbo": 10.00,
    "gpt-4": 30.00,
    "gpt-3.5-turbo": 0.50,
    "claude-3-5-sonnet-20241022": 3.00,
    "claude-3-5-haiku-20241022": 0.80,
    "claude-3-opus-20240229": 15.00,
    "claude-3-sonnet-20240229": 3.00,
    "claude-3-haiku-20240307": 0.25,
}

_COST_PER_M_OUTPUT: dict[str, float] = {
    "gpt-4o": 15.00,
    "gpt-4o-mini": 0.60,
    "gpt-4-turbo": 30.00,
    "gpt-4": 60.00,
    "gpt-3.5-turbo": 1.50,
    "claude-3-5-sonnet-20241022": 15.00,
    "claude-3-5-haiku-20241022": 4.00,
    "claude-3-opus-20240229": 75.00,
    "claude-3-sonnet-20240229": 15.00,
    "claude-3-haiku-20240307": 1.25,
}

_DEFAULT_INPUT_COST = 5.00   # fallback $/1M
_DEFAULT_OUTPUT_COST = 15.00


# ---------------------------------------------------------------------------
# Tokeniser — lazy-load tiktoken; char heuristic fallback
# ---------------------------------------------------------------------------

def _get_tokeniser(model: str) -> Any | None:
    """Return a tiktoken Encoding for the model, or None if unavailable."""
    try:
        import tiktoken
        try:
            return tiktoken.encoding_for_model(model)
        except KeyError:
            return tiktoken.get_encoding("cl100k_base")
    except ImportError:
        return None


def estimate(text: str, model: str = "gpt-4o") -> int:
    """Return estimated token count for *text* under *model*.

    Uses tiktoken when available; falls back to `len(text) // 4`.
    """
    if not text:
        return 0
    enc = _get_tokeniser(model)
    if enc is not None:
        return len(enc.encode(text))
    # Char heuristic: ~4 chars/token for English prose
    return max(1, len(text) // 4)


def estimate_messages(messages: list[dict[str, Any]], model: str = "gpt-4o") -> int:
    """Sum token estimates across a list of {role, content} message dicts."""
    total = 0
    for msg in messages:
        content = msg.get("content") or ""
        if isinstance(content, list):
            # Multi-modal: flatten text parts
            for part in content:
                if isinstance(part, dict) and part.get("type") == "text":
                    total += estimate(part.get("text", ""), model)
        else:
            total += estimate(str(content), model)
        # Per-message overhead (role + separators)
        total += 4
    return total + 2  # conversation framing


def cost_usd(input_tokens: int, output_tokens: int, model: str) -> float:
    """Return estimated cost in USD for the given token counts."""
    input_rate = _COST_PER_M_INPUT.get(model, _DEFAULT_INPUT_COST)
    output_rate = _COST_PER_M_OUTPUT.get(model, _DEFAULT_OUTPUT_COST)
    return (input_tokens * input_rate + output_tokens * output_rate) / 1_000_000


# ---------------------------------------------------------------------------
# Session cost tracker
# ---------------------------------------------------------------------------

@dataclass
class SessionCostTracker:
    """Accumulate token counts and cost for a single session."""

    model: str = "gpt-4o"
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    turn_count: int = 0
    _turn_snapshots: list[dict[str, Any]] = field(default_factory=list)

    def record_turn(
        self,
        input_tokens: int,
        output_tokens: int,
        *,
        model: str | None = None,
    ) -> None:
        """Record token usage for one LLM turn."""
        mdl = model or self.model
        self.total_input_tokens += input_tokens
        self.total_output_tokens += output_tokens
        self.turn_count += 1
        self._turn_snapshots.append(
            {
                "turn": self.turn_count,
                "input": input_tokens,
                "output": output_tokens,
                "model": mdl,
                "cost_usd": cost_usd(input_tokens, output_tokens, mdl),
            }
        )

    @property
    def total_tokens(self) -> int:
        return self.total_input_tokens + self.total_output_tokens

    @property
    def total_cost_usd(self) -> float:
        return cost_usd(self.total_input_tokens, self.total_output_tokens, self.model)

    def summary(self) -> dict[str, Any]:
        return {
            "model": self.model,
            "turns": self.turn_count,
            "input_tokens": self.total_input_tokens,
            "output_tokens": self.total_output_tokens,
            "total_tokens": self.total_tokens,
            "cost_usd": round(self.total_cost_usd, 6),
        }

    def reset(self) -> None:
        self.total_input_tokens = 0
        self.total_output_tokens = 0
        self.turn_count = 0
        self._turn_snapshots.clear()
