"""
Shared cross-plane NATS publisher for Ingestion Plane services.

Publishes events to verevon-nats (shared NATS broker) on verevon.ingestion.*
subjects for consumption by other planes (Data, Reasoning, Application).

Naming convention:
  - Cross-plane (verevon-nats): verevon.<plane>.<domain>.<action>
  - Intra-plane (local NATS): ingestion.<domain>.<action>
  - Notifications (plain NATS): verevon.notifications.<source>.<action>

Gracefully handles missing verevon-nats — if unavailable, events are simply
not published (no caller errors).
"""

import asyncio
import json
import logging
from typing import Optional, Any, Dict

try:
    import nats
except ImportError:
    nats = None  # type: ignore

logger = logging.getLogger(__name__)


class SharedNatsPublisher:
    """
    Publishes cross-plane domain events to verevon-nats.
    
    Thread-safe async API. If NATS connection unavailable at init, gracefully
    continues without publishing.
    """

    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "ingestion-service",
    ):
        """
        Initialize shared NATS publisher.
        
        Args:
            nats_url: NATS broker URL (e.g., "nats://verevon-nats:4222")
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
        Connect to shared NATS and ensure AQENCIA_INGESTION stream exists.
        
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
                    max_reconnect_attempts=3,
                )
                self.js = self.nc.jetstream()

                # Create or ensure stream exists
                try:
                    from nats.js.api import StreamConfig
                    
                    stream_config = StreamConfig(
                        name="VEREVON_INGESTION",
                        subjects=["verevon.ingestion.>"],
                        max_age=14 * 24 * 60 * 60,
                        max_msgs=100_000,
                        discard="old",
                    )
                    await self.js.add_stream(config=stream_config)
                except Exception as e:
                    # Stream might already exist
                    if "STREAM_EXISTS" not in str(e):
                        logger.warning(f"Failed to create VEREVON_INGESTION stream: {e}")

                logger.info(f"✅ Shared NATS ({self.service_name}): VEREVON_INGESTION stream ready")
                logger.info(f"✅ Connected to shared NATS ({self.service_name}): {self.nats_url}")
                self._initialized = True
                return True

            except Exception as e:
                logger.warning(f"⚠️  Shared NATS unavailable ({self.service_name}): {e}")
                self._initialized = True
                return False

    async def publish_import_started(
        self,
        org_id: str,
        import_id: str,
        source: str,
    ) -> None:
        """Publish when an import starts."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "import_id": import_id,
            "source": source,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.import.started", payload)

    async def publish_import_completed(
        self,
        org_id: str,
        import_id: str,
        source: str,
        document_count: int = 0,
    ) -> None:
        """Publish when an import completes."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "import_id": import_id,
            "source": source,
            "document_count": document_count,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.import.completed", payload)

    async def publish_m365_connected(
        self,
        org_id: str,
        user_id: str,
        provider: str = "microsoft365",
    ) -> None:
        """Publish when M365 gets connected."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "user_id": user_id,
            "provider": provider,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.m365.connected", payload)

    async def publish_m365_disconnected(
        self,
        org_id: str,
        user_id: str,
        provider: str = "microsoft365",
    ) -> None:
        """Publish when M365 gets disconnected."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "user_id": user_id,
            "provider": provider,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.m365.disconnected", payload)

    async def publish_crawl_started(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
    ) -> None:
        """Publish when a crawl starts."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.crawl.started", payload)

    async def publish_crawl_completed(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
        page_count: int = 0,
    ) -> None:
        """Publish when a crawl completes."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "page_count": page_count,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.crawl.completed", payload)

    async def publish_crawl_failed(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
        error: str,
    ) -> None:
        """Publish when a crawl fails."""
        if not self.nc or not self.js:
            return

        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "error": error,
            "timestamp": self._iso_now(),
        }
        await self._publish("verevon.ingestion.crawl.failed", payload)

    async def publish_plain(self, subject: str, payload: dict[str, Any]) -> None:
        """Publish to plain NATS core (not JetStream).

        Used for notification-core subjects (notifications.*) which are
        subscribed via plain conn.Subscribe, not JetStream consumers.
        Fire-and-forget — never raises.
        """
        try:
            if self.nc:
                await self.nc.publish(subject, json.dumps(payload).encode())
        except Exception as e:
            logger.warning(f"Failed to plain-publish {subject}: {e}")

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
