"""Provider comparison and ranking utilities."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from provider_research.profiles import ModelProfile, ProviderProfile


@dataclass(frozen=True)
class _RankedModel:
    """Internal ranking result for a single model."""

    provider_id: str
    provider_name: str
    model: ModelProfile
    total_cost: float


class ProviderComparison:
    """Compare a set of provider profiles on cost and capabilities.

    Args:
        profiles: List of :class:`ProviderProfile` to compare.
    """

    def __init__(self, profiles: list[ProviderProfile]) -> None:
        self._profiles = list(profiles)

    # ------------------------------------------------------------------
    # Cost ranking
    # ------------------------------------------------------------------

    def compute_cost_ranking(
        self,
        input_tokens: int = 1000,
        output_tokens: int = 500,
    ) -> list[dict[str, Any]]:
        """Rank every model across all providers by total cost.

        Args:
            input_tokens: Number of input tokens to price.
            output_tokens: Number of output tokens to price.

        Returns:
            List of dicts sorted by ascending ``total_cost``, each containing
            ``provider_id``, ``provider_name``, ``model_id``, ``model_name``,
            ``total_cost``, and ``tier``.
        """
        ranked: list[_RankedModel] = []
        for provider in self._profiles:
            for model in provider.models:
                cost = (
                    (input_tokens / 1000.0) * model.input_cost_per_1k
                    + (output_tokens / 1000.0) * model.output_cost_per_1k
                )
                ranked.append(
                    _RankedModel(
                        provider_id=provider.provider_id,
                        provider_name=provider.name,
                        model=model,
                        total_cost=cost,
                    )
                )

        ranked.sort(key=lambda r: r.total_cost)

        return [
            {
                "provider_id": r.provider_id,
                "provider_name": r.provider_name,
                "model_id": r.model.model_id,
                "model_name": r.model.name,
                "total_cost": round(r.total_cost, 6),
                "tier": r.model.tier.value,
            }
            for r in ranked
        ]

    # ------------------------------------------------------------------
    # Capability matrix
    # ------------------------------------------------------------------

    def compute_capability_matrix(self) -> list[dict[str, Any]]:
        """Build a capability matrix across all models.

        Returns:
            List of dicts, one per model, each containing ``provider_id``,
            ``model_id``, ``context_window``, ``supports_streaming``,
            ``supports_tools``, and ``tier``.
        """
        matrix: list[dict[str, Any]] = []
        for provider in self._profiles:
            for model in provider.models:
                matrix.append(
                    {
                        "provider_id": provider.provider_id,
                        "model_id": model.model_id,
                        "context_window": model.context_window,
                        "supports_streaming": model.supports_streaming,
                        "supports_tools": model.supports_tools,
                        "tier": model.tier.value,
                    }
                )
        return matrix

    # ------------------------------------------------------------------
    # Budget filter
    # ------------------------------------------------------------------

    def best_for_budget(
        self,
        max_cost_usd: float,
        min_context_window: int = 0,
        *,
        input_tokens: int = 1000,
        output_tokens: int = 500,
    ) -> list[dict[str, Any]]:
        """Return models that fit within *max_cost_usd* and meet the
        minimum context window, sorted cheapest-first.

        Args:
            max_cost_usd: Maximum acceptable cost for the token volumes.
            min_context_window: Minimum context window required.
            input_tokens: Number of input tokens to price.
            output_tokens: Number of output tokens to price.

        Returns:
            Filtered and sorted list of model dicts.
        """
        ranking = self.compute_cost_ranking(input_tokens, output_tokens)
        return [
            entry
            for entry in ranking
            if entry["total_cost"] <= max_cost_usd
            and self._context_window_for(entry["provider_id"], entry["model_id"])
            >= min_context_window
        ]

    def _context_window_for(self, provider_id: str, model_id: str) -> int:
        for provider in self._profiles:
            if provider.provider_id == provider_id:
                for model in provider.models:
                    if model.model_id == model_id:
                        return model.context_window
        return 0


def compare_providers(
    profiles: list[ProviderProfile],
    *,
    input_tokens: int = 1000,
    output_tokens: int = 500,
) -> list[dict[str, Any]]:
    """Convenience function: rank providers by cost.

    Args:
        profiles: Provider profiles to compare.
        input_tokens: Number of input tokens (default 1000).
        output_tokens: Number of output tokens (default 500).

    Returns:
        Cost-ranked list of model dicts.
    """
    comparison = ProviderComparison(profiles)
    return comparison.compute_cost_ranking(input_tokens, output_tokens)
