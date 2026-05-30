"""Routing policy — load / save org-level routing rules."""

from __future__ import annotations

import logging

from app import repository
from app.domain import (
    FallbackStrategy,
    LatencyClass,
    RoutingPolicy,
)

logger = logging.getLogger(__name__)


async def get_policy(org_id: str) -> RoutingPolicy:
    """Load an org's routing policy, or return a sensible default."""
    policy = await repository.get_routing_policy(org_id)
    if policy is not None:
        return policy

    return RoutingPolicy(
        org_id=org_id,
        tier="basic",
        latency_class=LatencyClass.BALANCED,
        fallback_strategy=FallbackStrategy.SEQUENTIAL,
        provider_priority=["openai", "anthropic"],
    )


async def upsert_policy(
    policy: RoutingPolicy, *, actor: str
) -> RoutingPolicy:
    """Create or update a routing policy."""
    saved = await repository.upsert_routing_policy(policy)
    await repository.write_audit(
        "routing_policy.upserted",
        entity_id=saved.org_id,
        actor_id=actor,
        payload={
            "tier": saved.tier,
            "latency_class": saved.latency_class.value,
        },
    )
    logger.info("routing policy upserted for org %s", saved.org_id)
    return saved
