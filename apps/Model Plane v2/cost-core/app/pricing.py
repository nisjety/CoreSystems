"""Canonical model pricing (USD per 1M tokens).

Verbatim port from agent-core/app/pricing.py.
"""
from __future__ import annotations

from decimal import Decimal, ROUND_HALF_UP
from typing import Dict, Tuple

# Prices are USD per 1,000,000 tokens.
MODEL_PRICING: Dict[str, Dict[str, Decimal]] = {
    # Anthropic
    "claude-opus-4-5": {"input": Decimal("15.00"), "output": Decimal("75.00")},
    "claude-sonnet-4-5": {"input": Decimal("3.00"), "output": Decimal("15.00")},
    "claude-haiku-4-5": {"input": Decimal("1.00"), "output": Decimal("5.00")},
    "claude-3-5-sonnet": {"input": Decimal("3.00"), "output": Decimal("15.00")},
    "claude-3-5-haiku": {"input": Decimal("0.80"), "output": Decimal("4.00")},
    # OpenAI
    "gpt-4o": {"input": Decimal("2.50"), "output": Decimal("10.00")},
    "gpt-4o-mini": {"input": Decimal("0.15"), "output": Decimal("0.60")},
    "gpt-4-turbo": {"input": Decimal("10.00"), "output": Decimal("30.00")},
    "o1": {"input": Decimal("15.00"), "output": Decimal("60.00")},
    "o1-mini": {"input": Decimal("3.00"), "output": Decimal("12.00")},
    # Google
    "gemini-1.5-pro": {"input": Decimal("1.25"), "output": Decimal("5.00")},
    "gemini-1.5-flash": {"input": Decimal("0.075"), "output": Decimal("0.30")},
    "gemini-2.0-flash": {"input": Decimal("0.10"), "output": Decimal("0.40")},
}

DEFAULT_PRICING: Dict[str, Decimal] = {
    "input": Decimal("3.00"),
    "output": Decimal("15.00"),
}


def get_pricing(model: str) -> Tuple[Decimal, Decimal]:
    """Return (input_per_1m, output_per_1m) for a model."""
    p = MODEL_PRICING.get(model)
    if p is None:
        # Fallback: try prefix match (e.g. "claude-sonnet-4-5-20250101")
        for key, value in MODEL_PRICING.items():
            if model.startswith(key):
                p = value
                break
    if p is None:
        p = DEFAULT_PRICING
    return p["input"], p["output"]


def calculate_usd(
    model: str,
    input_tokens: int,
    output_tokens: int,
) -> Decimal:
    """Compute USD cost for a turn. Rounded half-up to 8 decimal places."""
    input_price, output_price = get_pricing(model)
    million = Decimal("1000000")
    cost = (
        (Decimal(input_tokens) / million) * input_price
        + (Decimal(output_tokens) / million) * output_price
    )
    return cost.quantize(Decimal("0.00000001"), rounding=ROUND_HALF_UP)
