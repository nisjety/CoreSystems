"""Model selector — picks the best model given a routing policy + model catalog."""

from __future__ import annotations

import logging

from app import repository
from app.domain import (
    FallbackStrategy,
    LatencyClass,
    ModelConfig,
    ModelSelection,
    RoutingPolicy,
)
from app.routing import budget as budget_mod
from app.routing import providers as provider_mod

logger = logging.getLogger(__name__)

# Rough latency ceilings per class (ms) used to filter models.
_LATENCY_CEILING: dict[LatencyClass, int] = {
    LatencyClass.REALTIME: 500,
    LatencyClass.FAST: 2_000,
    LatencyClass.BALANCED: 5_000,
    LatencyClass.BACKGROUND: 999_999,
}


async def select(
    org_id: str,
    *,
    feature_requirements: dict[str, bool] | None = None,
    max_input_tokens: int | None = None,
) -> ModelSelection | None:
    """Select the best model for a request.

    Steps:
    1. Load routing policy.
    2. Load enabled model catalog.
    3. Filter by latency class, cost ceiling, and feature requirements.
    4. Apply provider priority / fallback strategy.
    5. Budget gate.
    """

    from app.routing.policy import get_policy

    policy = await get_policy(org_id)
    budget_result = await budget_mod.check(org_id, policy)
    if not budget_result.allowed:
        logger.warning("budget exhausted for org %s: %s", org_id, budget_result.reason)
        return None

    all_models = await repository.list_models(enabled=True)
    if not all_models:
        logger.error("no enabled models in catalog")
        return None

    candidates = _filter(all_models, policy, feature_requirements, max_input_tokens)
    if not candidates:
        logger.warning("no models match routing policy for org %s", org_id)
        return None

    ranked = _rank(candidates, policy)

    # Pick first healthy provider
    for model in ranked:
        healthy = await provider_mod.is_healthy(model.provider)
        if healthy:
            return ModelSelection(
                model_id=model.model_id,
                provider=model.provider,
                api_endpoint=model.api_endpoint,
                priority=ranked.index(model),
                estimated_cost_nok=model.cost_per_1k_input_nok,
            )

    # All providers unhealthy — return top-ranked regardless
    best = ranked[0]
    return ModelSelection(
        model_id=best.model_id,
        provider=best.provider,
        api_endpoint=best.api_endpoint,
        priority=0,
        estimated_cost_nok=best.cost_per_1k_input_nok,
    )


def _filter(
    models: list[ModelConfig],
    policy: RoutingPolicy,
    feature_reqs: dict[str, bool] | None,
    max_input_tokens: int | None,
) -> list[ModelConfig]:
    ceiling = _LATENCY_CEILING[policy.latency_class]
    result: list[ModelConfig] = []

    for m in models:
        if m.cost_per_1k_input_nok > policy.max_cost_per_request_nok:
            continue
        if max_input_tokens and m.context_window < max_input_tokens:
            continue
        if feature_reqs:
            if not all(m.capabilities.get(k) == v for k, v in feature_reqs.items()):
                continue
        result.append(m)

    return result


def _rank(
    models: list[ModelConfig],
    policy: RoutingPolicy,
) -> list[ModelConfig]:
    priority_map = {p: i for i, p in enumerate(policy.provider_priority)}

    if policy.fallback_strategy == FallbackStrategy.CHEAPEST:
        return sorted(models, key=lambda m: m.cost_per_1k_input_nok)

    if policy.fallback_strategy == FallbackStrategy.LOWEST_LATENCY:
        # Proxy: cheaper models are usually faster; could be refined later.
        return sorted(models, key=lambda m: m.cost_per_1k_input_nok)

    # SEQUENTIAL — use provider_priority order
    return sorted(
        models,
        key=lambda m: priority_map.get(m.provider, 999),
    )
