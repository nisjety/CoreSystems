"""
Retrieval event publisher — publishes search queries to shared NATS.

Shared NATS (JetStream):
  aqencia.data.search.executed -> Cross-plane subscribers (for analytics, logging, etc.)
"""
from __future__ import annotations

import logging
import sys
from typing import Any

from app.config import settings

logger = logging.getLogger(__name__)

_shared_nats: Any = None  # SharedNatsPublisher instance


async def get_shared_nats() -> Any:
    """Get or initialize shared NATS publisher (lazy init)."""
    global _shared_nats
    if _shared_nats is None:
        try:
            # Import here to avoid hard dependency
            sys.path.insert(0, "/app/services")
            from shared_nats import SharedNatsPublisher
            
            _shared_nats = SharedNatsPublisher(
                settings.nats_shared_url,
                settings.nats_shared_token,
                "retrieval-service",
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


async def publish_search_executed(org_id: str, query: str, result_count: int) -> None:
    """Publish search.executed to shared NATS for cross-plane subscribers."""
    nats = await get_shared_nats()
    if nats:
        await nats.publish_search_executed(
            org_id=org_id,
            query=query,
            result_count=result_count,
        )
