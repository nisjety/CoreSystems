"""Tests for Phase S — Token Cost Tracking."""

from __future__ import annotations

import pytest

from app.cost_tracker import (
    BudgetExceeded,
    CostTracker,
    TurnUsage,
    parse_usage_from_response,
)


# ---------------------------------------------------------------------------
# TurnUsage
# ---------------------------------------------------------------------------


class TestTurnUsage:
    def test_total_tokens(self) -> None:
        u = TurnUsage(input_tokens=100, output_tokens=50)
        assert u.total_tokens == 150

    def test_total_excludes_cache(self) -> None:
        u = TurnUsage(
            input_tokens=100,
            output_tokens=50,
            cache_creation_tokens=10,
            cache_read_tokens=20,
        )
        # total_tokens is input + output only
        assert u.total_tokens == 150
        assert u.cache_creation_tokens == 10
        assert u.cache_read_tokens == 20

    def test_frozen(self) -> None:
        u = TurnUsage(input_tokens=1, output_tokens=2)
        with pytest.raises(AttributeError):
            u.input_tokens = 99  # type: ignore[misc]

    def test_defaults(self) -> None:
        u = TurnUsage(input_tokens=0, output_tokens=0)
        assert u.cache_creation_tokens == 0
        assert u.cache_read_tokens == 0


# ---------------------------------------------------------------------------
# CostTracker
# ---------------------------------------------------------------------------


class TestCostTracker:
    def test_initial_state(self) -> None:
        ct = CostTracker(budget=10_000)
        assert ct.total_tokens == 0
        assert len(ct.turns) == 0
        assert ct.utilization == 0.0

    def test_record_accumulates(self) -> None:
        ct = CostTracker(budget=10_000)
        ct.record(TurnUsage(input_tokens=100, output_tokens=50))
        ct.record(TurnUsage(input_tokens=200, output_tokens=100))
        assert ct.total_tokens == 450
        assert len(ct.turns) == 2

    def test_check_budget_within(self) -> None:
        ct = CostTracker(budget=10_000)
        ct.record(TurnUsage(input_tokens=100, output_tokens=50))
        ct.check_budget()  # Should not raise

    def test_check_budget_exceeded(self) -> None:
        ct = CostTracker(budget=100)
        ct.record(TurnUsage(input_tokens=80, output_tokens=30))
        with pytest.raises(BudgetExceeded):
            ct.check_budget()

    def test_utilization(self) -> None:
        ct = CostTracker(budget=1000)
        ct.record(TurnUsage(input_tokens=250, output_tokens=250))
        assert ct.utilization == pytest.approx(0.5)

    def test_summary(self) -> None:
        ct = CostTracker(budget=10_000)
        ct.record(TurnUsage(input_tokens=100, output_tokens=50))
        s = ct.summary()
        assert s["total_tokens"] == 150
        assert s["budget"] == 10_000
        assert s["turns_tracked"] == 1
        assert "utilization" in s

    def test_zero_budget_always_exceeds(self) -> None:
        ct = CostTracker(budget=0)
        ct.record(TurnUsage(input_tokens=1, output_tokens=1))
        with pytest.raises(BudgetExceeded):
            ct.check_budget()


# ---------------------------------------------------------------------------
# parse_usage_from_response
# ---------------------------------------------------------------------------


class TestParseUsageFromResponse:
    def test_standard_response(self) -> None:
        data = {
            "content": "hello",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
            },
        }
        u = parse_usage_from_response(data)
        assert u.input_tokens == 100
        assert u.output_tokens == 50

    def test_with_cache_tokens(self) -> None:
        data = {
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_creation_tokens": 10,
                "cache_read_tokens": 20,
            },
        }
        u = parse_usage_from_response(data)
        assert u.cache_creation_tokens == 10
        assert u.cache_read_tokens == 20

    def test_missing_usage(self) -> None:
        data = {"content": "hello"}
        u = parse_usage_from_response(data)
        assert u.total_tokens == 0

    def test_empty_usage(self) -> None:
        u = parse_usage_from_response({"usage": {}})
        assert u.total_tokens == 0

    def test_empty_dict(self) -> None:
        u = parse_usage_from_response({})
        assert u.total_tokens == 0


# ---------------------------------------------------------------------------
# per_model_breakdown (Phase S extension)
# ---------------------------------------------------------------------------


class TestPerModelBreakdown:
    def test_empty_breakdown(self) -> None:
        ct = CostTracker()
        assert ct.per_model_breakdown() == {}

    def test_single_model(self) -> None:
        ct = CostTracker()
        ct.record(TurnUsage(input_tokens=100, output_tokens=50, model="claude-haiku"))
        ct.record(TurnUsage(input_tokens=200, output_tokens=80, model="claude-haiku"))
        breakdown = ct.per_model_breakdown()
        assert "claude-haiku" in breakdown
        entry = breakdown["claude-haiku"]
        assert entry["input_tokens"] == 300
        assert entry["output_tokens"] == 130
        assert entry["turns"] == 2

    def test_multi_model(self) -> None:
        ct = CostTracker()
        ct.record(TurnUsage(input_tokens=50, output_tokens=20, model="haiku"))
        ct.record(TurnUsage(input_tokens=100, output_tokens=40, model="sonnet"))
        breakdown = ct.per_model_breakdown()
        assert set(breakdown.keys()) == {"haiku", "sonnet"}


# ---------------------------------------------------------------------------
# to_session_state / from_session_state (Phase S persistence)
# ---------------------------------------------------------------------------


class TestSessionStateSerialization:
    def test_round_trip_empty(self) -> None:
        ct = CostTracker(budget=5000)
        state = ct.to_session_state()
        restored = CostTracker.from_session_state(state)
        assert restored.total_tokens == 0
        assert restored.total_input == 0
        assert restored.total_output == 0

    def test_round_trip_with_data(self) -> None:
        ct = CostTracker(budget=10_000)
        ct.record(TurnUsage(input_tokens=300, output_tokens=150))
        ct.record(TurnUsage(input_tokens=200, output_tokens=100))
        state = ct.to_session_state()
        restored = CostTracker.from_session_state(state)
        assert restored.total_input == ct.total_input
        assert restored.total_output == ct.total_output
        assert restored.total_tokens == ct.total_tokens

    def test_state_is_serializable(self) -> None:
        import json

        ct = CostTracker()
        ct.record(TurnUsage(input_tokens=10, output_tokens=5))
        state = ct.to_session_state()
        # Must be JSON-serializable
        json.dumps(state)

