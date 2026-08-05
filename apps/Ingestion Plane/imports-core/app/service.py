import asyncio
import hashlib
import logging
import os
import time
from collections.abc import AsyncGenerator
from datetime import datetime, timedelta, timezone
from typing import Any
from uuid import UUID

import httpx
from sqlalchemy import and_, or_, select, update

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


def get_http_client() -> httpx.AsyncClient:
    """Public accessor for the shared httpx client (used by the knowledge-sync
    actions-gateway wiring in main.py)."""
    return _get_client()


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
_data_plane_tokens: dict[str, tuple[str, float]] = {}
_data_plane_token_lock = asyncio.Lock()
_DATA_PLANE_TOKEN_TTL = 240.0


class QuotaCheckUnavailable(RuntimeError):
    """Raised when Control Plane cannot provide an authoritative quota decision."""


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
_JOB_LEASE = timedelta(minutes=15)
_WORKER_ID = f"{os.getenv('HOSTNAME', 'imports-core')}:{os.getpid()}"


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
        try:
            response = await _get_client().post(url, json=payload, timeout=10.0)
            response.raise_for_status()
            body = response.json()
        except (httpx.HTTPError, ValueError) as exc:
            logger.error(
                "quota dependency unavailable org=%s items=%d status=%s",
                org_id,
                items,
                getattr(getattr(exc, "response", None), "status_code", "transport-error"),
            )
            raise QuotaCheckUnavailable("authoritative quota decision unavailable") from exc

        allowed = body.get("allowed") if isinstance(body, dict) else None
        if not isinstance(allowed, bool):
            logger.error("quota dependency returned invalid decision org=%s items=%d", org_id, items)
            raise QuotaCheckUnavailable("authoritative quota decision unavailable")

        # A cached approval can oversubscribe a quota through concurrent jobs. A
        # denial is safe to cache and keeps repeated rejected requests inexpensive.
        if not allowed:
            _quota_cache_set(key, False)
        return allowed

    async def _data_plane_token(self, org_id: str) -> str:
        cached = _data_plane_tokens.get(org_id)
        if cached is not None and time.monotonic() < cached[1]:
            return cached[0]
        if not self._settings.ingestion_service_api_key.strip():
            raise RuntimeError("INGESTION_SERVICE_API_KEY is not configured")

        async with _data_plane_token_lock:
            cached = _data_plane_tokens.get(org_id)
            if cached is not None and time.monotonic() < cached[1]:
                return cached[0]
            response = await _get_client().post(
                f"{self._settings.auth_core_url.rstrip('/')}/api/data-plane/internal-token",
                headers={
                    "x-service-id": self._settings.ingestion_service_id,
                    "x-service-api-key": self._settings.ingestion_service_api_key,
                },
                json={
                    "orgId": org_id,
                    "scopes": ["documents:write"],
                    "reason": "imports-core durable document ingest",
                },
                timeout=5.0,
            )
            response.raise_for_status()
            body = response.json()
            token = body.get("token") if isinstance(body, dict) else None
            if not isinstance(token, str) or not token.strip():
                raise RuntimeError("Auth Core returned an invalid Data Plane token")
            _data_plane_tokens[org_id] = (
                token.strip(),
                time.monotonic() + _DATA_PLANE_TOKEN_TTL,
            )
            return token.strip()

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
                    document_payload=document.model_dump(mode="json"),
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
        try:
            await event_publisher.publish(subject, payload)
        except Exception:
            # Import durability is owned by Postgres/Data Plane. A transient
            # event-bus outage must not strand a job in `running`.
            logger.exception("Failed to publish lifecycle event %s for job %s", subject, job.id)

    async def run_job(
        self, job_id: UUID, documents: list[ImportDocument] | None = None
    ) -> None:
        """Process all documents in the job using bounded parallelism.

        Up to _STORE_CONCURRENCY documents are stored concurrently. Progress
        counters and DB commits are serialised via an asyncio.Lock so they
        stay consistent even when multiple _store_document coroutines finish
        simultaneously.
        """
        async with SessionLocal() as session:
            now = datetime.now(timezone.utc)
            claimed = await session.execute(
                update(ImportJob)
                .where(
                    ImportJob.id == job_id,
                    or_(
                        ImportJob.status == "queued",
                        and_(
                            ImportJob.status == "running",
                            or_(
                                ImportJob.lease_expires_at.is_(None),
                                ImportJob.lease_expires_at < now,
                            ),
                        ),
                    ),
                )
                .values(
                    status="running",
                    started_at=now,
                    lease_owner=_WORKER_ID,
                    lease_expires_at=now + _JOB_LEASE,
                    attempts=ImportJob.attempts + 1,
                )
                .returning(ImportJob)
            )
            job = claimed.scalar_one_or_none()
            if not job:
                await session.rollback()
                return
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
            if documents is None:
                try:
                    documents = [
                        ImportDocument.model_validate(item.document_payload) for item in items
                    ]
                except Exception as exc:
                    job.status = "failed"
                    job.error_message = "durable import payload is missing or invalid"
                    job.completed_at = datetime.now(timezone.utc)
                    job.lease_owner = None
                    job.lease_expires_at = None
                    await session.commit()
                    logger.exception("Cannot recover import job %s", job.id)
                    return

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

                        await self._store_document(job, document, item.id if item else None)

                        async with counter_lock:
                            if item:
                                item.status = "completed"
                                item.completed_at = datetime.now(timezone.utc)
                            job.processed_items += 1
                            job.lease_expires_at = datetime.now(timezone.utc) + _JOB_LEASE
                            if item:
                                item.document_payload = None
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
                            job.lease_expires_at = datetime.now(timezone.utc) + _JOB_LEASE
                            await session.commit()

            # Run all documents concurrently, bounded by the semaphore
            await asyncio.gather(*[
                _process_one(i, doc) for i, doc in enumerate(documents)
            ])

            job.status = "completed" if job.failed_items == 0 else "completed_with_errors"
            job.completed_at = datetime.now(timezone.utc)
            job.lease_owner = None
            job.lease_expires_at = None
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
                        "verevon.notifications.import.completed",
                        {
                            "subscriberId": job.user_id,
                            "orgId": job.org_id,
                            "importId": str(job.id),
                            "source": job.source_type or "",
                            "documentCount": job.processed_items,
                        },
                    )

    async def _store_document(
        self, job: ImportJob, document: ImportDocument, item_id: UUID | None = None
    ) -> None:
        url = (
            f"{self._settings.document_service_url.rstrip('/')}/"
            f"{self._settings.document_service_import_path.lstrip('/')}"
        )
        title = next(
            (
                value.strip()
                for value in (document.title, document.source_name, document.source_id)
                if isinstance(value, str) and value.strip()
            ),
            "Imported document",
        )
        source = document.metadata.get("url") or document.source_name or f"import:{job.source_type}"
        fallback_key = hashlib.sha256(document.text.encode("utf-8")).hexdigest()[:24]
        idempotency_key = f"import:{job.id}:{item_id or fallback_key}"
        payload = {
            "org_id": job.org_id,
            "source": str(source)[:2048],
            "type": job.source_type,
            "title": title,
            "content": document.text,
            "metadata": {
                **document.metadata,
                "import_job_id": str(job.id),
                "source_id": document.source_id,
                "source_name": document.source_name,
                "requested_by": job.user_id,
            },
            "idempotency_key": idempotency_key,
            "ingest_policy": {"zdr_mode": "off", "ephemeral_only": False},
        }
        token = await self._data_plane_token(job.org_id)
        headers = {
            "Authorization": f"Bearer {token}",
            "X-Org-Id": job.org_id,
        }
        response = await _get_client().post(url, json=payload, headers=headers, timeout=30.0)
        response.raise_for_status()

    async def get_job(self, job_id: UUID, org_id: str | None = None) -> ImportJob | None:
        job_id = UUID(str(job_id))
        async with SessionLocal() as session:
            query = select(ImportJob).where(ImportJob.id == job_id)
            if org_id is not None:
                query = query.where(ImportJob.org_id == org_id)
            result = await session.execute(query)
            return result.scalar_one_or_none()

    async def get_job_with_items(
        self, job_id: UUID, org_id: str | None = None
    ) -> tuple[ImportJob | None, list[ImportJobItem]]:
        job_id = UUID(str(job_id))
        async with SessionLocal() as session:
            query = select(ImportJob).where(ImportJob.id == job_id)
            if org_id is not None:
                query = query.where(ImportJob.org_id == org_id)
            result = await session.execute(query)
            job = result.scalar_one_or_none()
            if not job:
                return None, []
            items_result = await session.execute(
                select(ImportJobItem).where(ImportJobItem.job_id == job.id).order_by(ImportJobItem.created_at)
            )
            items = list(items_result.scalars().all())
            return job, items

    async def recoverable_job_ids(self, limit: int = 100) -> list[UUID]:
        """Return queued or abandoned jobs for lease-safe startup recovery."""
        now = datetime.now(timezone.utc)
        async with SessionLocal() as session:
            result = await session.execute(
                select(ImportJob.id)
                .where(
                    or_(
                        ImportJob.status == "queued",
                        and_(
                            ImportJob.status == "running",
                            or_(
                                ImportJob.lease_expires_at.is_(None),
                                ImportJob.lease_expires_at < now,
                            ),
                        ),
                    )
                )
                .order_by(ImportJob.created_at)
                .limit(max(1, min(limit, 1000)))
            )
            return list(result.scalars().all())

    async def create_source_documents(
        self,
        source_type: str,
        connection: dict[str, Any],
        options: dict[str, Any],
    ) -> list[ImportDocument]:
        return await import_from_source(source_type, connection, options)


import_service = ImportService()
