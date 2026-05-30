"""NATS client for capability-core — publish capability events."""

from __future__ import annotations

import asyncio
import json
import logging

import nats
from nats.aio.client import Client as NatsClient

from app.config import settings

logger = logging.getLogger(__name__)

_nc: NatsClient | None = None


async def connect() -> NatsClient:
    """Connect to NATS with a timeout; if unavailable, proceed without NATS."""
    global _nc
    if _nc is None or _nc.is_closed:
        opts: dict = {"servers": [settings.nats_url], "connect_timeout": 5}
        if settings.nats_token:
            opts["token"] = settings.nats_token
        try:
            _nc = await asyncio.wait_for(
                nats.connect(**opts), timeout=10
            )
            logger.info("nats_connected", extra={"url": settings.nats_url})
        except (asyncio.TimeoutError, Exception) as e:
            logger.warning(
                "nats_connection_failed: %s (proceeding without NATS)",
                str(e),
            )
            _nc = None
    return _nc


async def close() -> None:
    global _nc
    if _nc and not _nc.is_closed:
        await _nc.drain()
        _nc = None
        logger.info("nats_closed")


async def publish_event(subject: str, payload: dict) -> None:
    """Publish event to NATS if connected; otherwise silent skip."""
    nc = await connect()
    if nc is not None:
        await nc.publish(subject, json.dumps(payload).encode())


async def tool_updated(tool_name: str, action: str = "upserted") -> None:
    await publish_event(
        "velion.capability.tool.updated",
        {"tool_name": tool_name, "action": action},
    )


async def plugin_installed(plugin_id: str, org_id: str) -> None:
    await publish_event(
        "velion.capability.plugin.installed",
        {"plugin_id": plugin_id, "org_id": org_id},
    )


async def plugin_enabled(plugin_id: str, session_id: str, enabled: bool) -> None:
    await publish_event(
        "velion.capability.plugin.enabled",
        {"plugin_id": plugin_id, "session_id": session_id, "enabled": enabled},
    )


# Aliases expected by main.py
connect_nats = connect
close_nats = close
