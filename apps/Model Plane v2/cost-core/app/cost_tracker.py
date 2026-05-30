"""Turn-level cost tracker and usage parser.

Ported verbatim (behavior-preserving) from agent-core/app/cost_tracker.py.
"""
from __future__ import annotations

import logging
from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any, Dict, List, Optional

from app.pricing import calculate_usd

logger = logging.getLogger(__name__)


class BudgetExceeded(Exception):
    """Raised when a run exceeds its configured cost budget."""


@dataclass(frozen=True)
class TurnUsage:
    model: str
    input_tokens: int
    output_tokens: int
    cost_usd: Decimal

    @classmethod
    def from_tokens(
        cls,
        model: str,
        input_tokens: int,
        output_tokens: int,
    ) -> "TurnUsage":
        return cls(
            model=model,
            input_tokens=int(input_tokens or 0),
            output_tokens=int(output_tokens or 0),
            cost_usd=calculate_usd(model, input_tokens or 0, output_tokens or 0),
        )


@dataclass
class CostTracker:
    run_id: str
    org_id: str
    budget_usd: Optional[Decimal] = None
    turns: List[TurnUsage] = field(default_factory=list)

    @property
    def total_cost_usd(self) -> Decimal:
        total = Decimal("0")
        for t in self.turns:
            total += t.cost_usd
        return total

    @property
    def total_input_tokens(self) -> int:
        return sum(t.input_tokens for t in self.turns)

    @property
    def total_output_tokens(self) -> int:
        return sum(t.output_tokens for t in self.turns)

    def record(self, usage: TurnUsage) -> None:
        self.turns.append(usage)
        if self.budget_usd is not None and self.total_cost_usd > self.budget_usd:
            raise BudgetExceeded(
                f"run {self.run_id} exceeded budget "
                f"{self.budget_usd} USD (current {self.total_cost_usd})"
            )


def parse_usage_from_response(
    model: str,
    response: Dict[str, Any],
) -> TurnUsage:
    """Extract token usage from a provider response dict.

    Supports Anthropic, OpenAI, and Google Gemini shapes.
    """
    usage = response.get("usage") or {}

    # Anthropic shape
    input_tokens = usage.get("input_tokens")
    output_tokens = usage.get("output_tokens")

    # OpenAI shape
    if input_tokens is None and "prompt_tokens" in usage:
        input_tokens = usage.get("prompt_tokens")
        output_tokens = usage.get("completion_tokens")

    # Gemini shape
    if input_tokens is None and "promptTokenCount" in usage:
        input_tokens = usage.get("promptTokenCount")
        output_tokens = usage.get("candidatesTokenCount")

    # Gemini alt shape: usageMetadata
    if input_tokens is None:
        meta = response.get("usageMetadata") or {}
        input_tokens = meta.get("promptTokenCount")
        output_tokens = meta.get("candidatesTokenCount")

    return TurnUsage.from_tokens(
        model=model,
        input_tokens=int(input_tokens or 0),
        output_tokens=int(output_tokens or 0),
    )
