"""Tests for Phase P — USD Cost Tracking + Model Pricing."""

from __future__ import annotations

import pytest

from app.cost_tracker import CostTracker, TurnUsage, parse_usage_from_response
from app.pricing import MODEL_PRICING, calculate_usd, get_pricing


# ---------------------------------------------------------------------------
# Pricing table
# ---------------------------------------------------------------------------


class TestGetPricing:
    def test_known_model_returns_rates(self) -> None:
        prompt, completion = get_pricing("gpt-4o-mini")
        assert prompt > 0
        assert completion > 0

    def test_unknown_model_returns_zero(self) -> None:
        assert get_pricing("totally-unknown-model-xyz") == (0.0, 0.0)

    def test_strips_provider_prefix(self) -> None:
        """'openai/gpt-4o' should resolve to the same as 'gpt-4o'."""
        assert get_pricing("openai/gpt-4o") == get_pricing("gpt-4o")

    def test_all_entries_non_negative(self) -> None:
        for model, (prompt, comp) in MODEL_PRICING.items():
            assert prompt >= 0, f"negative prompt rate for {model}"
            assert comp >= 0, f"negative completion rate for {model}"


class TestCalculateUsd:
    def test_zero_tokens_is_zero_cost(self) -> None:
        assert calculate_usd(0, 0, "gpt-4o-mini") == 0.0

    def test_known_model_cost(self) -> None:
        # gpt-4o-mini: 0.00015 / 1k prompt, 0.0006 / 1k completion
        cost = calculate_usd(1000, 1000, "gpt-4o-mini")
        expected = (0.00015 + 0.0006)
        assert abs(cost - expected) < 1e-8

    def test_unknown_model_zero_cost(self) -> None:
        assert calculate_usd(10_000, 5_000, "no-such-model") == 0.0

    def test_result_rounded(self) -> None:
        # Should not raise and result has finite precision
        cost = calculate_usd(123, 456, "claude-3-5-haiku-latest")
        assert isinstance(cost, float)
        assert cost >= 0


# ---------------------------------------------------------------------------
# TurnUsage.usd_cost
# ---------------------------------------------------------------------------


class TestTurnUsageUsdCost:
    def test_no_model_is_zero(self) -> None:
        u = TurnUsage(input_tokens=1000, output_tokens=500)
        assert u.usd_cost == 0.0

    def test_known_model_cost(self) -> None:
        u = TurnUsage(input_tokens=1000, output_tokens=500, model="gpt-4o-mini")
        assert u.usd_cost > 0

    def test_usd_cost_present_in_to_dict(self) -> None:
        u = TurnUsage(input_tokens=100, output_tokens=50, model="gpt-4o")
        d = u.to_dict()
        assert "usd_cost" in d
        assert isinstance(d["usd_cost"], float)


# ---------------------------------------------------------------------------
# CostTracker.total_usd
# ---------------------------------------------------------------------------


class TestCostTrackerUsd:
    def test_total_usd_zero_on_init(self) -> None:
        ct = CostTracker(budget=100_000)
        assert ct.total_usd == 0.0

    def test_total_usd_accumulates(self) -> None:
        ct = CostTracker(budget=100_000)
        ct.record(TurnUsage(input_tokens=1000, output_tokens=500, model="gpt-4o-mini"))
        ct.record(TurnUsage(input_tokens=500, output_tokens=200, model="gpt-4o-mini"))
        assert ct.total_usd > 0

    def test_total_usd_in_summary(self) -> None:
        ct = CostTracker(budget=50_000)
        ct.record(TurnUsage(input_tokens=100, output_tokens=50, model="gpt-4o"))
        summary = ct.summary()
        assert "total_usd" in summary
        assert isinstance(summary["total_usd"], float)

    def test_no_cross_model_interference(self) -> None:
        """Turns with unknown model don't inflate USD total."""
        ct = CostTracker(budget=100_000)
        ct.record(TurnUsage(input_tokens=5000, output_tokens=2000, model=""))
        assert ct.total_usd == 0.0


# ---------------------------------------------------------------------------
# parse_usage_from_response model passthrough
# ---------------------------------------------------------------------------


class TestParseUsageModel:
    def test_model_from_response_data(self) -> None:
        data = {
            "model": "gpt-4o-mini",
            "usage": {"input_tokens": 100, "output_tokens": 50},
        }
        usage = parse_usage_from_response(data)
        assert usage.model == "gpt-4o-mini"
        assert usage.usd_cost > 0

    def test_model_kwarg_overrides_response(self) -> None:
        data = {
            "model": "gpt-4o-mini",
            "usage": {"input_tokens": 100, "output_tokens": 50},
        }
        usage = parse_usage_from_response(data, model="claude-3-5-haiku-latest")
        assert usage.model == "claude-3-5-haiku-latest"

    def test_missing_usage_key_defaults_zero(self) -> None:
        usage = parse_usage_from_response({})
        assert usage.input_tokens == 0
        assert usage.output_tokens == 0
        assert usage.usd_cost == 0.0
