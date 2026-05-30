"""Tests for provider_research comparison utilities."""

from __future__ import annotations

import pytest

from provider_research.comparison import ProviderComparison, compare_providers
from provider_research.profiles import ModelProfile, ModelTier, ProviderProfile


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def _make_model(
    model_id: str = "m1",
    name: str = "Model One",
    tier: ModelTier = ModelTier.standard,
    input_cost: float = 0.01,
    output_cost: float = 0.02,
    context_window: int = 128_000,
    streaming: bool = True,
    tools: bool = True,
) -> ModelProfile:
    return ModelProfile(
        model_id=model_id,
        name=name,
        tier=tier,
        input_cost_per_1k=input_cost,
        output_cost_per_1k=output_cost,
        context_window=context_window,
        supports_streaming=streaming,
        supports_tools=tools,
    )


def _sample_profiles() -> list[ProviderProfile]:
    return [
        ProviderProfile(
            provider_id="prov-a",
            name="Provider A",
            models=[
                _make_model("a-econ", "A Economy", ModelTier.economy, 0.001, 0.002, 32_000),
                _make_model("a-prem", "A Premium", ModelTier.premium, 0.05, 0.10, 200_000),
            ],
            supported_modalities=["text"],
        ),
        ProviderProfile(
            provider_id="prov-b",
            name="Provider B",
            models=[
                _make_model("b-std", "B Standard", ModelTier.standard, 0.01, 0.02, 128_000),
            ],
            supported_modalities=["text", "image"],
        ),
    ]


# ---------------------------------------------------------------------------
# Cost ranking tests
# ---------------------------------------------------------------------------

class TestCostRanking:
    def test_ranking_order(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        ranking = comp.compute_cost_ranking(input_tokens=1000, output_tokens=500)
        costs = [entry["total_cost"] for entry in ranking]
        assert costs == sorted(costs), "Ranking should be ascending by cost"

    def test_ranking_includes_all_models(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        ranking = comp.compute_cost_ranking()
        assert len(ranking) == 3  # 2 from prov-a + 1 from prov-b

    def test_cost_calculation(self) -> None:
        model = _make_model(input_cost=0.01, output_cost=0.02)
        profile = ProviderProfile(
            provider_id="test", name="Test", models=[model]
        )
        comp = ProviderComparison([profile])
        ranking = comp.compute_cost_ranking(input_tokens=2000, output_tokens=1000)
        # (2000/1000)*0.01 + (1000/1000)*0.02 = 0.02 + 0.02 = 0.04
        assert ranking[0]["total_cost"] == pytest.approx(0.04)


# ---------------------------------------------------------------------------
# Capability matrix tests
# ---------------------------------------------------------------------------

class TestCapabilityMatrix:
    def test_matrix_size(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        matrix = comp.compute_capability_matrix()
        assert len(matrix) == 3

    def test_matrix_fields(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        matrix = comp.compute_capability_matrix()
        entry = matrix[0]
        assert "provider_id" in entry
        assert "model_id" in entry
        assert "context_window" in entry
        assert "supports_streaming" in entry
        assert "supports_tools" in entry
        assert "tier" in entry


# ---------------------------------------------------------------------------
# Budget filter tests
# ---------------------------------------------------------------------------

class TestBestForBudget:
    def test_filters_by_cost(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        # Economy model costs: (1000/1000)*0.001 + (500/1000)*0.002 = 0.002
        # Standard model costs: (1000/1000)*0.01 + (500/1000)*0.02 = 0.02
        # Premium model costs: (1000/1000)*0.05 + (500/1000)*0.10 = 0.10
        results = comp.best_for_budget(0.01)
        model_ids = [r["model_id"] for r in results]
        assert "a-econ" in model_ids
        assert "a-prem" not in model_ids

    def test_filters_by_context_window(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        results = comp.best_for_budget(1.0, min_context_window=100_000)
        model_ids = [r["model_id"] for r in results]
        assert "a-econ" not in model_ids  # 32k context
        assert "a-prem" in model_ids  # 200k context
        assert "b-std" in model_ids  # 128k context

    def test_empty_when_nothing_fits(self) -> None:
        profiles = _sample_profiles()
        comp = ProviderComparison(profiles)
        results = comp.best_for_budget(0.0001)
        assert results == []


# ---------------------------------------------------------------------------
# Convenience function tests
# ---------------------------------------------------------------------------

class TestCompareProviders:
    def test_returns_ranked_list(self) -> None:
        profiles = _sample_profiles()
        results = compare_providers(profiles)
        assert len(results) == 3
        costs = [r["total_cost"] for r in results]
        assert costs == sorted(costs)
