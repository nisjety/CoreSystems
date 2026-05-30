"""Run and turn timeout enforcement — CC queryLoop timeout pattern.

Provides:
- per-turn timeout wrapping for LLM calls
- per-run wall-clock deadline tracked across turns
- graceful timeout handling with structured errors

Constants:
- DEFAULT_TURN_TIMEOUT: max seconds for a single LLM call (120s)
- DEFAULT_RUN_TIMEOUT: max wall-clock seconds for an entire run (600s)
"""

from __future__ import annotations

import asyncio
import logging
import time
from dataclasses import dataclass

logger = logging.getLogger(__name__)

DEFAULT_TURN_TIMEOUT: float = 120.0  # seconds
DEFAULT_RUN_TIMEOUT: float = 600.0  # seconds


class TurnTimeoutError(Exception):
    """Raised when a single LLM call exceeds the per-turn timeout."""

    def __init__(self, turn_index: int, timeout: float) -> None:
        self.turn_index = turn_index
        self.timeout = timeout
        super().__init__(f"Turn {turn_index} timed out after {timeout}s")


class RunDeadlineExceeded(Exception):
    """Raised when the entire run exceeds its wall-clock deadline."""

    def __init__(self, elapsed: float, deadline: float) -> None:
        self.elapsed = elapsed
        self.deadline = deadline
        super().__init__(f"Run deadline exceeded: {elapsed:.1f}s > {deadline}s")


@dataclass
class RunClock:
    """Tracks wall-clock time for a run and enforces deadline.

    Usage:
        clock = RunClock(deadline=600.0)
        clock.check()            # raises RunDeadlineExceeded if over
        remaining = clock.remaining()  # seconds left
    """

    deadline: float
    _started_at: float = 0.0

    def __post_init__(self) -> None:
        self._started_at = time.monotonic()

    def elapsed(self) -> float:
        """Seconds elapsed since clock start."""
        return time.monotonic() - self._started_at

    def remaining(self) -> float:
        """Seconds remaining before deadline. May be negative."""
        return self.deadline - self.elapsed()

    def check(self) -> None:
        """Raise RunDeadlineExceeded if deadline passed."""
        e = self.elapsed()
        if e >= self.deadline:
            raise RunDeadlineExceeded(elapsed=e, deadline=self.deadline)

    def turn_timeout(self, default: float = DEFAULT_TURN_TIMEOUT) -> float:
        """Return timeout for next turn, capped by remaining run time."""
        rem = self.remaining()
        if rem <= 0:
            raise RunDeadlineExceeded(
                elapsed=self.elapsed(), deadline=self.deadline
            )
        return min(default, rem)


async def with_turn_timeout(
    coro,  # noqa: ANN001
    *,
    turn_index: int,
    timeout: float = DEFAULT_TURN_TIMEOUT,
) -> str:
    """Wrap an async LLM call with a per-turn timeout.

    Args:
        coro: The awaitable LLM call (e.g. llm_client.planner_complete(msgs)).
        turn_index: Current turn number (for error context).
        timeout: Max seconds to wait.

    Returns:
        The LLM response string.

    Raises:
        TurnTimeoutError: If the call exceeds the timeout.
    """
    try:
        return await asyncio.wait_for(coro, timeout=timeout)
    except asyncio.TimeoutError:
        logger.warning(
            "turn_timeout",
            extra={"turn_index": turn_index, "timeout": timeout},
        )
        raise TurnTimeoutError(turn_index=turn_index, timeout=timeout)
