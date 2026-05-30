"""Model pricing table — USD cost per 1,000 tokens.

Prices are (prompt_per_1k, completion_per_1k) in USD.
Unknown models default to (0.0, 0.0) — no cost charged.

Source: provider pricing pages as of April 2026.
Extend this dict as new models are deployed.
"""

from __future__ import annotations

# (prompt_usd_per_1k_tokens, completion_usd_per_1k_tokens)
MODEL_PRICING: dict[str, tuple[float, float]] = {
    # --- Anthropic ---
    "claude-3-5-sonnet-20241022": (0.003, 0.015),
    "claude-3-5-sonnet-latest": (0.003, 0.015),
    "claude-3-5-haiku-20241022": (0.0008, 0.004),
    "claude-3-5-haiku-latest": (0.0008, 0.004),
    "claude-3-opus-20240229": (0.015, 0.075),
    "claude-3-haiku-20240307": (0.00025, 0.00125),
    "claude-3-sonnet-20240229": (0.003, 0.015),
    "claude-sonnet-4-5": (0.003, 0.015),
    "claude-opus-4-5": (0.015, 0.075),
    # --- OpenAI ---
    "gpt-4o": (0.0025, 0.01),
    "gpt-4o-mini": (0.00015, 0.0006),
    "gpt-4-turbo": (0.01, 0.03),
    "gpt-4": (0.03, 0.06),
    "gpt-3.5-turbo": (0.0005, 0.0015),
    "o1": (0.015, 0.06),
    "o1-mini": (0.003, 0.012),
    "o3-mini": (0.0011, 0.0044),
    # --- Google ---
    "gemini-1.5-pro": (0.00125, 0.005),
    "gemini-1.5-flash": (0.000075, 0.0003),
    "gemini-2.0-flash": (0.0001, 0.0004),
    "gemini-2.5-pro": (0.00125, 0.01),
}

_FALLBACK: tuple[float, float] = (0.0, 0.0)


def get_pricing(model: str) -> tuple[float, float]:
    """Return (prompt_per_1k, completion_per_1k) for *model*.

    Falls back to (0.0, 0.0) for unknown models so cost is never
    negative or raises — it simply isn't charged.
    Strips provider prefixes like "openai/" or "anthropic/".
    """
    # Strip provider prefix (e.g. "openai/gpt-4o" → "gpt-4o")
    clean = model.split("/")[-1].lower()
    # Try exact match first, then cleaned
    return MODEL_PRICING.get(model) or MODEL_PRICING.get(clean, _FALLBACK)


def calculate_usd(
    input_tokens: int,
    output_tokens: int,
    model: str,
) -> float:
    """Return USD cost for a single LLM call.

    Args:
        input_tokens: Prompt tokens billed (includes cache read tokens
            when the provider charges for those at the prompt rate).
        output_tokens: Completion tokens billed.
        model: Model identifier string.

    Returns:
        Cost in USD, rounded to 8 decimal places.
    """
    prompt_rate, completion_rate = get_pricing(model)
    cost = (input_tokens * prompt_rate + output_tokens * completion_rate) / 1000.0
    return round(cost, 8)
