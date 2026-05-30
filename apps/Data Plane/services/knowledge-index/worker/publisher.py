"""
Knowledge Index event publisher — publishes document indexed events to shared NATS.

Shared NATS (JetStream):
  aqencia.data.document.indexed -> Cross-plane subscribers (to update caches, etc.)
"""
from __future__ import annotations

import logging

from worker.config import settings
from worker.shared_nats import SharedNatsPublisher

logger = logging.getLogger(__name__)

_shared_nats: SharedNatsPublisher | None = None  # SharedNatsPublisher instance


async def get_shared_nats() -> SharedNatsPublisher:
    """Get or initialize shared NATS publisher (lazy init)."""
    global _shared_nats
    if _shared_nats is None:
        try:
            _shared_nats = SharedNatsPublisher(
                settings.nats_shared_url,
                settings.nats_shared_token,
                "knowledge-index",
            )
            await _shared_nats.initialize()
        except Exception as e:
            logger.warning("Failed to init shared NATS: %s — events disabled", e)
            _shared_nats = None
    return _shared_nats


async def close_shared_nats() -> None:
    global _shared_nats
    if _shared_nats:
        try:
            await _shared_nats.close()
        except Exception as e:
            logger.warning("Error closing shared NATS: %s", e)


async def publish_document_indexed(org_id: str, document_id: str, title: str) -> None:
    """Publish document.indexed to shared NATS for cross-plane subscribers."""
    nats = await get_shared_nats()
    if nats:
        await nats.publish_document_indexed(
            org_id=org_id,
            document_id=document_id,
            title=title,
        )
