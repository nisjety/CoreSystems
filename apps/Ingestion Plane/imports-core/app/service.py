import asyncio
import logging
import time
from collections.abc import AsyncGenerator
from datetime import datetime, timezone
from typing import Any
from uuid import UUID

import httpx
from sqlalchemy import select

from app.config import get_settings
from app.connectors import import_from_source
from app.db import SessionLocal
from app.events import event_publisher
from app.models import ImportJob, ImportJobItem
from app.progress import progress_hub
from app.schemas import ImportDocument, ProgressEvent


logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Shared HTTP client — initialised in lifespan, avoids per-request TCP handshakes
# ---------------------------------------------------------------------------
http_client: httpx.AsyncClient | None = None


def init_http_client() -> None:
    """Create the shared httpx client. Called from FastAPI lifespan."""
    global http_client
    http_client = httpx.AsyncClient(
        timeout=30.0,
        limits=httpx.Limits(max_connections=20, max_keepalive_connections=10),
    )


async def close_http_client() -> None:
    """Gracefully close the shared httpx client. Called from FastAPI lifespan."""
    global http_client
    if http_client is not None:
        await http_client.aclose()
        http_client = None


def _get_client() -> httpx.AsyncClient:
    if http_client is None:
        raise RuntimeError("HTTP client not initialised — call init_http_client() first")
    return http_client


async def _get_shared_nats_publisher():
    """Get the global shared NATS publisher from main.py."""
    try:
        from app.main import shared_nats_publisher
        return shared_nats_publisher
    except ImportError:
        return None


# ---------------------------------------------------------------------------
# Quota cache — avoids repeated HTTP calls to org-core for the same org
# within the TTL window. Quota decisions are stable per upload session.
# ---------------------------------------------------------------------------
_QUOTA_CACHE_TTL = 60.0  # seconds
_quota_cache: dict[str, tuple[bool, float]] = {}  # key → (allowed, expires_at)


def _quota_cache_key(org_id: str, items: int) -> str:
    return f"{org_id}:{items}"


def _quota_cache_get(key: str) -> bool | None:
    entry = _quota_cache.get(key)
    if entry is None:
        return None
    allowed, expires_at = entry
    if time.monotonic() > expires_at:
        _quota_cache.pop(key, None)
        return None
    return allowed


def _quota_cache_set(key: str, allowed: bool) -> None:
    _quota_cache[key] = (allowed, time.monotonic() + _QUOTA_CACHE_TTL)


# ---------------------------------------------------------------------------
# Parallelism settings for document processing
# ---------------------------------------------------------------------------
# _STORE_CONCURRENCY limits how many _store_document coroutines run at once.
# 5 is a good balance: avoids overwhelming the document-service while still
# giving a significant throughput improvement over serial processing.
_STORE_CONCURRENCY = 5


