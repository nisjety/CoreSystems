"""NATS client — publishes completion events."""

from __future__ import annotations

import asyncio
import json
import logging

import nats
from nats.aio.client import Client as NATSClient

from app.config import get_settings
from app.domain import CompletionEvent

logger = logging.getLogger(__name__)

_nc: NATSClient | None = None

SUBJECT_COMPLETED = "velion.reasoning.llm.completed"


async def connect() -> None:
    global _nc
    settings = get_settings()
    opts: dict = {"servers": [settings.nats_url]}
    if settings.nats_token:
        opts["token"] = settings.nats_token
    try:
        _nc = await asyncio.wait_for(nats.connect(**opts), timeout=10)
        logger.info("nats_connected url=%s", settings.nats_url)
    except (asyncio.TimeoutError, Exception) as e:
        logger.warning("nats_connection_failed: %s (proceeding without NATS)", str(e))
        _nc = None


async def close() -> None:
    global _nc
    if _nc:
        await _nc.close()
        _nc = None
        logger.info("nats_closed")


async def publish_completed(event: CompletionEvent) -> None:
    if _nc is None:
        logger.warning("nats_not_connected skip_publish")
        return

    payload = json.dumps(event.model_dump()).encode()
    await _nc.publish(SUBJECT_COMPLETED, payload)
    logger.debug(
        "published subject=%s request_id=%s", SUBJECT_COMPLETED, event.request_id
    )
