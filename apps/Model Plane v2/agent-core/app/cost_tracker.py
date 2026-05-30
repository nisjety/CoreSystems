"""Token and USD cost tracking — accumulate actual LLM usage per run.

Mirrors CC's costTracker.ts: tracks input_tokens, output_tokens,
cache_creation, and cache_read per turn, enforces the token_budget,
and calculates real USD cost using the pricing table in app.pricing.

The CostTracker is instantiated per-run and threaded through the
turn loop. After the run, final totals can be persisted to metadata.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

from app.pricing import calculate_usd

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class TurnUsage:
    """Token usage from a single LLM call."""

    input_tokens: int = 0
    output_tokens: int = 0
    cache_creation_tokens: int = 0
    cache_read_tokens: int = 0
    model: str = ""  # model used for this turn (used for USD cost)

    @property
    def total_tokens(self) -> int:
        return self.input_tokens + self.output_tokens

    @property
    def usd_cost(self) -> float:
        """USD cost for this turn based on the model pricing table."""
        if not self.model:
            return 0.0
        return calculate_usd(self.input_tokens, self.output_tokens, self.model)

    def to_dict(self) -> dict[str, int | float | str]:
        return {
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "cache_creation_tokens": self.cache_creation_tokens,
            "cache_read_tokens": self.cache_read_tokens,
            "total_tokens": self.total_tokens,
            "model": self.model,
            "usd_cost": self.usd_cost,
        }


class BudgetExceeded(Exception):
    """Raised when cumulative token usage exceeds the run budget."""

    def __init__(self, used: int, budget: int) -> None:
        self.used = used
        self.budget = budget
        super().__init__(f"Token budget exceeded: {used} >= {budget}")


@dataclass
class CostTracker:
    """Accumulates token usage across turns within a single run.

    Usage:
        tracker = CostTracker(budget=100_000)
        tracker.record(TurnUsage(input_tokens=500, output_tokens=200))
        tracker.check_budget()  # raises BudgetExceeded if over
        summary = tracker.summary()
    """

    budget: int = 100_000
    turns: list[TurnUsage] = field(default_factory=list)

    @property
    def total_input(self) -> int:
        return sum(t.input_tokens for t in self.turns)

    @property
    def total_output(self) -> int:
        return sum(t.output_tokens for t in self.turns)

    @property
    def total_tokens(self) -> int:
        return sum(t.total_tokens for t in self.turns)

    @property
    def total_cache_creation(self) -> int:
        return sum(t.cache_creation_tokens for t in self.turns)

    @property
    def total_cache_read(self) -> int:
        return sum(t.cache_read_tokens for t in self.turns)

    @property
    def total_usd(self) -> float:
        """Total USD cost across all turns."""
        return round(sum(t.usd_cost for t in self.turns), 8)

    @property
    def utilization(self) -> float:
        """Fraction of budget used (0.0–1.0+)."""
        if self.budget <= 0:
            return 0.0
        return self.total_tokens / self.budget

    def record(self, usage: TurnUsage) -> None:
        """Record usage from a single turn."""
        self.turns.append(usage)
        logger.debug(
            "cost_recorded",
            extra={
                "turn": len(self.turns),
                "input": usage.input_tokens,
                "output": usage.output_tokens,
                "cumulative": self.total_tokens,
                "budget": self.budget,
            },
        )

    def check_budget(self) -> None:
        """Raise BudgetExceeded if cumulative usage meets or exceeds budget."""
        if self.total_tokens >= self.budget:
            raise BudgetExceeded(used=self.total_tokens, budget=self.budget)

    def per_model_breakdown(self) -> dict[str, dict[str, int | float]]:
        """Return per-model token and cost breakdown (CC's formatTotalCost)."""
        breakdown: dict[str, dict[str, int | float]] = {}
        for t in self.turns:
            key = t.model or "unknown"
            entry = breakdown.setdefault(key, {
                "input_tokens": 0, "output_tokens": 0,
                "total_tokens": 0, "usd_cost": 0.0, "turns": 0,
            })
            entry["input_tokens"] += t.input_tokens
            entry["output_tokens"] += t.output_tokens
            entry["total_tokens"] += t.total_tokens
            entry["usd_cost"] = round(entry["usd_cost"] + t.usd_cost, 8)
            entry["turns"] += 1
        return breakdown

    def summary(self) -> dict[str, int | float | dict]:
        """Return a summary dict for persisting to run metadata."""
        return {
            "total_input_tokens": self.total_input,
            "total_output_tokens": self.total_output,
            "total_tokens": self.total_tokens,
            "cache_creation_tokens": self.total_cache_creation,
            "cache_read_tokens": self.total_cache_read,
            "budget": self.budget,
            "utilization": round(self.utilization, 4),
            "turns_tracked": len(self.turns),
            "total_usd": self.total_usd,
            "per_model": self.per_model_breakdown(),
        }

    # ------------------------------------------------------------------
    # Session persistence (CC costTracker.ts save/restore)
    # ------------------------------------------------------------------

    def to_session_state(self) -> dict[str, Any]:
        """Serialize tracker state for session persistence."""
        return {
            "budget": self.budget,
            "turns": [t.to_dict() for t in self.turns],
        }

    @classmethod
    def from_session_state(cls, state: dict[str, Any]) -> "CostTracker":
        """Restore tracker from persisted session state."""
        tracker = cls(budget=state.get("budget", 100_000))
        for turn_data in state.get("turns", []):
            tracker.turns.append(TurnUsage(
                input_tokens=turn_data.get("input_tokens", 0),
                output_tokens=turn_data.get("output_tokens", 0),
                cache_creation_tokens=turn_data.get("cache_creation_tokens", 0),
                cache_read_tokens=turn_data.get("cache_read_tokens", 0),
                model=turn_data.get("model", ""),
            ))
        return tracker


def parse_usage_from_response(response_data: dict, model: str = "") -> TurnUsage:
    """Extract token usage from an LLM response dict.

    Supports the standard format from llm-worker:
    {"content": "...", "usage": {"input_tokens": N, "output_tokens": N, ...},
     "model": "gpt-4o-mini"}

    Falls back to zeros if usage data is not present.
    Args:
        response_data: Raw response dict from the LLM client.
        model: Override model name; if empty, reads ``response_data["model"]``.
    """
    usage = response_data.get("usage") or {}
    resolved_model = model or response_data.get("model", "")
    return TurnUsage(
        input_tokens=usage.get("input_tokens", 0),
        output_tokens=usage.get("output_tokens", 0),
        cache_creation_tokens=usage.get("cache_creation_tokens", 0),
        cache_read_tokens=usage.get("cache_read_tokens", 0),
        model=resolved_model,
    )
