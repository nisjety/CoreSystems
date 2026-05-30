"""
Redis Stream publisher — fires events consumed by downstream workers.
Also publishes to shared NATS for cross-plane coordination.

Redis Streams:
  dataplane.documents.created   → Knowledge Index worker
  dataplane.documents.deleted   → Embedding Worker + Knowledge Index

Shared NATS (JetStream) — aqencia bus:
  aqencia.data.document.ingested  → Data Plane + other planes
  aqencia.data.document.indexed   → Cross-plane subscribers

Velion NATS (JetStream) — Model Plane v2 bus:
  velion.documents.indexed        → agent-core v2
  velion.documents.deleted        → agent-core v2
"""
from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any, Dict

import redis.asyncio as aioredis

from app.config import settings
from app.observability import (
    DOCUMENT_CROSSPLANE_PUBLISH_TOTAL,
    DOCUMENT_EVENT_PUBLISH_TOTAL,
)
from app.shared_nats import SharedNatsPublisher

logger = logging.getLogger(__name__)

_redis: aioredis.Redis | None = None
_shared_nats: SharedNatsPublisher | None = None  # Aqencia NATS publisher
_velion_nats: SharedNatsPublisher | None = None  # Velion-nats publisher


async def get_redis() -> aioredis.Redis:
    global _redis
    if _redis is None:
        _redis = aioredis.from_url(settings.redis_url, decode_responses=True)
    return _redis


async def get_shared_nats() -> SharedNatsPublisher:
    """Get or initialize shared NATS publisher (lazy init)."""
    global _shared_nats
    if _shared_nats is None:
        try:
            _shared_nats = SharedNatsPublisher(
                settings.nats_shared_url,
                settings.nats_shared_token,
                "documents-service",
            )
            await _shared_nats.initialize()
        except Exception as e:
            logger.warning("Failed to init shared NATS: %s — events disabled", e)
            _shared_nats = None
    return _shared_nats


async def get_velion_nats() -> SharedNatsPublisher | None:
    """Get or initialize velion-nats publisher for Model Plane v2 (lazy init)."""
    global _velion_nats
    if _velion_nats is None:
        if not settings.nats_velion_url:
            return None
        try:
            _velion_nats = SharedNatsPublisher(
                settings.nats_velion_url,
                settings.nats_velion_token,
                "documents-service-velion",
            )
            await _velion_nats.initialize()
        except Exception as e:
            logger.warning("Failed to init velion NATS: %s — v2 events disabled", e)
            _velion_nats = None
    return _velion_nats


async def close_shared_nats() -> None:
    global _shared_nats
    if _shared_nats:
        try:
            await _shared_nats.close()
        except Exception as e:
            logger.warning("Error closing shared NATS: %s", e)


async def close_velion_nats() -> None:
    global _velion_nats
    if _velion_nats:
        try:
            await _velion_nats.close()
        except Exception as e:
            logger.warning("Error closing velion NATS: %s", e)


# ── Stream names ──────────────────────────────────────────────────────────────

STREAM_DOCUMENT_CREATED = "dataplane.documents.created"
STREAM_DOCUMENT_DELETED = "dataplane.documents.deleted"


# ── Publishers ────────────────────────────────────────────────────────────────

async def publish_document_created(document_id: str, org_id: str, payload: Dict[str, Any]) -> None:
    """Publish document.created to Redis (intra-plane), aqencia NATS, and velion-nats."""
    r = await get_redis()
    try:
        await r.xadd(
            STREAM_DOCUMENT_CREATED,
            {
                "document_id": document_id,
                "org_id": org_id,
                "payload": json.dumps(payload),
            },
        )
        DOCUMENT_EVENT_PUBLISH_TOTAL.labels(event="document_created", result="success").inc()
    except Exception:
        DOCUMENT_EVENT_PUBLISH_TOTAL.labels(event="document_created", result="failure").inc()
        raise
    logger.info("event=document.created document_id=%s org_id=%s", document_id, org_id)
    
    title = payload.get("title", "")
    source = payload.get("source", "")

    # Publish to aqencia shared NATS for cross-plane subscribers
    nats = await get_shared_nats()
    if not nats:
        DOCUMENT_CROSSPLANE_PUBLISH_TOTAL.labels(
            event="document_ingested",
            result="skipped",
        ).inc()
        logger.warning(
            "cross-plane publish skipped document_id=%s org_id=%s reason=nats_unavailable",
            document_id,
            org_id,
        )
    else:
        try:
            await nats.publish_document_ingested(
                org_id=org_id,
                document_id=document_id,
                title=title,
                source=source,
            )
            DOCUMENT_CROSSPLANE_PUBLISH_TOTAL.labels(
                event="document_ingested",
                result="success",
            ).inc()
        except Exception as exc:
            DOCUMENT_CROSSPLANE_PUBLISH_TOTAL.labels(
                event="document_ingested",
                result="failure",
            ).inc()
            logger.error(
                "cross-plane publish failed document_id=%s org_id=%s error=%s",
                document_id,
                org_id,
                exc,
                exc_info=True,
            )

    # Publish to velion-nats for Model Plane v2 subscribers (agent-core v2)
    await _publish_velion_event(
        subject="velion.documents.indexed",
        document_id=document_id,
        org_id=org_id,
        title=title,
    )


async def publish_document_deleted(document_id: str, org_id: str) -> None:
    """Publish document.deleted to Redis (intra-plane) and velion-nats."""
    r = await get_redis()
    try:
        await r.xadd(
            STREAM_DOCUMENT_DELETED,
            {"document_id": document_id, "org_id": org_id},
        )
        DOCUMENT_EVENT_PUBLISH_TOTAL.labels(event="document_deleted", result="success").inc()
    except Exception:
        DOCUMENT_EVENT_PUBLISH_TOTAL.labels(event="document_deleted", result="failure").inc()
        raise
    logger.info("event=document.deleted document_id=%s org_id=%s", document_id, org_id)

    # Publish to velion-nats for Model Plane v2 subscribers
    await _publish_velion_event(
        subject="velion.documents.deleted",
        document_id=document_id,
        org_id=org_id,
    )


# ── Velion-nats helper ────────────────────────────────────────────────────────

async def _publish_velion_event(
    subject: str,
    document_id: str,
    org_id: str,
    title: str = "",
) -> None:
    """Publish a lightweight event to velion-nats (best-effort, no caller errors)."""
    vnats = await get_velion_nats()
    if not vnats or not vnats.nc:
        return
    payload = json.dumps({
        "org_id": org_id,
        "document_id": document_id,
        "title": title,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }).encode()
    try:
        await vnats.nc.publish(subject, payload)
        logger.info("velion_event_published subject=%s document_id=%s", subject, document_id)
    except Exception as exc:
        logger.warning(
            "velion_event_publish_failed subject=%s document_id=%s error=%s",
            subject, document_id, exc,
        )
