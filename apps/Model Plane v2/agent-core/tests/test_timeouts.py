"""Tests for Phase R — Run + Turn Timeouts."""

from __future__ import annotations

import asyncio
import time

import pytest

from app.timeouts import (
    DEFAULT_RUN_TIMEOUT,
    DEFAULT_TURN_TIMEOUT,
    RunClock,
    RunDeadlineExceeded,
    TurnTimeoutError,
    with_turn_timeout,
)


# ---------------------------------------------------------------------------
# RunClock
# ---------------------------------------------------------------------------


class TestRunClock:
    def test_explicit_deadline(self) -> None:
        clock = RunClock(deadline=DEFAULT_RUN_TIMEOUT)
        assert clock.deadline == DEFAULT_RUN_TIMEOUT

    def test_custom_deadline(self) -> None:
        clock = RunClock(deadline=30.0)
        assert clock.deadline == 30.0

    def test_elapsed_increases(self) -> None:
        clock = RunClock(deadline=600.0)
        t0 = clock.elapsed()
        # elapsed should be very small at start
        assert t0 < 1.0

    def test_remaining_decreases_over_time(self) -> None:
        clock = RunClock(deadline=600.0)
        r = clock.remaining()
        assert r > 0
        assert r <= 600.0

    def test_check_within_deadline(self) -> None:
        clock = RunClock(deadline=600.0)
        # Should not raise
        clock.check()

    def test_check_past_deadline_raises(self) -> None:
        clock = RunClock(deadline=0.0)
        # Immediately past deadline
        with pytest.raises(RunDeadlineExceeded):
            clock.check()

    def test_turn_timeout_capped_by_remaining(self) -> None:
        clock = RunClock(deadline=5.0)
        tt = clock.turn_timeout(default=120.0)
        assert tt <= 5.0

    def test_turn_timeout_uses_default_when_ample_time(self) -> None:
        clock = RunClock(deadline=600.0)
        tt = clock.turn_timeout(default=30.0)
        assert tt == 30.0


# ---------------------------------------------------------------------------
# TurnTimeoutError
# ---------------------------------------------------------------------------


class TestTurnTimeoutError:
    def test_message(self) -> None:
        err = TurnTimeoutError(turn_index=3, timeout=120.0)
        assert "Turn 3" in str(err)
        assert "120" in str(err)


# ---------------------------------------------------------------------------
# RunDeadlineExceeded
# ---------------------------------------------------------------------------


class TestRunDeadlineExceeded:
    def test_message(self) -> None:
        err = RunDeadlineExceeded(elapsed=605.5, deadline=600.0)
        assert "605" in str(err)
        assert "600" in str(err)


# ---------------------------------------------------------------------------
# with_turn_timeout
# ---------------------------------------------------------------------------


class TestWithTurnTimeout:
    @pytest.mark.asyncio
    async def test_completes_within_timeout(self) -> None:
        async def fast() -> str:
            return "ok"

        result = await with_turn_timeout(fast(), turn_index=0, timeout=5.0)
        assert result == "ok"

    @pytest.mark.asyncio
    async def test_raises_on_timeout(self) -> None:
        async def slow() -> str:
            await asyncio.sleep(10)
            return "never"

        with pytest.raises(TurnTimeoutError):
            await with_turn_timeout(slow(), turn_index=1, timeout=0.05)

    @pytest.mark.asyncio
    async def test_propagates_inner_exception(self) -> None:
        async def failing() -> str:
            raise ValueError("boom")

        with pytest.raises(ValueError, match="boom"):
            await with_turn_timeout(failing(), turn_index=0, timeout=5.0)
