"""NATS client — manages connections to velion-nats and reasoning-nats."""

from __future__ import annotations

import asyncio
import logging

import nats
from nats.aio.client import Client as NATSClient
from nats.js import JetStreamContext

from app.config import settings

logger = logging.getLogger(__name__)


class NatsManager:
    """Manages dual NATS connections (cross-plane + local)."""

    def __init__(self) -> None:
        self._cross: NATSClient | None = None
        self._local: NATSClient | None = None
        self._js: JetStreamContext | None = None

    async def connect(self) -> None:
        """Establish connections to both NATS clusters."""
        # Cross-plane (velion-nats)
        opts: dict = {"servers": [settings.nats_url], "name": settings.service_name}
        if settings.nats_token:
            opts["token"] = settings.nats_token
        try:
            self._cross = await asyncio.wait_for(nats.connect(**opts), timeout=10)
            self._js = self._cross.jetstream()
            logger.info("nats_cross_connected", extra={"url": settings.nats_url})
        except (asyncio.TimeoutError, Exception) as e:
            logger.warning("nats_cross_connect_failed: %s (proceeding without NATS)", str(e))
            self._cross = None
            self._js = None

        # Local (reasoning-nats)
        local_opts: dict = {"servers": [settings.nats_local_url], "name": f"{settings.service_name}-local"}
        if settings.nats_local_token:
            local_opts["token"] = settings.nats_local_token
        try:
            self._local = await asyncio.wait_for(nats.connect(**local_opts), timeout=10)
            logger.info("nats_local_connected", extra={"url": settings.nats_local_url})
        except Exception as e:
            logger.warning("nats_local_connect_failed", extra={"error": str(e)})
            self._local = None

    @property
    def cross(self) -> NATSClient:
        return self._cross

    @property
    def local(self) -> NATSClient | None:
        return self._local

    @property
    def js(self) -> JetStreamContext:
        return self._js

    async def close(self) -> None:
        if self._local and not self._local.is_closed:
            await self._local.close()
        if self._cross and not self._cross.is_closed:
            await self._cross.close()
        logger.info("nats_connections_closed")
