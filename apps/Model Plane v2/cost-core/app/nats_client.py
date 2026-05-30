"""NATS JetStream client for cost-core v2.

Owns the VELION_COST stream covering velion.cost.> subjects.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Awaitable, Callable, Optional

import nats
from nats.aio.client import Client as NATS
from nats.js import JetStreamContext
from nats.js.api import RetentionPolicy, StorageType, StreamConfig

from app.config import get_settings

logger = logging.getLogger(__name__)

_nc: Optional[NATS] = None
_js: Optional[JetStreamContext] = None


async def connect() -> JetStreamContext:
    global _nc, _js
    if _js is not None:
        return _js

    settings = get_settings()
    _nc = await nats.connect(servers=[settings.nats_url], name=settings.service_name)
    _js = _nc.jetstream()

    await _js.add_stream(
        config=StreamConfig(
            name=settings.nats_stream,
            subjects=[settings.nats_subjects],
            retention=RetentionPolicy.LIMITS,
            storage=StorageType.FILE,
            max_age=72 * 3600,
            max_bytes=1 * 1024 * 1024 * 1024,
            num_replicas=1,
        )
    )
    logger.info(
        "nats jetstream ready",
        extra={"stream": settings.nats_stream, "subjects": settings.nats_subjects},
    )
    return _js


async def close() -> None:
    global _nc, _js
    if _nc is not None:
        await _nc.drain()
        _nc = None
    _js = None


async def publish(subject: str, payload: dict[str, Any]) -> None:
    js = await connect()
    data = json.dumps(payload).encode("utf-8")
    await js.publish(subject, data)


async def subscribe(
    subject: str,
    durable: str,
    handler: Callable[[dict[str, Any]], Awaitable[None]],
) -> None:
    js = await connect()

    async def _cb(msg):
        try:
            payload = json.loads(msg.data.decode("utf-8"))
            await handler(payload)
            await msg.ack()
        except Exception:
            logger.exception("nats handler failed subject=%s", subject)
            await msg.nak()

    await js.subscribe(subject, durable=durable, cb=_cb, manual_ack=True)
    logger.info("subscribed", extra={"subject": subject, "durable": durable})
