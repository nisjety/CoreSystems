"""
Shared NATS Publisher for Ingestion Plane Services

Provides async cross-plane event publishing to velion-nats broker.
Gracefully handles unavailable NATS (service continues without events).

Usage:
    publisher = SharedNatsPublisher("nats://velion-nats:4222", "token", "service-name")
    if await publisher.initialize():
        await publisher.publish_crawl_completed(org_id, url, page_count)
    # If unavailable, service continues normally (no events published)
"""

import asyncio
import json
import logging
import os
from typing import Optional, Any, Dict

logger = logging.getLogger(__name__)


class SharedNatsPublisher:
    """
    Async NATS JetStream publisher for Ingestion Plane events.
    
    Features:
    - Automatic VELION_INGESTION stream creation on first use
    - Fire-and-forget publishing (async, non-blocking)
    - Graceful degradation: continues if NATS unavailable
    - Subject convention: velion.ingestion.<domain>.<action>
    """

    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "ingestion-service",
    ):
        """
        Initialize publisher with connection details.
        
        Args:
            nats_url: NATS server URL (e.g., nats://velion-nats:4222)
            nats_token: Authentication token
            service_name: Service identifier for logging
        """
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc = None
        self.js = None
        self._stream_created = False
        self._connecting = False

    async def initialize(self) -> bool:
        """
        Connect to shared NATS and ensure stream exists.
        
        Returns:
            True if connected, False if NATS unavailable or disabled.
            
        Note:
            Always returns True to caller for graceful degradation.
            Connection failures are logged but never raised.
        """
        if not self.nats_url:
            logger.debug(f"[{self.service_name}] NATS_SHARED_URL not set — events disabled")
            return False

        if self._connecting:
            return False

        self._connecting = True
        try:
            # Try importing nats library
            try:
                import nats
                from nats.errors import Error as NatsError
            except ImportError:
                logger.warning(
                    f"[{self.service_name}] nats library not installed — events disabled"
                )
                return False

            # Connect to velion-nats
            try:
                self.nc = await nats.connect(
                    self.nats_url,
                    token=self.nats_token,
                    name=self.service_name,
                    connect_timeout=5,
                    max_reconnect_attempts=3,
                )
                self.js = self.nc.jetstream()
                logger.info(
                    f"[{self.service_name}] Connected to shared NATS at {self.nats_url}"
                )
            except Exception as e:
                logger.warning(
                    f"[{self.service_name}] Failed to connect to shared NATS: {e}"
                )
                self._connecting = False
                return False

            # Ensure VELION_INGESTION stream exists
            try:
                await self.js.add_stream(
                    name="VELION_INGESTION",
                    subjects=["velion.ingestion.>"],
                    max_age=14 * 24 * 60 * 60,
                    max_msgs=100_000,
                )
            except Exception:
                # Stream might already exist; ignore
                pass

            self._stream_created = True
            logger.info(f"[{self.service_name}] VELION_INGESTION stream ready")
            self._connecting = False
            return True

        except Exception as e:
            logger.error(f"[{self.service_name}] Initialization failed: {e}")
            self._connecting = False
            return False

    async def publish_crawl_started(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish crawl started event.
        
        Subject: velion.ingestion.crawl.started
        """
        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.crawl.started", payload)

    async def publish_crawl_completed(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
        page_count: int = 0,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish crawl completed event.
        
        Subject: velion.ingestion.crawl.completed
        """
        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "page_count": page_count,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.crawl.completed", payload)

    async def publish_crawl_failed(
        self,
        org_id: str,
        url: str,
        crawl_id: str,
        error: str,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish crawl failed event.
        
        Subject: velion.ingestion.crawl.failed
        """
        payload = {
            "org_id": org_id,
            "url": url,
            "crawl_id": crawl_id,
            "error": error,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.crawl.failed", payload)

    async def publish_m365_connected(
        self,
        org_id: str,
        user_id: str,
        provider: str = "microsoft365",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish M365 provider connected event.
        
        Subject: velion.ingestion.m365.connected
        """
        payload = {
            "org_id": org_id,
            "user_id": user_id,
            "provider": provider,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.m365.connected", payload)

    async def publish_m365_disconnected(
        self,
        org_id: str,
        user_id: str,
        provider: str = "microsoft365",
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish M365 provider disconnected event.
        
        Subject: velion.ingestion.m365.disconnected
        """
        payload = {
            "org_id": org_id,
            "user_id": user_id,
            "provider": provider,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.m365.disconnected", payload)

    async def publish_import_started(
        self,
        org_id: str,
        import_id: str,
        source: str,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish import started event.
        
        Subject: velion.ingestion.import.started
        """
        payload = {
            "org_id": org_id,
            "import_id": import_id,
            "source": source,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.import.started", payload)

    async def publish_import_completed(
        self,
        org_id: str,
        import_id: str,
        source: str,
        document_count: int = 0,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> bool:
        """
        Publish import completed event.
        
        Subject: velion.ingestion.import.completed
        """
        payload = {
            "org_id": org_id,
            "import_id": import_id,
            "source": source,
            "document_count": document_count,
            "service": self.service_name,
            "metadata": metadata or {},
        }
        return await self._publish("velion.ingestion.import.completed", payload)

    async def _publish(
        self,
        subject: str,
        payload: Dict[str, Any],
    ) -> bool:
        """
        Internal publish method.
        
        Publishes to JetStream asynchronously (fire-and-forget).
        Never blocks caller or raises exceptions.
        
        Returns:
            True if published, False if NATS unavailable.
        """
        if not self.js:
            return False

        try:
            message_bytes = json.dumps(payload).encode("utf-8")
            await self.js.publish(subject, message_bytes)
            logger.debug(
                f"[{self.service_name}] Published: {subject} (size: {len(message_bytes)} bytes)"
            )
            return True
        except Exception as e:
            logger.warning(f"[{self.service_name}] Publish failed on {subject}: {e}")
            return False

    async def close(self):
        """
        Close connection to NATS.
        
        Should be called on shutdown.
        """
        if self.nc:
            try:
                await self.nc.close()
                logger.info(f"[{self.service_name}] Closed NATS connection")
            except Exception as e:
                logger.warning(f"[{self.service_name}] Close failed: {e}")
            finally:
                self.nc = None
                self.js = None
