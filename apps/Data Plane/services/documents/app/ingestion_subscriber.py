"""
Ingestion Event Subscriber — listens to Quarry crawl lifecycle events.

Subscribes to:
  velion.ingestion.crawl.started   → mark documents as 'processing'
  velion.ingestion.crawl.completed → mark documents as 'active'
  velion.ingestion.crawl.failed    → mark documents as 'error'

Matches documents by metadata->>'crawl_job_id' = crawl_id.
Documents created via /internal/v1/documents carry this field in their metadata
when ingested from a Quarry job (crawl_job_id is set by the ingest-job route).

Uses NATS JetStream with queue groups for load balancing across instances.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Optional

import nats
from nats.js import JetStreamContext
from nats.errors import Error as NatsError
from sqlalchemy import text

from app.db.postgres import SessionLocal

logger = logging.getLogger(__name__)

# Mapping from Quarry NATS event to document status
_EVENT_STATUS: dict[str, str] = {
    "velion.ingestion.crawl.started": "processing",
    "velion.ingestion.crawl.completed": "active",
    "velion.ingestion.crawl.failed": "error",
}


class IngestionSubscriber:
    """Async event subscriber for Quarry ingestion lifecycle events."""

    def __init__(self, nats_url: str, nats_token: str, service_name: str = "documents-service"):
        self.nats_url = nats_url
        self.nats_token = nats_token
        self.service_name = service_name
        self.nc: Optional[nats.NATS] = None
        self.js: Optional[JetStreamContext] = None

    async def initialize(self) -> bool:
        """
        Connect to shared NATS and subscribe to ingestion lifecycle events.

        Returns True on success, False if NATS is unavailable (graceful degradation).
        """
        if not self.nats_url:
            logger.info("IngestionSubscriber: NATS URL not configured — skipping")
            return False

        try:
            self.nc = await nats.connect(
                self.nats_url,
                token=self.nats_token,
                connect_timeout=3,
                max_reconnect_attempts=2,
                reconnect_time_wait=2,
            )
            logger.info("IngestionSubscriber (%s): connected to %s", self.service_name, self.nats_url)

            self.js = self.nc.jetstream()

            # Confirm the VELION_INGESTION stream exists (Quarry creates it on startup)
            try:
                await self.js.stream_info("VELION_INGESTION")
            except NatsError as exc:
                logger.warning(
                    "IngestionSubscriber: VELION_INGESTION stream not found (%s) — "
                    "ensure Quarry has run at least once",
                    exc,
                )
                return False

            subscriptions = [
                ("velion.ingestion.crawl.started", "ingestion-docs-started"),
                ("velion.ingestion.crawl.completed", "ingestion-docs-completed"),
                ("velion.ingestion.crawl.failed", "ingestion-docs-failed"),
            ]
            for subject, queue in subscriptions:
                await self.js.subscribe(
                    subject,
                    queue=queue,
                    cb=self._make_handler(subject),
                    ordered_consumer=False,
                )
                logger.info("  ✅ Subscribed to: %s (queue=%s)", subject, queue)

            logger.info("✅ IngestionSubscriber (%s): listening for crawl events", self.service_name)
            return True

        except Exception as exc:
            logger.warning("⚠️  IngestionSubscriber (%s) failed to initialize: %s", self.service_name, exc)
            return False

    def _make_handler(self, subject: str):
        """Return a message handler bound to the given subject."""
        target_status = _EVENT_STATUS[subject]

        async def handler(msg: Any) -> None:
            try:
                data: dict = json.loads(msg.data.decode("utf-8"))
            except Exception as exc:
                logger.error("IngestionSubscriber: failed to decode message on %s: %s", subject, exc)
                await msg.ack()
                return

            crawl_id: str = data.get("crawl_id") or data.get("job_id") or ""
            error_msg: str = data.get("error", "") if target_status == "error" else ""

            if not crawl_id:
                # Nothing to update without a crawl/job ID — ACK and move on
                logger.debug("IngestionSubscriber: no crawl_id in %s event, skipping", subject)
                await msg.ack()
                return

            try:
                await _update_documents_by_crawl_id(crawl_id, target_status, error_msg or None)
                logger.info(
                    "IngestionSubscriber: crawl_id=%s → status=%s (event=%s)",
                    crawl_id, target_status, subject,
                )
                await msg.ack()
            except Exception as exc:
                logger.error(
                    "IngestionSubscriber: DB update failed for crawl_id=%s status=%s: %s",
                    crawl_id, target_status, exc,
                )
                await msg.nak()  # trigger redelivery

        return handler

    async def close(self) -> None:
        if self.nc:
            await self.nc.close()
            logger.info("IngestionSubscriber (%s): closed", self.service_name)


async def _update_documents_by_crawl_id(
    crawl_id: str,
    status: str,
    error_message: Optional[str] = None,
) -> int:
    """
    Update all documents whose metadata->>'crawl_job_id' matches crawl_id.

    Returns the number of rows updated.
    Skips rows already at the target status to avoid unnecessary writes.
    """
    async with SessionLocal() as db:
        if error_message:
            result = await db.execute(
                text("""
                    UPDATE documents
                    SET    status        = :status,
                           error_message = :error,
                           updated_at    = NOW()
                    WHERE  metadata->>'crawl_job_id' = :crawl_id
                    AND    status != :status
                """),
                {"status": status, "error": error_message, "crawl_id": crawl_id},
            )
        else:
            result = await db.execute(
                text("""
                    UPDATE documents
                    SET    status     = :status,
                           updated_at = NOW()
                    WHERE  metadata->>'crawl_job_id' = :crawl_id
                    AND    status != :status
                """),
                {"status": status, "crawl_id": crawl_id},
            )
        await db.commit()
        return result.rowcount
