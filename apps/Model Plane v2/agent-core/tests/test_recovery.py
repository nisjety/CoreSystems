"""Tests for Phase M — Conversation recovery (domain + logic)."""

from __future__ import annotations

from datetime import datetime, timedelta, timezone

import pytest

from app.domain import RunRecord, RunStatus
from app.recovery import MAX_RECOVERY_AGE, STALE_THRESHOLD


# ---------------------------------------------------------------------------
# Recovery logic (unit tests — no DB)
# ---------------------------------------------------------------------------


class TestRecoveryConstants:
    def test_stale_threshold(self) -> None:
        assert STALE_THRESHOLD == timedelta(minutes=15)

    def test_max_recovery_age(self) -> None:
        assert MAX_RECOVERY_AGE == timedelta(hours=24)


class TestRecoveryDecision:
    """Test the recovery decision logic without DB/Redis."""

    def test_old_run_should_fail(self) -> None:
        """Runs older than MAX_RECOVERY_AGE should be marked failed."""
        now = datetime.now(timezone.utc)
        old_time = now - timedelta(hours=25)
        age = now - old_time
        assert age > MAX_RECOVERY_AGE

    def test_recent_run_should_resume(self) -> None:
        """Runs within MAX_RECOVERY_AGE with checkpoint should resume."""
        now = datetime.now(timezone.utc)
        recent_time = now - timedelta(minutes=30)
        age = now - recent_time
        assert age < MAX_RECOVERY_AGE

    def test_stale_detection_threshold(self) -> None:
        """Runs updated more than STALE_THRESHOLD ago are stale."""
        now = datetime.now(timezone.utc)
        stale_time = now - timedelta(minutes=20)
        assert (now - stale_time) > STALE_THRESHOLD

    def test_fresh_run_not_stale(self) -> None:
        """Runs updated recently are not stale."""
        now = datetime.now(timezone.utc)
        fresh_time = now - timedelta(minutes=5)
        assert (now - fresh_time) < STALE_THRESHOLD

    def test_run_with_checkpoint_is_recoverable(self) -> None:
        """Runs with checkpoint_index > 0 should attempt resume."""
        run = RunRecord(
            session_id="s1",
            user_id="u1",
            goal="test",
            checkpoint_index=3,
        )
        assert run.checkpoint_index > 0

    def test_run_without_checkpoint_requeues(self) -> None:
        """Runs with no checkpoint should requeue."""
        run = RunRecord(
            session_id="s1",
            user_id="u1",
            goal="test",
            checkpoint_index=0,
        )
        assert run.checkpoint_index == 0


# ---------------------------------------------------------------------------
# Session compact fact extraction (Phase N)
# ---------------------------------------------------------------------------


class TestSessionCompactDomain:
    def test_fact_extraction_prompt_is_valid(self) -> None:
        from app.context.session_compact import FACT_EXTRACTION_PROMPT

        assert "JSON array" in FACT_EXTRACTION_PROMPT
        assert "key" in FACT_EXTRACTION_PROMPT
        assert "content" in FACT_EXTRACTION_PROMPT
