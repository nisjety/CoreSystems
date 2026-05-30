"""
Knowledge Index Worker — main event loop.

Subscribes to:
  dataplane.documents.created   → build knowledge units + publish embedding jobs
  dataplane.documents.deleted   → clean up orphan knowledge units (belt-and-suspenders)

Uses Redis Consumer Groups so multiple instances can scale horizontally.
"""
from __future__ import annotations

import asyncio
import logging
from time import perf_counter

import redis.asyncio as aioredis
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker

from worker.config import settings
from worker.dead_letter import move_to_dead_letter
from worker.documents_client import close_documents_client
from worker.knowledge_builder import (
    handle_document_deleted,
    process_document,
)
from worker.observability import (
    WORKER_DLQ_TOTAL,
    WORKER_LOOP_ERRORS_TOTAL,
    WORKER_MESSAGE_DURATION_SECONDS,
    WORKER_MESSAGES_TOTAL,
    WORKER_PENDING_CLAIMED_TOTAL,
    WorkerState,
    set_worker_ready,
    start_admin_server,
)
from worker.stream_recovery import claim_pending_messages

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)

# ── Stream / group config ─────────────────────────────────────────────────────

STREAM_DOC_CREATED = "dataplane.documents.created"
STREAM_DOC_DELETED = "dataplane.documents.deleted"
GROUP_NAME = "knowledge-index"
CONSUMER_NAME = "knowledge-index-worker"
BLOCK_MS = 2_000   # long-poll timeout
BATCH = 10         # messages per poll
WORKER_NAME = "knowledge-index"

engine = create_async_engine(settings.database_url, pool_size=5)
SessionLocal = async_sessionmaker(engine, expire_on_commit=False)


async def ensure_consumer_groups(r: aioredis.Redis) -> None:
    for stream in (STREAM_DOC_CREATED, STREAM_DOC_DELETED):
        try:
            await r.xgroup_create(stream, GROUP_NAME, id="0", mkstream=True)
            logger.info("consumer group created stream=%s group=%s", stream, GROUP_NAME)
        except Exception as exc:
            if "BUSYGROUP" in str(exc):
                pass  # already exists
            else:
                raise


async def move_claimed_message_to_dlq(
    r: aioredis.Redis,
    *,
    stream_name: str,
    message_id: str,
    fields: dict[str, str],
    delivery_count: int,
) -> None:
    await move_to_dead_letter(
        r,
        source_stream=stream_name,
        group=GROUP_NAME,
        worker_name=WORKER_NAME,
        message_id=message_id,
        fields=fields,
        delivery_count=delivery_count,
        error="max_delivery_attempts_exceeded",
    )
    await r.xack(stream_name, GROUP_NAME, message_id)
    WORKER_DLQ_TOTAL.labels(worker=WORKER_NAME, stream=stream_name).inc()
    WORKER_MESSAGES_TOTAL.labels(worker=WORKER_NAME, stream=stream_name, result="dlq").inc()


