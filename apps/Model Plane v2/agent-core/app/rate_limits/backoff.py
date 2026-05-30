"""Exponential backoff with jitter for rate-limited retries.

CC pattern: 500ms × 2^attempt, capped, with random jitter.
"""

from __future__ import annotations

import random


# Defaults matching CC behavior
BASE_DELAY_MS: float = 500.0
MAX_DELAY_MS: float = 60_000.0  # 1 minute cap
MAX_RETRIES: int = 5
JITTER_FACTOR: float = 0.25  # ±25% jitter


def compute_backoff_ms(
    attempt: int,
    *,
    base_ms: float = BASE_DELAY_MS,
    max_ms: float = MAX_DELAY_MS,
    jitter: float = JITTER_FACTOR,
) -> float:
    """Compute delay in ms for the given attempt (0-indexed).

    Formula: min(base × 2^attempt, max) × (1 ± jitter)
    """
    delay = base_ms * (2 ** attempt)
    delay = min(delay, max_ms)

    if jitter > 0:
        jitter_range = delay * jitter
        delay += random.uniform(-jitter_range, jitter_range)

    return max(0.0, delay)


def should_retry(attempt: int, max_retries: int = MAX_RETRIES) -> bool:
    """Check if another retry should be attempted."""
    return attempt < max_retries


def compute_backoff_seconds(
    attempt: int,
    *,
    base_ms: float = BASE_DELAY_MS,
    max_ms: float = MAX_DELAY_MS,
    jitter: float = JITTER_FACTOR,
) -> float:
    """Convenience: backoff in seconds (for asyncio.sleep)."""
    return compute_backoff_ms(attempt, base_ms=base_ms, max_ms=max_ms, jitter=jitter) / 1000.0
