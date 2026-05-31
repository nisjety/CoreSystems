"""
Control Plane event subscriber for Ingestion Plane.

Subscribes to Control Plane events via shared NATS and responds with
Ingestion Plane-specific actions.

- velion.controlplane.user.provider_linked → legacy setup hook, ignored for
  identity-only OAuth sign-ins
- velion.controlplane.org.plan_changed → (future: adjust sync resources)
- velion.controlplane.billing.quota_exceeded → pauses imports for the org
- velion.ingestion.quota.exceeded → cross-plane quota signal from Quarry
"""

import asyncio
import json
import logging
import time
from typing import Optional, Any, Dict, Set

try:
    import nats
except ImportError:
    nats = None  # type: ignore

logger = logging.getLogger(__name__)

MICROSOFT_IDENTITY_SCOPES = {"openid", "profile", "email"}


class ControlPlaneSubscriber:
    """
    Subscribes to Control Plane events and handles them in Ingestion Plane.
    
    Gracefully handles missing NATS broker — if unavailable, simply skips
    subscription without affecting service operation.
    """

    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "ingestion-plane",
    ):
        """
        Initialize Control Plane subscriber.
        
        Args:
            nats_url: NATS broker URL (e.g., "nats://velion-nats:4222")
            nats_token: Authentication token for shared NATS
            service_name: Local service identifier (for logging)
        """
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc: Optional[Any] = None
        self.js: Optional[Any] = None
        self._subscriptions: list[Any] = []
        self._lock = asyncio.Lock()
        self._initialized = False

        # Callbacks for event handlers (set by parent service)
        self.on_user_provider_linked: Optional[callable] = None
        self.on_org_plan_changed: Optional[callable] = None
        self.on_billing_quota_exceeded: Optional[callable] = None

        # Orgs paused due to quota exhaustion — checked by ImportService.
        # Keys: org_id → expiry timestamp (monotonic). Entries auto-expire
        # after _PAUSE_TTL_SECONDS so orgs resume automatically when the
        # billing window resets, even without an explicit "resume" event.
        self._paused_orgs: Dict[str, float] = {}
        self._PAUSE_TTL_SECONDS = 3600  # 1 hour default

    def is_org_paused(self, org_id: str) -> bool:
        """Check whether an org is currently paused due to quota exhaustion."""
        expiry = self._paused_orgs.get(org_id)
        if expiry is None:
            return False
        if time.monotonic() > expiry:
            self._paused_orgs.pop(org_id, None)
            return False
        return True

    def pause_org(self, org_id: str, ttl_seconds: Optional[int] = None) -> None:
        """Mark an org as paused for the given TTL."""
        ttl = ttl_seconds if ttl_seconds is not None else self._PAUSE_TTL_SECONDS
        self._paused_orgs[org_id] = time.monotonic() + ttl
        logger.warning(f"⏸️  Paused imports for org={org_id} (TTL={ttl}s)")

    def resume_org(self, org_id: str) -> None:
        """Explicitly resume an org (e.g., after a billing top-up)."""
        if self._paused_orgs.pop(org_id, None) is not None:
            logger.info(f"▶️  Resumed imports for org={org_id}")

    async def initialize(self) -> bool:
        """
        Connect to shared NATS and subscribe to Control Plane events.
        
        Returns True if initialized, False if unavailable (graceful degradation).
        """
        if self._initialized:
            return self.nc is not None

        async with self._lock:
            if self._initialized:
                return self.nc is not None

            try:
                if not nats:
                    logger.warning("nats library not installed — Control Plane subscriber disabled")
                    self._initialized = True
                    return False

                if not self.nats_url:
                    logger.info("NATS_SHARED_URL empty — Control Plane subscriber disabled")
                    self._initialized = True
                    return False

                logger.info(f"🔐 Control Plane Subscriber ({self.service_name}): using token authentication")
                self.nc = await nats.connect(
                    self.nats_url,
                    token=self.nats_token,
                    name=f"{self.service_name}-subscriber",
                reconnect_time_wait=2,
                    max_reconnect_attempts=3,
                )
                self.js = self.nc.jetstream()

                # Subscribe to live Control Plane events. These handlers do not
                # need durable replay, so core NATS subscriptions avoid
                # JetStream stream bootstrap coupling at service startup.
                logger.info(f"📡 Control Plane Subscriber ({self.service_name}): subscribing to events...")

                # Subscribe to user.provider_linked (M365 setup trigger)
                sub1 = await self.nc.subscribe(
                    "velion.controlplane.user.provider_linked",
                    queue="ingestion-plane-m365",
                    cb=self._handle_user_provider_linked,
                )
                self._subscriptions.append(sub1)
                logger.info("  ✅ Subscribed to: velion.controlplane.user.provider_linked")

                # Subscribe to org.plan_changed (future: adjust sync resources)
                sub2 = await self.nc.subscribe(
                    "velion.controlplane.org.plan_changed",
                    queue="ingestion-plane-plan",
                    cb=self._handle_org_plan_changed,
                )
                self._subscriptions.append(sub2)
                logger.info("  ✅ Subscribed to: velion.controlplane.org.plan_changed")

                # Subscribe to billing.quota_exceeded (pauses imports for the org)
                sub3 = await self.nc.subscribe(
                    "velion.controlplane.billing.quota_exceeded",
                    queue="ingestion-plane-quota",
                    cb=self._handle_billing_quota_exceeded,
                )
                self._subscriptions.append(sub3)
                logger.info("  ✅ Subscribed to: velion.controlplane.billing.quota_exceeded")

                # Subscribe to Quarry's ingestion-level quota exceeded signal
                sub4 = await self.nc.subscribe(
                    "velion.ingestion.quota.exceeded",
                    queue="ingestion-plane-quota-ingestion",
                    cb=self._handle_ingestion_quota_exceeded,
                )
                self._subscriptions.append(sub4)
                logger.info("  ✅ Subscribed to: velion.ingestion.quota.exceeded")

                logger.info(f"✅ Control Plane Subscriber ({self.service_name}): listening for events")
                self._initialized = True
                return True

            except Exception as e:
                logger.warning(f"⚠️  Control Plane Subscriber ({self.service_name}) unavailable: {e}")
                self._initialized = True
                return False

    async def _handle_user_provider_linked(self, msg: Any) -> None:
        """Handle velion.controlplane.user.provider_linked event."""
        try:
            event = json.loads(msg.data.decode())
            user_id = event.get("user_id")
            provider = event.get("provider")
            tenant_id = event.get("tenant_id")
            email = event.get("email")
            scopes_granted = set(event.get("scopes_granted") or [])

            logger.info(
                f"📬 Received: user.provider_linked — "
                f"user_id={user_id} provider={provider} tenant_id={tenant_id}"
            )

            # Filter for Microsoft provider and ignore identity-only sign-in scopes.
            if provider and provider.lower() in ("microsoft", "microsoft365"):
                data_access_scopes = {
                    scope for scope in scopes_granted if scope not in MICROSOFT_IDENTITY_SCOPES
                }
                if not data_access_scopes:
                    logger.info(
                        "Skipping M365 provisioning for identity-only auth-core link event"
                    )
                    await self._ack_msg(msg)
                    return

                if self.on_user_provider_linked:
                    await self.on_user_provider_linked(
                        user_id=user_id,
                        provider=provider,
                        tenant_id=tenant_id,
                        email=email,
                    )
                else:
                    logger.warning("⚠️  No handler for user.provider_linked events")

            await self._ack_msg(msg)

        except Exception as e:
            logger.error(f"Error handling user.provider_linked event: {e}")
            await self._nak_msg(msg)

    async def _handle_org_plan_changed(self, msg: Any) -> None:
        """Handle velion.controlplane.org.plan_changed event."""
        try:
            event = json.loads(msg.data.decode())
            org_id = event.get("org_id")
            plan = event.get("plan")
            tier = event.get("tier")

            logger.info(
                f"📬 Received: org.plan_changed — "
                f"org_id={org_id} plan={plan} tier={tier}"
            )

            if self.on_org_plan_changed:
                await self.on_org_plan_changed(
                    org_id=org_id,
                    plan=plan,
                    tier=tier,
                )

            await self._ack_msg(msg)

        except Exception as e:
            logger.error(f"Error handling org.plan_changed event: {e}")
            await self._nak_msg(msg)

    async def _handle_billing_quota_exceeded(self, msg: Any) -> None:
        """Handle velion.controlplane.billing.quota_exceeded event.
        
        Pauses imports for the affected org so no further credits are consumed
        until the billing window resets or the org tops up.
        """
        try:
            event = json.loads(msg.data.decode())
            org_id = event.get("org_id")
            metric = event.get("metric")
            limit = event.get("limit")
            current = event.get("current")

            logger.info(
                f"📬 Received: billing.quota_exceeded — "
                f"org_id={org_id} metric={metric} limit={limit} current={current}"
            )

            # Pause imports for this org.
            if org_id:
                self.pause_org(org_id)

            if self.on_billing_quota_exceeded:
                await self.on_billing_quota_exceeded(
                    org_id=org_id,
                    metric=metric,
                    limit=limit,
                    current=current,
                )

            await self._ack_msg(msg)

        except Exception as e:
            logger.error(f"Error handling billing.quota_exceeded event: {e}")
            await self._nak_msg(msg)

    async def _handle_ingestion_quota_exceeded(self, msg: Any) -> None:
        """Handle velion.ingestion.quota.exceeded event (from Quarry).
        
        Also pauses imports for the affected org since the shared credit pool
        is exhausted across all ingestion services.
        """
        try:
            event = json.loads(msg.data.decode())
            org_id = event.get("org_id")
            metric = event.get("metric")

            logger.info(
                f"📬 Received: ingestion.quota.exceeded — "
                f"org_id={org_id} metric={metric}"
            )

            if org_id:
                self.pause_org(org_id)

            await self._ack_msg(msg)

        except Exception as e:
            logger.error(f"Error handling ingestion.quota.exceeded event: {e}")
            await self._nak_msg(msg)

    async def _ack_msg(self, msg: Any) -> None:
        ack = getattr(msg, "ack", None)
        if ack:
            await ack()

    async def _nak_msg(self, msg: Any) -> None:
        nak = getattr(msg, "nak", None)
        if nak:
            await nak()

    async def close(self) -> None:
        """Close connection and unsubscribe."""
        try:
            for sub in self._subscriptions:
                await sub.unsubscribe()
            if self.nc:
                await self.nc.close()
                self.nc = None
                self.js = None
            self._subscriptions.clear()
        except Exception as e:
            logger.warning(f"Error closing Control Plane subscriber: {e}")