class ImportService:
    def __init__(self) -> None:
        self._settings = get_settings()

    async def check_quota(self, org_id: str, items: int) -> bool:
        """Check whether org_id is allowed to import `items` documents.

        Results are cached for _QUOTA_CACHE_TTL seconds so rapid consecutive
        calls (e.g., chunked uploads) don't hammer org-core.

        Also checks the in-memory paused-orgs set populated by the
        ControlPlaneSubscriber when billing.quota_exceeded events arrive.
        """
        # Fast-path: check if org is paused due to billing quota exhaustion.
        try:
            from app.main import control_plane_subscriber
            if control_plane_subscriber and control_plane_subscriber.is_org_paused(org_id):
                logger.warning("check_quota DENIED (org paused) org=%s items=%d", org_id, items)
                return False
        except ImportError:
            pass

        key = _quota_cache_key(org_id, items)
        cached = _quota_cache_get(key)
        if cached is not None:
            logger.debug("check_quota cache hit for org=%s items=%d", org_id, items)
            return cached

        url = f"{self._settings.org_service_url}{self._settings.org_service_quota_path}"
        payload = {"org_id": org_id, "operation": "import", "items": items}
        response = await _get_client().post(url, json=payload, timeout=10.0)
        if response.status_code == 404:
            _quota_cache_set(key, True)
            return True
        response.raise_for_status()
        body = response.json()
        allowed = bool(body.get("allowed", True))
        _quota_cache_set(key, allowed)
        return allowed

    async def create_job(
        self,
        org_id: str,
        user_id: str | None,
        source_type: str,
        documents: list[ImportDocument],
        metadata: dict[str, Any] | None = None,
    ) -> UUID:
        async with SessionLocal() as session:
            job = ImportJob(
                org_id=org_id,
                user_id=user_id,
                source_type=source_type,
                status="queued",
                total_items=len(documents),
                metadata_json=metadata or {},
            )
            session.add(job)
            await session.flush()

            for document in documents:
                item = ImportJobItem(
                    job_id=job.id,
                    source_id=document.source_id,
                    source_name=document.source_name,
                    status="queued",
                    metadata_json=document.metadata,
                )
                session.add(item)

            await session.commit()
            
            # Publish import started event to shared NATS for cross-plane subscribers
            nats_pub = await _get_shared_nats_publisher()
            if nats_pub:
                await nats_pub.publish_import_started(
                    org_id=org_id,
                    import_id=str(job.id),
                    source=source_type,
                )
            
            return job.id

    async def _publish_progress(
        self,
        job: ImportJob,
        event_name: str,
        message: str | None = None,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        payload = ProgressEvent(
            event=event_name,
            job_id=job.id,
            status=job.status,
            processed_items=job.processed_items,
            total_items=job.total_items,
            failed_items=job.failed_items,
            message=message,
            metadata=metadata or {},
        ).model_dump(mode="json")
        await progress_hub.publish(job.id, payload)

    async def _publish_lifecycle_event(self, subject: str, job: ImportJob) -> None:
        payload = {
            "job_id": str(job.id),
            "org_id": job.org_id,
            "user_id": job.user_id,
            "source_type": job.source_type,
            "status": job.status,
            "total_items": job.total_items,
            "processed_items": job.processed_items,
            "failed_items": job.failed_items,
            "created_at": job.created_at.isoformat() if job.created_at else None,
            "started_at": job.started_at.isoformat() if job.started_at else None,
            "completed_at": job.completed_at.isoformat() if job.completed_at else None,
        }
        await event_publisher.publish(subject, payload)

    async def run_job(self, job_id: UUID, documents: list[ImportDocument]) -> None:
        """Process all documents in the job using bounded parallelism.

        Up to _STORE_CONCURRENCY documents are stored concurrently. Progress
        counters and DB commits are serialised via an asyncio.Lock so they
        stay consistent even when multiple _store_document coroutines finish
        simultaneously.
        """
        async with SessionLocal() as session:
            job = await session.get(ImportJob, job_id)
            if not job:
                return

            job.status = "running"
            job.started_at = datetime.now(timezone.utc)
            await session.commit()
            await session.refresh(job)

            await self._publish_lifecycle_event("import.started", job)
            await self._publish_progress(job, "import.started", message="Import job started")

            items_result = await session.execute(
                select(ImportJobItem)
                .where(ImportJobItem.job_id == job.id)
                .order_by(ImportJobItem.created_at)
            )
            items = list(items_result.scalars().all())

            semaphore = asyncio.Semaphore(_STORE_CONCURRENCY)
            counter_lock = asyncio.Lock()

            async def _process_one(index: int, document: ImportDocument) -> None:
                item = items[index] if index < len(items) else None
                async with semaphore:
                    try:
                        if item:
                            item.status = "running"
                            async with counter_lock:
                                await session.commit()

                        await self._store_document(job, document)

                        async with counter_lock:
                            if item:
                                item.status = "completed"
                                item.completed_at = datetime.now(timezone.utc)
                            job.processed_items += 1
                            await session.commit()
                            await session.refresh(job)

                        await self._publish_progress(
                            job,
                            "import.progress",
                            metadata={
                                "item": {
                                    "source_id": document.source_id,
                                    "source_name": document.source_name,
                                }
                            },
                        )
                    except Exception as exc:
                        logger.exception("Failed to process import item for job %s", job.id)
                        async with counter_lock:
                            if item:
                                item.status = "failed"
                                item.error_message = str(exc)
                                item.completed_at = datetime.now(timezone.utc)
                            job.failed_items += 1
                            await session.commit()

            # Run all documents concurrently, bounded by the semaphore
            await asyncio.gather(*[
                _process_one(i, doc) for i, doc in enumerate(documents)
            ])

            job.status = "completed" if job.failed_items == 0 else "completed_with_errors"
            job.completed_at = datetime.now(timezone.utc)
            await session.commit()
            await session.refresh(job)

            await self._publish_lifecycle_event("import.completed", job)
            await self._publish_progress(job, "import.completed", message="Import job completed")
            
            # Publish to shared NATS for cross-plane subscribers
            nats_pub = await _get_shared_nats_publisher()
            if nats_pub:
                await nats_pub.publish_import_completed(
                    org_id=job.org_id,
                    import_id=str(job.id),
                    source=job.source_type,
                    document_count=job.processed_items,
                )
                # Notify the user who triggered the import
                if job.user_id:
                    await nats_pub.publish_plain(
                        "velion.notifications.import.completed",
                        {
                            "subscriberId": job.user_id,
                            "orgId": job.org_id,
                            "importId": str(job.id),
                            "source": job.source_type or "",
                            "documentCount": job.processed_items,
                        },
                    )

    async def _store_document(self, job: ImportJob, document: ImportDocument) -> None:
        url = f"{self._settings.document_service_url}{self._settings.document_service_import_path}"
        payload = {
            "org_id": job.org_id,
            "user_id": job.user_id,
            "source": "import-service",
            "source_type": job.source_type,
            "job_id": str(job.id),
            "source_id": document.source_id,
            "source_name": document.source_name,
            "title": document.title,
            "content": document.text,
            "metadata": document.metadata,
        }
        # Reuse the shared client — no per-call TCP handshake
        headers = {
            "X-Internal-Api-Key": self._settings.internal_api_key,
            "X-Org-Id": job.org_id,
            "X-Service-Name": self._settings.import_service_name,
        }
        response = await _get_client().post(url, json=payload, headers=headers, timeout=30.0)
        response.raise_for_status()

    async def get_job(self, job_id: UUID) -> ImportJob | None:
        async with SessionLocal() as session:
            return await session.get(ImportJob, job_id)

    async def get_job_with_items(self, job_id: UUID) -> tuple[ImportJob | None, list[ImportJobItem]]:
        async with SessionLocal() as session:
            job = await session.get(ImportJob, job_id)
            if not job:
                return None, []
            items_result = await session.execute(
                select(ImportJobItem).where(ImportJobItem.job_id == job.id).order_by(ImportJobItem.created_at)
            )
            items = list(items_result.scalars().all())
            return job, items

    async def create_source_documents(
        self,
        source_type: str,
        connection: dict[str, Any],
        options: dict[str, Any],
    ) -> list[ImportDocument]:
        return await import_from_source(source_type, connection, options)


import_service = ImportService()
