"""Token estimation — lightweight char-ratio heuristic.

CC uses tiktoken for exact token counts, but we avoid the heavy dependency.
Instead we use a character-to-token ratio (roughly 4 chars ≈ 1 token for
English text, which is the standard GPT approximation).

Provides:
- estimate_tokens(text) → int
- estimate_messages_tokens(messages) → int
- fits_in_budget(messages, budget) → bool
"""

from __future__ import annotations

from typing import Any

# Average chars per token for English text (GPT-family models).
# Conservative estimate — slightly overcount to avoid overflows.
CHARS_PER_TOKEN = 3.8

# Per-message overhead in tokens (role label + formatting)
MESSAGE_OVERHEAD = 4


def estimate_tokens(text: str) -> int:
    """Estimate token count for a single string."""
    if not text:
        return 0
    return max(1, int(len(text) / CHARS_PER_TOKEN))


def estimate_messages_tokens(messages: list[dict[str, Any]]) -> int:
    """Estimate total tokens across a message list.

    Accounts for per-message overhead (role headers, formatting).
    Follows the OpenAI message token counting convention.
    """
    total = 0
    for msg in messages:
        content = msg.get("content", "")
        total += estimate_tokens(content) + MESSAGE_OVERHEAD
    # Add 2 for the assistant reply priming
    total += 2
    return total


def fits_in_budget(
    messages: list[dict[str, Any]],
    budget: int,
) -> bool:
    """Check whether messages fit within a token budget."""
    return estimate_messages_tokens(messages) <= budget
