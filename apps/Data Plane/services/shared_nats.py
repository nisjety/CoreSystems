"""
Shared cross-plane NATS publisher for Data Plane services.

Publishes events to velion-nats (shared NATS broker) on aqencia.data.*
subjects for consumption by other planes (Control, Reasoning, Application).

Gracefully handles missing velion-nats — if unavailable, events are simply
not published (no caller errors).
"""

import asyncio
import json
import logging
from typing import Any, Optional

try:
    import nats
except ImportError:
    nats = None  # type: ignore

logger = logging.getLogger(__name__)


class SharedNatsPublisher:
    """
    Publishes cross-plane domain events to velion-nats.
    
    Thread-safe async API. If NATS connection unavailable at init, gracefully
    continues without publishing.
    """

    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "data-plane",
    ):
        """
        Initialize shared NATS publisher.
        
        Args:
            nats_url: NATS broker URL (e.g., "nats://velion-nats:4222")
            nats_token: Authentication token for shared NATS
            service_name: Source service identifier (for logging)
        """
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc: Optional[Any] = None
        self.js: Optional[Any] = None
        self._lock = asyncio.Lock()
        self._initialized = False

    async def initialize(self) -> bool:
        """
        Connect to shared NATS and ensure AQENCIA_DATAPLANE stream exists.
        
        Returns True if connected, False if unavailable (graceful degradation).
        """
        if self._initialized:
            return self.nc is not None

        async with self._lock:
            if self._initialized:
                return self.nc is not None

            try:
                if not nats:
                    logger.warning("nats library not installed — shared NATS disabled")
                    self._initialized = True
                    return False

                if not self.nats_url:
                    logger.info("NATS_SHARED_URL empty — shared cross-plane events disabled")
                    self._initialized = True
                    return False

                logger.info(f"🔐 Shared NATS ({self.service_name}): using token authentication")
                self.nc = await nats.connect(
                    self.nats_url,
                    token=self.nats_token,
                    name=f"{self.service_name}-publisher",
                    reconnect_time_wait=2,
                    max_reconnect_attempts=10,
                )
                self.js = self.nc.jetstream()

                # Create or ensure stream exists
                try:
                    await self.js.add_stream(
                        name="AQENCIA_DATAPLANE",
                        subjects=["aqencia.data.>"],
                        max_age=14 * 24 * 60 * 60 * 1_000_000_000,  # 14 days in nanoseconds
                        max_msgs=100_000,
                        discard="old",
                    )
                except Exception as e:
                    # Stream might already exist
                    if "STREAM_EXISTS" not in str(e):
                        logger.warning(f"Failed to create AQENCIA_DATAPLANE stream: {e}")

                logger.info(f"✅ Shared NATS ({self.service_name}): AQENCIA_DATAPLANE stream ready")
                logger.info(f"✅ Connected to shared NATS ({self.service_name}): {self.nats_url}")
                self._initialized = True
                return True

            except Exception as e:
                logger.warning(f"⚠️  Shared NATS unavailable ({self.service_name}): {e}")
                self._initialized = True
                return False

    async def publish_document_ingested(
        self,
        org_id: str,
        document_id: str,
        title: str,
        source: str,
    ) -> None:
        """Publish when a document is ingested (stored in DB)."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "document_id": document_id,
            "title": title,
            "source": source,
            "timestamp": self._iso_now(),
        }
        await self._publish("aqencia.data.document.ingested", payload)

    async def publish_document_embedded(
        self,
        org_id: str,
        document_id: str,
        embedding_model: str,
    ) -> None:
        """Publish when embeddings are created."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "document_id": document_id,
            "embedding_model": embedding_model,
            "timestamp": self._iso_now(),
        }
        await self._publish("aqencia.data.document.embedded", payload)

    async def publish_document_indexed(
        self,
        org_id: str,
        document_id: str,
        title: str,
    ) -> None:
        """Publish when document becomes searchable (indexed)."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "document_id": document_id,
            "title": title,
            "timestamp": self._iso_now(),
        }
        await self._publish("aqencia.data.document.indexed", payload)

    async def publish_search_executed(
        self,
        org_id: str,
        query: str,
        result_count: int,
    ) -> None:
        """Publish when retrieval (vector search + rerank) completes."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "query": query,
            "result_count": result_count,
            "timestamp": self._iso_now(),
        }
        await self._publish("aqencia.data.search.executed", payload)

    async def _publish(self, subject: str, payload: dict[str, Any]) -> None:
        """Publish to JetStream (fire-and-forget, never fails caller)."""
        try:
            if self.js:
                await self.js.publish(subject, json.dumps(payload).encode())
        except Exception as e:
            logger.warning(f"Failed to publish {subject}: {e}")

    async def close(self) -> None:
        """Close connection."""
        try:
            if self.nc:
                await self.nc.close()
                self.nc = None
                self.js = None
        except Exception as e:
            logger.warning(f"Error closing shared NATS connection: {e}")

    @staticmethod
    def _iso_now() -> str:
        """Return ISO 8601 timestamp."""
        from datetime import datetime, timezone

        return datetime.now(timezone.utc).isoformat()
