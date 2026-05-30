"""Org plan sync — subscribes to org.plan_changed events from org-core.

org-core publishes ``aqencia.controlplane.org.plan_changed`` on velion-nats
(shared_publisher.go) whenever an organisation's billing plan changes.

This handler keeps local ``org_policy_limits`` rows current so that the
agent-core rate-limiter and cost guards reflect the org's current plan.

Plan → limit mapping: plans are treated as tiers. Unknown plans default
to the "free" tier caps. Override by setting the limits table directly
for granular control.
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from app.nats_client import NatsManager

from app.database import get_pool
from app.policy import OrgPolicyLimits, PolicyLimitAction
from app.policy.repository import upsert_org_policy

logger = logging.getLogger(__name__)

# velion-nats subject where org-core publishes plan changes
PLAN_CHANGED_SUBJECT = "aqencia.controlplane.org.plan_changed"
DURABLE_NAME = "agent-core-v2-org-plan-sync"
STREAM_NAME = "AQENCIA_CONTROL"  # must exist on velion-nats or fallback to push sub

# Tier → default policy limits
_PLAN_LIMITS: dict[str, dict[str, Any]] = {
    "free": {
        "max_concurrent_runs": 2,
        "max_concurrent_runs_per_user": 1,
        "max_tokens_per_run": 50_000,
        "max_cost_usd_per_run": 0.5,
        "max_cost_usd_monthly": 5.0,
        "max_actions_per_run": 5,
        "cost_limit_action": PolicyLimitAction.BLOCK,
    },
    "starter": {
        "max_concurrent_runs": 5,
        "max_concurrent_runs_per_user": 3,
        "max_tokens_per_run": 100_000,
        "max_cost_usd_per_run": 2.0,
        "max_cost_usd_monthly": 50.0,
        "max_actions_per_run": 10,
        "cost_limit_action": PolicyLimitAction.WARN,
    },
    "pro": {
        "max_concurrent_runs": 10,
        "max_concurrent_runs_per_user": 5,
        "max_tokens_per_run": 200_000,
        "max_cost_usd_per_run": 5.0,
        "max_cost_usd_monthly": 500.0,
        "max_actions_per_run": 20,
        "cost_limit_action": PolicyLimitAction.WARN,
    },
    "enterprise": {
        "max_concurrent_runs": -1,
        "max_concurrent_runs_per_user": -1,
        "max_tokens_per_run": -1,
        "max_cost_usd_per_run": -1.0,
        "max_cost_usd_monthly": -1.0,
        "max_actions_per_run": -1,
        "cost_limit_action": PolicyLimitAction.WARN,
    },
}


class OrgPlanSyncHandler:
    """Listens for org plan-change events and updates local policy rows."""

    def __init__(self, nats_mgr: NatsManager) -> None:
        self._nats = nats_mgr
        self._task: asyncio.Task[None] | None = None
        self._stop = asyncio.Event()

    async def start(self) -> None:
        """Start background subscription. No-op if velion-nats unavailable."""
        self._stop.clear()
        self._task = asyncio.create_task(self._run(), name="org-plan-sync")
        logger.info("org_plan_sync_started", extra={"subject": PLAN_CHANGED_SUBJECT})

    async def stop(self) -> None:
        self._stop.set()
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

    async def _run(self) -> None:
        """Subscribe to plan-changed events via plain core NATS on velion-nats."""
        try:
            cross = self._nats.cross  # raises RuntimeError if not connected
        except RuntimeError:
            logger.warning("org_plan_sync_no_cross_nats_skipping")
            return

        # Use plain core NATS subscribe (the subject may not have a stream)
        sub = await cross.subscribe(PLAN_CHANGED_SUBJECT)
        logger.info("org_plan_sync_subscribed")

        while not self._stop.is_set():
            try:
                msg = await asyncio.wait_for(sub.next_msg(), timeout=5.0)
            except asyncio.TimeoutError:
                continue
            except Exception as exc:
                logger.error("org_plan_sync_receive_error", extra={"error": str(exc)})
                await asyncio.sleep(1)
                continue

            try:
                payload = json.loads(msg.data.decode())
                await self._handle(payload)
            except Exception as exc:
                logger.error("org_plan_sync_handle_error", extra={"error": str(exc)})

        await sub.unsubscribe()

    async def _handle(self, payload: dict[str, Any]) -> None:
        org_id = payload.get("org_id") or payload.get("organizationId") or payload.get("organization_id")
        new_plan = payload.get("new_plan") or payload.get("newPlan") or payload.get("plan")

        if not org_id or not new_plan:
            logger.debug("org_plan_sync_missing_fields", extra={"payload_keys": list(payload.keys())})
            return

        plan_key = str(new_plan).lower()
        limits = _PLAN_LIMITS.get(plan_key, _PLAN_LIMITS["free"])

        policy = OrgPolicyLimits(org_id=org_id, **limits)

        pool = await get_pool()
        async with pool.acquire() as conn:
            await upsert_org_policy(conn, policy)

        logger.info(
            "org_plan_synced",
            extra={"org_id": org_id, "plan": plan_key},
        )
