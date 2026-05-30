"""
Embedding Worker — main event loop.

Subscribes to:
  dataplane.knowledge.units.created  → embed + write to Qdrant + mark done
  dataplane.documents.deleted        → delete Qdrant vectors for document

Batching:
  Knowledge unit messages are accumulated up to BATCH_SIZE before calling
  Cohere (reduces API calls, improves throughput).
"""
from __future__ import annotations

import asyncio
import json
import logging
from time import perf_counter
from typing import Any, Dict, List

import redis.asyncio as aioredis
from sqlalchemy import text
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker

from worker.openai_embed_client import embed_texts
from worker.config import settings
from worker.dead_letter import move_to_dead_letter
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
from worker.qdrant_writer import delete_vectors_by_document, ensure_collection, upsert_vectors
from worker.publisher import publish_document_embedded, publish_document_indexed
from worker.stream_recovery import claim_pending_messages

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s %(message)s",
)
logger = logging.getLogger(__name__)

STREAM_KU_CREATED  = "dataplane.knowledge.units.created"
STREAM_DOC_DELETED = "dataplane.documents.deleted"
GROUP_NAME    = "embedding-worker"
CONSUMER_NAME = "embedding-worker-0"
BLOCK_MS      = 2_000
BATCH         = settings.batch_size
WORKER_NAME   = "embedding-worker"

engine = create_async_engine(settings.database_url, pool_size=5)
SessionLocal = async_sessionmaker(engine, expire_on_commit=False)


# ── DB helpers ────────────────────────────────────────────────────────────────

async def mark_units_done(db, knowledge_ids: List[str]) -> None:
    await db.execute(
        text("""
            UPDATE knowledge_units
            SET embedding_status = 'done'
            WHERE knowledge_id = ANY(:ids)
        """),
        {"ids": knowledge_ids},
    )
    await db.commit()


async def mark_units_failed(db, knowledge_ids: List[str], error: str) -> None:
    await db.execute(
        text("""
            UPDATE knowledge_units
            SET embedding_status = 'failed', error_message = :err
            WHERE knowledge_id = ANY(:ids)
        """),
        {"ids": knowledge_ids, "err": error},
    )
    # Check if all units for each document are now done-or-failed, update doc status
    await db.commit()


async def maybe_mark_document_indexed(db, document_ids: List[str]) -> List[Dict[str, str]]:
    """
    After embedding a batch, check if all knowledge units for a document
    are done → mark document status = indexed.
    
    Returns: List of newly indexed documents with their org_id and title.
    """
    indexed_docs = []
    for doc_id in set(document_ids):
        result = await db.execute(
            text("""
                SELECT
                    COUNT(*) FILTER (WHERE embedding_status != 'done') AS pending_count
                FROM knowledge_units
                WHERE document_id = :doc_id
            """),
            {"doc_id": doc_id},
        )
        row = result.mappings().one()
        if row["pending_count"] == 0:
            doc_result = await db.execute(
                text(
                    """
                    UPDATE documents
                    SET status = 'indexed'
                    WHERE document_id = :id AND status != 'indexed'
                    RETURNING document_id, org_id, title
                    """
                ),
                {"id": doc_id},
            )
            doc_row = doc_result.mappings().one_or_none()
            if doc_row:
                indexed_docs.append({
                    "document_id": doc_row["document_id"],
                    "org_id": doc_row["org_id"],
                    "title": doc_row["title"] or "",
                })
    await db.commit()
    return indexed_docs


# ── Batch processor ───────────────────────────────────────────────────────────

async def process_ku_batch(
    messages: List[tuple[str, Dict[str, Any]]],
    db,
    r: aioredis.Redis,
) -> bool:
    """
    Embed a batch of knowledge unit messages, upsert into Qdrant, update DB.
    """
    msg_ids      : List[str]           = []
    knowledge_ids: List[str]           = []
    texts        : List[str]           = []
    payloads     : List[Dict[str,Any]] = []
    document_ids : List[str]           = []

    for msg_id, fields in messages:
        msg_ids.append(msg_id)
        knowledge_ids.append(fields["knowledge_id"])
        texts.append(fields["text"])
        document_ids.append(fields["document_id"])

        meta = json.loads(fields.get("metadata", "{}"))
        payloads.append({
            "knowledge_id": fields["knowledge_id"],
            "document_id":  fields["document_id"],
            "org_id":       fields["org_id"],
            "chunk_index":  int(fields.get("chunk_index", 0)),
            "text":         fields["text"],
            **meta,
        })

    try:
        # Step 1 — Cohere embed
        vectors = await embed_texts(texts, input_type="search_document")

        # Step 2 — Upsert to Qdrant
        points = [
            {"id": kid, "vector": vec, "payload": payload}
            for kid, vec, payload in zip(knowledge_ids, vectors, payloads)
        ]
        upsert_vectors(points)

        # Step 3 — Mark done in Postgres
        await mark_units_done(db, knowledge_ids)
        indexed_docs = await maybe_mark_document_indexed(db, document_ids)

        # Step 4 — Publish document.embedded to shared NATS for newly indexed documents
        for doc_info in indexed_docs:
            await publish_document_embedded(
                org_id=doc_info["org_id"],
                document_id=doc_info["document_id"],
                embedding_model="text-embedding-3-large",
            )
            await publish_document_indexed(
                org_id=doc_info["org_id"],
                document_id=doc_info["document_id"],
                title=doc_info["title"],
            )

        # Step 5 — Ack messages
        for msg_id in msg_ids:
            await r.xack(STREAM_KU_CREATED, GROUP_NAME, msg_id)

        logger.info("embedded %d units documents=%s", len(knowledge_ids), list(set(document_ids)))
        return True

    except Exception as exc:
        logger.error("embedding batch failed: %s", exc, exc_info=True)
        await mark_units_failed(db, knowledge_ids, str(exc))
        # Messages left unacked for retry
        return False