async def process_messages(r: aioredis.Redis, worker_state: WorkerState) -> None:
    while True:
        try:
            pending_created = await claim_pending_messages(
                r,
                STREAM_DOC_CREATED,
                GROUP_NAME,
                CONSUMER_NAME,
                min_idle_ms=settings.pending_min_idle_ms,
                count=BATCH,
            )
            pending_deleted = await claim_pending_messages(
                r,
                STREAM_DOC_DELETED,
                GROUP_NAME,
                CONSUMER_NAME,
                min_idle_ms=settings.pending_min_idle_ms,
                count=BATCH,
            )

            if pending_created:
                WORKER_PENDING_CLAIMED_TOTAL.labels(
                    worker=WORKER_NAME,
                    stream=STREAM_DOC_CREATED,
                ).inc(len(pending_created))
            if pending_deleted:
                WORKER_PENDING_CLAIMED_TOTAL.labels(
                    worker=WORKER_NAME,
                    stream=STREAM_DOC_DELETED,
                ).inc(len(pending_deleted))

            # Read from both streams in one call
            results = await r.xreadgroup(
                GROUP_NAME,
                CONSUMER_NAME,
                streams={STREAM_DOC_CREATED: ">", STREAM_DOC_DELETED: ">"},
                count=BATCH,
                block=BLOCK_MS,
            )
            if not results and not pending_created and not pending_deleted:
                continue

            async with SessionLocal() as db:
                if pending_created:
                    logger.warning(
                        "reclaimed %d pending document.created events",
                        len(pending_created),
                    )
                    for pending_message in pending_created:
                        if pending_message.delivery_count >= settings.max_delivery_attempts:
                            await move_claimed_message_to_dlq(
                                r,
                                stream_name=STREAM_DOC_CREATED,
                                message_id=pending_message.message_id,
                                fields=pending_message.fields,
                                delivery_count=pending_message.delivery_count,
                            )
                            continue

                        started_at = perf_counter()
                        try:
                            await process_document(pending_message.fields, db, r)
                            await r.xack(
                                STREAM_DOC_CREATED,
                                GROUP_NAME,
                                pending_message.message_id,
                            )
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_CREATED,
                                result="acked",
                            ).inc()
                            WORKER_MESSAGE_DURATION_SECONDS.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_CREATED,
                            ).observe(perf_counter() - started_at)
                        except Exception as exc:
                            worker_state.last_error = str(exc)
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_CREATED,
                                result="failed",
                            ).inc()
                            logger.error(
                                "stream=%s msg_id=%s error=%s",
                                STREAM_DOC_CREATED,
                                pending_message.message_id,
                                exc,
                                exc_info=True,
                            )

                if pending_deleted:
                    logger.warning(
                        "reclaimed %d pending document.deleted events",
                        len(pending_deleted),
                    )
                    for pending_message in pending_deleted:
                        if pending_message.delivery_count >= settings.max_delivery_attempts:
                            await move_claimed_message_to_dlq(
                                r,
                                stream_name=STREAM_DOC_DELETED,
                                message_id=pending_message.message_id,
                                fields=pending_message.fields,
                                delivery_count=pending_message.delivery_count,
                            )
                            continue

                        started_at = perf_counter()
                        try:
                            await handle_document_deleted(pending_message.fields, db)
                            await r.xack(
                                STREAM_DOC_DELETED,
                                GROUP_NAME,
                                pending_message.message_id,
                            )
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_DELETED,
                                result="acked",
                            ).inc()
                            WORKER_MESSAGE_DURATION_SECONDS.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_DELETED,
                            ).observe(perf_counter() - started_at)
                        except Exception as exc:
                            worker_state.last_error = str(exc)
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=STREAM_DOC_DELETED,
                                result="failed",
                            ).inc()
                            logger.error(
                                "stream=%s msg_id=%s error=%s",
                                STREAM_DOC_DELETED,
                                pending_message.message_id,
                                exc,
                                exc_info=True,
                            )

                for stream_name, messages in results:
                    for msg_id, fields in messages:
                        started_at = perf_counter()
                        try:
                            if stream_name == STREAM_DOC_CREATED:
                                await process_document(fields, db, r)
                            elif stream_name == STREAM_DOC_DELETED:
                                await handle_document_deleted(fields, db)

                            # Acknowledge processed message
                            await r.xack(stream_name, GROUP_NAME, msg_id)
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=stream_name,
                                result="acked",
                            ).inc()
                            WORKER_MESSAGE_DURATION_SECONDS.labels(
                                worker=WORKER_NAME,
                                stream=stream_name,
                            ).observe(perf_counter() - started_at)

                        except Exception as exc:
                            worker_state.last_error = str(exc)
                            WORKER_MESSAGES_TOTAL.labels(
                                worker=WORKER_NAME,
                                stream=stream_name,
                                result="failed",
                            ).inc()
                            logger.error(
                                "stream=%s msg_id=%s error=%s",
                                stream_name,
                                msg_id,
                                exc,
                                exc_info=True,
                            )
                            # Leave unacked — will be re-delivered on restart

        except Exception as exc:
            worker_state.last_error = str(exc)
            WORKER_LOOP_ERRORS_TOTAL.labels(worker=WORKER_NAME).inc()
            logger.error("worker loop error: %s", exc, exc_info=True)
            await asyncio.sleep(2)


async def main() -> None:
    logger.info("knowledge-index worker starting")
    worker_state = WorkerState()
    r = aioredis.from_url(settings.redis_url, decode_responses=True)
    await ensure_consumer_groups(r)
    admin_server = await start_admin_server(
        settings.admin_host,
        settings.admin_port,
        worker_name=WORKER_NAME,
        state=worker_state,
    )
    worker_state.ready = True
    set_worker_ready(WORKER_NAME, True)
    try:
        await process_messages(r, worker_state)
    finally:
        worker_state.ready = False
        set_worker_ready(WORKER_NAME, False)
        admin_server.close()
        await admin_server.wait_closed()
        await close_documents_client()
        await r.close()
        await engine.dispose()


if __name__ == "__main__":
    asyncio.run(main())
