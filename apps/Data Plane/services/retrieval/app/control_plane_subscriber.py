"""
Control Plane Event Subscriber — listens to cross-plane events published by Control Plane.

Subscribes to:
  - aqencia.controlplane.org.plan_changed    → quota enforcement
  - aqencia.controlplane.billing.quota_exceeded  → quota enforcement

Uses NATS JetStream with ordered consumers and queue groups for load balancing.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Callable, Dict, Optional

import nats
from nats.js import JetStreamContext
from nats.errors import Error as NatsError

logger = logging.getLogger(__name__)


class ControlPlaneSubscriber:
    """Async event subscriber for Control Plane events via shared NATS JetStream."""

    def __init__(self, nats_url: str, nats_token: str, service_name: str):
        """
        Initialize Control Plane Subscriber.

        Args:
            nats_url: NATS broker URL (e.g., nats://velion-nats:4222)
            nats_token: NATS auth token
            service_name: Name of subscribing service (for logging, queue groups)
        """
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc: Optional[nats.NATS] = None
        self.js: Optional[JetStreamContext] = None

        # Event handler callbacks (set by caller)
        self.on_org_plan_changed: Optional[Callable] = None
        self.on_billing_quota_exceeded: Optional[Callable] = None
        self.on_document_acl_changed: Optional[Callable] = None

    async def initialize(self) -> bool:
        """
        Connect to shared NATS, ensure stream exists, and subscribe to events.

        Returns:
            True if successful, False if NATS unavailable (graceful degradation).
        """
        try:
            # Connect to NATS
            self.nc = await nats.connect(
                self.nats_url,
                token=self.nats_token,
                connect_timeout=3,
                max_reconnect_attempts=2,
                reconnect_time_wait=2,
            )
            logger.info(
                f"🔐 Control Plane Subscriber ({self.service_name}): using token authentication"
            )
            logger.info(
                f"✅ Connected to shared NATS ({self.service_name}): {self.nats_url}"
            )

            # Get JetStream context
            self.js = self.nc.jetstream()

            # Ensure AQENCIA_CONTROLPLANE stream exists (idempotent)
            try:
                await self.js.stream_info("AQENCIA_CONTROLPLANE")
            except NatsError as e:
                logger.warning(
                    f"AQENCIA_CONTROLPLANE stream missing ({self.service_name}): {e}"
                )
                return False

            logger.info(
                f"📡 Control Plane Subscriber ({self.service_name}): subscribing to events..."
            )

            # Subscribe to org.plan_changed
            await self.js.subscribe(
                "aqencia.controlplane.org.plan_changed",
                queue=f"data-plane-quota",  # Load balancing across instances
                cb=self._handle_org_plan_changed,
                ordered_consumer=False,
            )
            logger.info(
                f"  ✅ Subscribed to: aqencia.controlplane.org.plan_changed"
            )

            # Subscribe to billing.quota_exceeded
            await self.js.subscribe(
                "aqencia.controlplane.billing.quota_exceeded",
                queue=f"data-plane-quota-alert",  # Load balancing across instances
                cb=self._handle_billing_quota_exceeded,
                ordered_consumer=False,
            )
            logger.info(
                f"  ✅ Subscribed to: aqencia.controlplane.billing.quota_exceeded"
            )

            # Subscribe to acl.document.changed
            await self.js.subscribe(
                "aqencia.controlplane.acl.document.changed",
                queue="data-plane-acl",
                durable="data-plane-acl-consumer",
                cb=self._handle_document_acl_changed,
                ordered_consumer=False,
            )
            logger.info(
                f"  ✅ Subscribed to: aqencia.controlplane.acl.document.changed"
            )

            logger.info(
                f"✅ Control Plane Subscriber ({self.service_name}): listening for events"
            )
            return True

        except Exception as e:
            logger.warning(
                f"⚠️  Control Plane Subscriber ({self.service_name}) failed: {e}"
            )
            return False

    async def _handle_org_plan_changed(self, msg: Any) -> None:
        """Handle org.plan_changed event from Control Plane."""
        try:
            data = json.loads(msg.data.decode("utf-8"))

            # Extract required fields
            org_id = data.get("org_id")
            # Publisher sends 'new_plan'; fall back to legacy 'plan' field
            plan = data.get("new_plan") or data.get("plan")  # tier: "free" | "professional" | "enterprise"

            if not org_id or not plan:
                logger.error(
                    f"Invalid org.plan_changed event (missing required fields): {data}"
                )
                # ACK so the broken message is not redelivered indefinitely
                await msg.ack()
                return

            # Invoke callback if registered
            if self.on_org_plan_changed:
                success = await self.on_org_plan_changed(org_id, plan)
                if success:
                    await msg.ack()
                else:
                    await msg.nak()  # Redelivery
            else:
                await msg.ack()  # No callback registered, just ack

            logger.info(f"event=org.plan_changed org_id={org_id} plan={plan}")

        except Exception as e:
            logger.error(f"Error handling org.plan_changed: {e}")
            await msg.nak()

    async def _handle_billing_quota_exceeded(self, msg: Any) -> None:
        """Handle billing.quota_exceeded event from Control Plane."""
        try:
            data = json.loads(msg.data.decode("utf-8"))

            # Extract required fields
            org_id = data.get("org_id")
            metric = data.get("metric")  # "api_calls" | "storage" | "users"
            limit = data.get("limit")
            current = data.get("current")

            if not org_id or not metric:
                logger.warning(
                    f"Invalid billing.quota_exceeded event: {data}"
                )
                await msg.ack()  # ACK to prevent infinite redelivery on malformed events
                return

            # Invoke callback if registered
            if self.on_billing_quota_exceeded:
                success = await self.on_billing_quota_exceeded(org_id, metric, limit, current)
                if success:
                    await msg.ack()
                else:
                    await msg.nak()  # Redelivery
            else:
                await msg.ack()  # No callback registered, just ack

            logger.warning(
                f"event=billing.quota_exceeded org_id={org_id} metric={metric} limit={limit} current={current}"
            )

        except Exception as e:
            logger.error(f"Error handling billing.quota_exceeded: {e}")
            await msg.nak()

    async def _handle_document_acl_changed(self, msg: Any) -> None:
        """Handle acl.document.changed event from Control Plane."""
        logger.info("🔔 [ACL] document.acl.changed received")
        try:
            data = json.loads(msg.data.decode("utf-8"))

            acl_id           = data.get("acl_id")
            org_id           = data.get("org_id")
            document_id      = data.get("document_id")
            user_id          = data.get("user_id")
            action           = data.get("action")           # "grant" | "revoke"
            permission_level = data.get("permission_level", "read")

            if not all([acl_id, org_id, document_id, user_id, action]):
                logger.warning("⚠️ [ACL] Missing required fields — skipping")
                await msg.ack()
                return

            if self.on_document_acl_changed:
                success = await self.on_document_acl_changed(
                    acl_id=acl_id,
                    org_id=org_id,
                    document_id=document_id,
                    user_id=user_id,
                    action=action,
                    permission_level=permission_level,
                )
                if success:
                    await msg.ack()
                else:
                    await msg.nak()
            else:
                await msg.ack()

            logger.info("✅ [ACL] document_acl processed: %s %s", action, document_id)

        except Exception as exc:
            logger.error("❌ [ACL] Failed to handle document_acl_changed: %s", exc)
            await msg.nak()

    async def close(self) -> None:
        """Clean up: unsubscribe and close NATS connection."""
        if self.nc:
            await self.nc.close()
            logger.info(f"Control Plane Subscriber ({self.service_name}): closed")