async def handle_document_deleted(
    msg_id: str, fields: Dict[str, Any], r: aioredis.Redis
) -> None:
    document_id = fields["document_id"]
    delete_vectors_by_document(document_id)
    await r.xack(STREAM_DOC_DELETED, GROUP_NAME, msg_id)
    logger.info("qdrant vectors deleted document_id=%s", document_id)


async def move_claimed_message_to_dlq(
    r: aioredis.Redis,
    *,
    stream_name: str,
    message_id: str,
    fields: Dict[str, Any],
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


# ── Main loop ─────────────────────────────────────────────────────────────────

async def ensure_consumer_groups(r: aioredis.Redis) -> None:
    for stream in (STREAM_KU_CREATED, STREAM_DOC_DELETED):
        try:
            await r.xgroup_create(stream, GROUP_NAME, id="0", mkstream=True)
        except Exception as exc:
            if "BUSYGROUP" not in str(exc):
                raise


async def run() -> None:
    worker_state = WorkerState()
    r = aioredis.from_url(settings.redis_url, decode_responses=True)
    await ensure_consumer_groups(r)
    ensure_collection()   # idempotent Qdrant setup
    admin_server = await start_admin_server(
        settings.admin_host,
        settings.admin_port,
        worker_name=WORKER_NAME,
        state=worker_state,
    )
    worker_state.ready = True
    set_worker_ready(WORKER_NAME, True)

    logger.info("embedding-worker started model=%s", settings.azure_openai_embedding_deployment)

    ku_buffer: List[tuple[str, Dict[str, Any]]] = []

    try:
        while True:
            try:
                pending_ku = await claim_pending_messages(
                    r,
                    STREAM_KU_CREATED,
                    GROUP_NAME,
                    CONSUMER_NAME,
                    min_idle_ms=settings.pending_min_idle_ms,
                )
                pending_del = await claim_pending_messages(
                    r,
                    STREAM_DOC_DELETED,
                    GROUP_NAME,
                    CONSUMER_NAME,
                    min_idle_ms=settings.pending_min_idle_ms,
                )

                if pending_ku:
                    WORKER_PENDING_CLAIMED_TOTAL.labels(
                        worker=WORKER_NAME,
                        stream=STREAM_KU_CREATED,
                    ).inc(len(pending_ku))
                if pending_del:
                    WORKER_PENDING_CLAIMED_TOTAL.labels(
                        worker=WORKER_NAME,
                        stream=STREAM_DOC_DELETED,
                    ).inc(len(pending_del))

                results = await r.xreadgroup(
                    GROUP_NAME,
                    CONSUMER_NAME,
                    streams={STREAM_KU_CREATED: ">", STREAM_DOC_DELETED: ">"},
                    count=BATCH,
                    block=BLOCK_MS,
                )

                if results or pending_ku or pending_del:
                    async with SessionLocal() as db:
                        for pending_message in pending_ku:
                            if pending_message.delivery_count >= settings.max_delivery_attempts:
                                await move_claimed_message_to_dlq(
                                    r,
                                    stream_name=STREAM_KU_CREATED,
                                    message_id=pending_message.message_id,
                                    fields=pending_message.fields,
                                    delivery_count=pending_message.delivery_count,
                                )
                                continue
                            ku_buffer.append((pending_message.message_id, pending_message.fields))

                        for pending_message in pending_del:
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
                                await handle_document_deleted(
                                    pending_message.message_id,
                                    pending_message.fields,
                                    r,
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
                                continue

                        if results:
                            for stream_name, messages in results:
                                if stream_name == STREAM_KU_CREATED:
                                    ku_buffer.extend(messages)
                                elif stream_name == STREAM_DOC_DELETED:
                                    for msg_id, fields in messages:
                                        started_at = perf_counter()
                                        try:
                                            await handle_document_deleted(msg_id, fields, r)
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
                                                msg_id,
                                                exc,
                                                exc_info=True,
                                            )
                                            continue

                        if ku_buffer:
                            started_at = perf_counter()
                            processed_count = len(ku_buffer)
                            try:
                                succeeded = await process_ku_batch(ku_buffer, db, r)
                                WORKER_MESSAGE_DURATION_SECONDS.labels(
                                    worker=WORKER_NAME,
                                    stream=STREAM_KU_CREATED,
                                ).observe(perf_counter() - started_at)
                                WORKER_MESSAGES_TOTAL.labels(
                                    worker=WORKER_NAME,
                                    stream=STREAM_KU_CREATED,
                                    result="acked" if succeeded else "failed",
                                ).inc(processed_count)
                                if not succeeded:
                                    worker_state.last_error = "embedding_batch_failed"
                            finally:
                                ku_buffer.clear()

            except Exception as exc:
                worker_state.last_error = str(exc)
                WORKER_LOOP_ERRORS_TOTAL.labels(worker=WORKER_NAME).inc()
                logger.error("worker loop error: %s", exc, exc_info=True)
                await asyncio.sleep(3)
    finally:
        worker_state.ready = False
        set_worker_ready(WORKER_NAME, False)
        admin_server.close()
        await admin_server.wait_closed()
        await r.close()
        await engine.dispose()


async def main() -> None:
    await run()


if __name__ == "__main__":
    asyncio.run(main())
