"""NATS client with JetStream for agent-core v2.

Manages three connections:
- velion-nats (cross-plane): session commands + agent events
- reasoning-nats (local): backward-compat aqencia.reasoning.* subjects
- controlplane-nats (control): usage events consumed by billing-core
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

import nats
from nats.aio.client import Client as NatsClient
from nats.js.api import (
    ConsumerConfig,
    DeliverPolicy,
    RetentionPolicy,
    StorageType,
    StreamConfig,
)
from nats.js.client import JetStreamContext

from app.config import settings

logger = logging.getLogger(__name__)

# JetStream stream definitions
STREAMS = {
    "VELION_AGENT": StreamConfig(
        name="VELION_AGENT",
        subjects=["velion.agent.>"],
        retention=RetentionPolicy.LIMITS,
        max_age=72 * 3600 * 1_000_000_000,  # 72h in nanos
        max_bytes=1 << 30,  # 1 GB
        storage=StorageType.FILE,
        num_replicas=1,
    ),
    "VELION_SESSION": StreamConfig(
        name="VELION_SESSION",
        subjects=["velion.session.>"],
        retention=RetentionPolicy.LIMITS,
        max_age=72 * 3600 * 1_000_000_000,
        max_bytes=1 << 30,
        storage=StorageType.FILE,
        num_replicas=1,
    ),
}

# Backward compatibility stream (on local reasoning-nats)
COMPAT_STREAM = StreamConfig(
    name="AQENCIA_REASONING",
    subjects=["aqencia.reasoning.>"],
    retention=RetentionPolicy.LIMITS,
    max_age=14 * 24 * 3600 * 1_000_000_000,  # 14 days
    max_msgs=100_000,
    storage=StorageType.FILE,
    num_replicas=1,
)


class NatsManager:
    """Wraps cross-plane, local, and control-plane NATS connections."""

    def __init__(self) -> None:
        self._cross: NatsClient | None = None
        self._local: NatsClient | None = None
        self._control: NatsClient | None = None
        self._js_cross: JetStreamContext | None = None
        self._js_local: JetStreamContext | None = None

    # ---- lifecycle ----

    async def connect(self) -> None:
        """Connect to both NATS clusters and create JetStream streams."""
        # Cross-plane (velion-nats)
        connect_opts: dict[str, Any] = {
            "servers": [settings.nats_url],
            "reconnect_time_wait": 2,
            "max_reconnect_attempts": -1,
            "name": f"{settings.service_name}-cross",
        }
        if settings.nats_token:
            connect_opts["token"] = settings.nats_token

        try:
            self._cross = await asyncio.wait_for(nats.connect(**connect_opts), timeout=10)
            self._js_cross = self._cross.jetstream()
            logger.info("nats_cross_connected", extra={"url": settings.nats_url})

            # Ensure cross-plane streams (non-fatal: velion-nats may not have JetStream)
            try:
                for stream_cfg in STREAMS.values():
                    await self._ensure_stream(self._js_cross, stream_cfg)
            except Exception as exc:
                logger.warning(
                    "nats_jetstream_stream_init_failed — JetStream features unavailable: %s",
                    exc,
                )
                self._js_cross = None
        except (asyncio.TimeoutError, Exception) as e:
            logger.warning("nats_cross_connect_failed: %s (proceeding without cross-plane NATS)", str(e))
            self._cross = None
            self._js_cross = None

        # Local (reasoning-nats) — optional, no hard failure
        try:
            local_opts: dict[str, Any] = {
                "servers": [settings.nats_local_url],
                "reconnect_time_wait": 2,
                "max_reconnect_attempts": 5,
                "name": f"{settings.service_name}-local",
            }
            if settings.nats_local_token:
                local_opts["token"] = settings.nats_local_token

            self._local = await asyncio.wait_for(nats.connect(**local_opts), timeout=10)
            self._js_local = self._local.jetstream()
            await self._ensure_stream(self._js_local, COMPAT_STREAM)
            logger.info("nats_local_connected", extra={"url": settings.nats_local_url})
        except Exception:
            logger.warning("nats_local_unavailable_continuing_without")
            self._local = None
            self._js_local = None

        # Control-plane (controlplane-nats) — for billing-core usage events
        # Uses plain NATS core (no JetStream) matching billing-core Subscribe()
        if settings.control_plane_nats_url:
            try:
                ctrl_opts: dict[str, Any] = {
                    "servers": [settings.control_plane_nats_url],
                    "reconnect_time_wait": 2,
                    "max_reconnect_attempts": 5,
                    "name": f"{settings.service_name}-control",
                }
                if settings.control_plane_nats_token:
                    ctrl_opts["token"] = settings.control_plane_nats_token
                self._control = await asyncio.wait_for(nats.connect(**ctrl_opts), timeout=10)
                logger.info("nats_control_connected", extra={"url": settings.control_plane_nats_url})
            except Exception:
                logger.warning("nats_control_unavailable_billing_events_disabled")
                self._control = None
        else:
            logger.info("nats_control_not_configured_billing_events_disabled")

    async def close(self) -> None:
        for nc in (self._cross, self._local, self._control):
            if nc and nc.is_connected:
                await nc.drain()
        self._cross = None
        self._local = None
        self._control = None
        self._js_cross = None
        self._js_local = None

    # ---- JetStream accessors ----

    @property
    def js_cross(self) -> JetStreamContext:
        if self._js_cross is None:
            raise RuntimeError("Cross-plane NATS not connected")
        return self._js_cross

    @property
    def js_local(self) -> JetStreamContext | None:
        return self._js_local

    @property
    def cross(self) -> NatsClient:
        if self._cross is None:
            raise RuntimeError("Cross-plane NATS not connected")
        return self._cross

    # ---- publish helpers ----

    async def publish_jetstream(
        self, subject: str, payload: dict[str, Any], *, local: bool = False
    ) -> None:
        """Publish a JSON message to a JetStream subject."""
        js = self._js_local if local else self._js_cross
        if js is None:
            logger.debug("nats_publish_skipped_no_connection", extra={"subject": subject})
            return
        data = json.dumps(payload, default=str).encode()
        await js.publish(subject, data)

    async def publish_core(self, subject: str, payload: dict[str, Any]) -> None:
        """Publish on core NATS (non-JetStream) on local connection."""
        nc = self._local or self._cross
        if nc is None:
            return
        data = json.dumps(payload, default=str).encode()
        await nc.publish(subject, data)

    async def publish_usage_event(self, subject: str, payload: dict[str, Any]) -> None:
        """Publish a usage event to controlplane-nats for billing-core.

        billing-core subscribes to ``usage.>`` via plain NATS core (not JetStream).
        Fire-and-forget — never raises so callers are never blocked.
        """
        if self._control is None or not self._control.is_connected:
            logger.debug("nats_control_not_connected_skipping_usage_event",
                         extra={"subject": subject})
            return
        try:
            data = json.dumps(payload, default=str).encode()
            await self._control.publish(subject, data)
        except Exception as exc:
            logger.warning("nats_usage_event_publish_failed",
                           extra={"subject": subject, "error": str(exc)})

    # ---- subscribe helpers ----

    async def subscribe_jetstream(
        self,
        subject: str,
        durable: str,
        *,
        stream: str | None = None,
    ) -> Any:
        """Create a JetStream pull subscription."""
        return await self.js_cross.pull_subscribe(
            subject,
            durable=durable,
            stream=stream,
        )

    # ---- internal ----

    @staticmethod
    async def _ensure_stream(js: JetStreamContext, cfg: StreamConfig) -> None:
        try:
            await js.find_stream_name_by_subject(cfg.subjects[0])
            await js.update_stream(cfg)
            logger.debug("stream_updated", extra={"name": cfg.name})
        except nats.js.errors.NotFoundError:
            await js.add_stream(cfg)
            logger.info("stream_created", extra={"name": cfg.name})
