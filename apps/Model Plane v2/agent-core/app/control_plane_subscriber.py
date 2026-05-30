"""Control Plane event subscriber for Model Plane v2.

Subscribes to Control Plane events via shared velion-nats JetStream and
responds with MP v2-specific actions.

- velion.controlplane.org.plan_changed    → logs plan change; future: adjust
                                             model routing tier per org
- velion.controlplane.billing.quota_exceeded → marks org as quota-exceeded;
                                               agent runs for that org will be
                                               rejected until billing resets

Gracefully degrades if velion-nats is unreachable at startup.
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Any, Callable, Coroutine, Optional

try:
    import nats
except ImportError:
    nats = None  # type: ignore

logger = logging.getLogger(__name__)

# Type alias for async event callbacks
_AsyncCallback = Callable[..., Coroutine[Any, Any, None]]


class ControlPlaneSubscriber:
    """Subscribe to Control Plane events on velion-nats JetStream.

    Thread-safe via asyncio.Lock.  All public methods are safe to call before
    ``initialize()`` — they no-op or return safe defaults.
    """

    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "mp-v2",
    ) -> None:
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name

        self.nc: Optional[Any] = None
        self.js: Optional[Any] = None
        self._subscriptions: list[Any] = []
        self._lock = asyncio.Lock()
        self._initialized = False

        # Optional external callbacks (injected after construction)
        self.on_org_plan_changed: Optional[_AsyncCallback] = None
        self.on_billing_quota_exceeded: Optional[_AsyncCallback] = None

        # Orgs that have exceeded their billing quota.
        # org_id → monotonic expiry timestamp.  Auto-expires after _PAUSE_TTL.
        self._paused_orgs: dict[str, float] = {}
        self._PAUSE_TTL_SECONDS: int = 3600  # 1 hour

    # ── Public helpers ────────────────────────────────────────────────────────

    def is_org_paused(self, org_id: str) -> bool:
        """Return True if the org is currently quota-paused."""
        expiry = self._paused_orgs.get(org_id)
        if expiry is None:
            return False
        if time.monotonic() > expiry:
            self._paused_orgs.pop(org_id, None)
            return False
        return True

    def pause_org(self, org_id: str, ttl_seconds: Optional[int] = None) -> None:
        """Mark an org as quota-paused for TTL seconds."""
        ttl = ttl_seconds if ttl_seconds is not None else self._PAUSE_TTL_SECONDS
        self._paused_orgs[org_id] = time.monotonic() + ttl
        logger.warning(
            "org_quota_paused",
            extra={"org_id": org_id, "ttl_seconds": ttl},
        )

    def resume_org(self, org_id: str) -> None:
        """Explicitly resume an org (e.g., after a billing top-up event)."""
        if self._paused_orgs.pop(org_id, None) is not None:
            logger.info("org_quota_resumed", extra={"org_id": org_id})

    # ── Lifecycle ─────────────────────────────────────────────────────────────

    async def initialize(self) -> bool:
        """Connect to velion-nats and subscribe to Control Plane subjects.

        Returns True if connected, False if unavailable (graceful degradation).
        Always safe to call; only attempts connection once.
        """
        if self._initialized:
            return self.nc is not None

        async with self._lock:
            if self._initialized:
                return self.nc is not None

            if not nats:
                logger.warning(
                    "control_plane_subscriber_disabled",
                    extra={"reason": "nats library not installed"},
                )
                self._initialized = True
                return False

            if not self.nats_url:
                logger.info(
                    "control_plane_subscriber_disabled",
                    extra={"reason": "NATS_URL empty"},
                )
                self._initialized = True
                return False

            try:
                logger.info(
                    "control_plane_subscriber_connecting",
                    extra={"url": self.nats_url, "service": self.service_name},
                )
                self.nc = await nats.connect(
                    self.nats_url,
                    token=self.nats_token or None,
                    name=f"{self.service_name}-cp-subscriber",
                    reconnect_time_wait=2,
                    max_reconnect_attempts=5,
                )
                self.js = self.nc.jetstream()

                # velion.controlplane.org.plan_changed
                sub1 = await self.js.subscribe(
                    "velion.controlplane.org.plan_changed",
                    queue=f"{self.service_name}-plan",
                    cb=self._handle_org_plan_changed,
                    ordered_consumer=False,
                )
                self._subscriptions.append(sub1)
                logger.info("cp_subscriber_subscribed", extra={"subject": "velion.controlplane.org.plan_changed"})

                # velion.controlplane.billing.quota_exceeded
                sub2 = await self.js.subscribe(
                    "velion.controlplane.billing.quota_exceeded",
                    queue=f"{self.service_name}-quota",
                    cb=self._handle_billing_quota_exceeded,
                    ordered_consumer=False,
                )
                self._subscriptions.append(sub2)
                logger.info("cp_subscriber_subscribed", extra={"subject": "velion.controlplane.billing.quota_exceeded"})

                logger.info(
                    "control_plane_subscriber_ready",
                    extra={"service": self.service_name, "subscriptions": 2},
                )
                self._initialized = True
                return True

            except Exception as exc:
                logger.warning(
                    "control_plane_subscriber_unavailable",
                    extra={"error": str(exc), "service": self.service_name},
                )
                self._initialized = True
                return False

    async def close(self) -> None:
        """Unsubscribe and close the NATS connection."""
        try:
            for sub in self._subscriptions:
                try:
                    await sub.unsubscribe()
                except Exception:
                    pass
            self._subscriptions.clear()
            if self.nc:
                await self.nc.close()
                self.nc = None
                self.js = None
        except Exception as exc:
            logger.warning("cp_subscriber_close_error", extra={"error": str(exc)})

    # ── Event handlers ────────────────────────────────────────────────────────

    async def _handle_org_plan_changed(self, msg: Any) -> None:
        """Handle velion.controlplane.org.plan_changed."""
        try:
            event: dict[str, Any] = json.loads(msg.data.decode())
            org_id: str = event.get("org_id", "")
            plan: str = event.get("plan", "")
            tier: str = event.get("tier", "")

            logger.info(
                "cp_event_org_plan_changed",
                extra={"org_id": org_id, "plan": plan, "tier": tier},
            )

            # If org was previously paused and plan upgraded, resume it
            if org_id and tier in ("pro", "enterprise", "unlimited"):
                self.resume_org(org_id)

            if self.on_org_plan_changed:
                await self.on_org_plan_changed(org_id=org_id, plan=plan, tier=tier)

            await msg.ack()

        except Exception as exc:
            logger.error("cp_event_handler_error", extra={"event": "org_plan_changed", "error": str(exc)})
            try:
                await msg.nak()
            except Exception:
                pass

    async def _handle_billing_quota_exceeded(self, msg: Any) -> None:
        """Handle velion.controlplane.billing.quota_exceeded.

        Marks the org as quota-exceeded so agent-core can reject new runs
        or return 429 immediately without burning more LLM tokens.
        """
        try:
            event: dict[str, Any] = json.loads(msg.data.decode())
            org_id: str = event.get("org_id", "")
            metric: str = event.get("metric", "")
            limit: Any = event.get("limit")
            current: Any = event.get("current")

            logger.warning(
                "cp_event_billing_quota_exceeded",
                extra={"org_id": org_id, "metric": metric, "limit": limit, "current": current},
            )

            if org_id:
                self.pause_org(org_id)

            if self.on_billing_quota_exceeded:
                await self.on_billing_quota_exceeded(
                    org_id=org_id,
                    metric=metric,
                    limit=limit,
                    current=current,
                )

            await msg.ack()

        except Exception as exc:
            logger.error("cp_event_handler_error", extra={"event": "billing_quota_exceeded", "error": str(exc)})
            try:
                await msg.nak()
            except Exception:
                pass
