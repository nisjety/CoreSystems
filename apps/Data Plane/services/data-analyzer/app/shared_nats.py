"""Shared cross-plane NATS publisher for Data Plane services."""
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
    def __init__(
        self,
        nats_url: str,
        nats_token: str,
        service_name: str = "data-plane",
    ):
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc: Optional[Any] = None
        self.js: Optional[Any] = None
        self._lock = asyncio.Lock()
        self._initialized = False

    async def initialize(self) -> bool:
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
                logger.info(
                    f"🔐 Shared NATS ({self.service_name}): using token authentication"
                )
                self.nc = await nats.connect(
                    self.nats_url,
                    token=self.nats_token,
                    name=f"{self.service_name}-publisher",
                    reconnect_time_wait=2,
                    max_reconnect_attempts=3,
                )
                self.js = self.nc.jetstream()
                try:
                    await self.js.add_stream(
                        name="AQENCIA_DATAPLANE",
                        subjects=["aqencia.data.>"],
                        max_age=14 * 24 * 60 * 60 * 1_000_000_000,
                        max_msgs=100_000,
                        discard="old",
                    )
                except Exception as e:
                    if "STREAM_EXISTS" not in str(e):
                        logger.warning(
                            f"Failed to create AQENCIA_DATAPLANE stream: {e}"
                        )
                logger.info(
                    f"✅ Shared NATS ({self.service_name}): AQENCIA_DATAPLANE stream ready"
                )
                self._initialized = True
                return True
            except Exception as e:
                logger.warning(
                    f"⚠️  Shared NATS unavailable ({self.service_name}): {e}"
                )
                self._initialized = True
                return False

    @staticmethod
    def _iso_now() -> str:
        """Return ISO 8601 timestamp."""
        from datetime import datetime, timezone

        return datetime.now(timezone.utc).isoformat()

    async def _publish(self, subject: str, payload: dict[str, Any]) -> None:
        try:
            if self.js:
                await self.js.publish(subject, json.dumps(payload).encode())
        except Exception as e:
            logger.warning(f"Failed to publish {subject}: {e}")

    async def close(self) -> None:
        try:
            if self.nc:
                await self.nc.close()
                self.nc = None
                self.js = None
        except Exception as e:
            logger.warning(f"Error closing shared NATS connection: {e}")

    async def publish_pdf_conversion_completed(
        self,
        org_id: str,
        document_id: str,
        page_count: int,
        minio_key: str,
    ) -> None:
        if not self.nc or not self.js:
            return
        await self._publish(
            "aqencia.data.document.pdf_conversion_completed",
            {
                "org_id": org_id,
                "document_id": document_id,
                "page_count": page_count,
                "minio_key": minio_key,
                "timestamp": self._iso_now(),
            },
        )

    async def publish_text_extracted(
        self,
        org_id: str,
        document_id: str,
        char_count: int,
        minio_key: str,
    ) -> None:
        if not self.nc or not self.js:
            return
        await self._publish(
            "aqencia.data.document.text_extracted",
            {
                "org_id": org_id,
                "document_id": document_id,
                "char_count": char_count,
                "minio_key": minio_key,
                "timestamp": self._iso_now(),
            },
        )

    async def publish_image_analyzed(
        self,
        org_id: str,
        document_id: str,
        image_count: int,
        minio_key: str,
    ) -> None:
        if not self.nc or not self.js:
            return
        await self._publish(
            "aqencia.data.document.image_analyzed",
            {
                "org_id": org_id,
                "document_id": document_id,
                "image_count": image_count,
                "minio_key": minio_key,
                "timestamp": self._iso_now(),
            },
        )
