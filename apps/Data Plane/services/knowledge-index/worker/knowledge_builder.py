"""
Knowledge Builder — takes a document, chunks it, saves KnowledgeUnits to Postgres,
then publishes embedding jobs to the Redis stream.

Flow:
  Document (from event)
    ↓
    Fetch full content (Documents Service gRPC)
    ↓
  Chunk text
    ↓
  Save KnowledgeUnits to Postgres (status=pending)
    ↓
  Publish to dataplane.knowledge.units.created (→ Embedding Worker)
    ↓
  Update document status → "processing"
"""
from __future__ import annotations

import json
import logging
from typing import Any, Dict

import redis.asyncio as aioredis
from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession

from worker.chunker import chunk_text
from worker.config import settings
from worker.documents_client import get_document_by_id
from worker.knowledge_ids import build_knowledge_id

logger = logging.getLogger(__name__)

STREAM_KU_CREATED = "dataplane.knowledge.units.created"
STREAM_DOC_DELETED = "dataplane.documents.deleted"


# ── DB helpers ────────────────────────────────────────────────────────────────

async def save_knowledge_units(
    db: AsyncSession,
    *,
    document_id: str,
    org_id: str,
    chunks: list[tuple[int, str]],
    doc_metadata: Dict[str, Any],
) -> list[str]:
    """
    Upsert knowledge units. Returns list of deterministic knowledge_ids.
    """
    ids: list[str] = []
    for chunk_index, text_chunk in chunks:
        knowledge_id = build_knowledge_id(document_id, chunk_index)
        result = await db.execute(
            text("""
                INSERT INTO knowledge_units
                    (knowledge_id, document_id, org_id, chunk_index, text, metadata)
                VALUES
                    (:knowledge_id, :doc_id, :org_id, :idx, :text, :meta)
                ON CONFLICT (knowledge_id) DO UPDATE
                SET text = EXCLUDED.text,
                    metadata = EXCLUDED.metadata,
                    embedding_status = 'pending',
                    error_message = NULL
                RETURNING knowledge_id
            """),
            {
                "knowledge_id": knowledge_id,
                "doc_id": document_id,
                "org_id": org_id,
                "idx": chunk_index,
                "text": text_chunk,
                "meta": json.dumps({
                    **doc_metadata,
                    "chunk_index": chunk_index,
                }),
            },
        )
        row = result.mappings().one()
        ids.append(row["knowledge_id"])
    await db.commit()
    return ids


async def delete_knowledge_units(db: AsyncSession, *, document_id: str) -> int:
    result = await db.execute(
        text("DELETE FROM knowledge_units WHERE document_id = :id"), {"id": document_id}
    )
    await db.commit()
    return result.rowcount


async def update_document_status(
    db: AsyncSession, *, document_id: str, status: str
) -> None:
    await db.execute(
        text("UPDATE documents SET status = :status WHERE document_id = :id"),
        {"status": status, "id": document_id},
    )
    await db.commit()


# ── Main processing function ──────────────────────────────────────────────────

async def process_document(
    event: Dict[str, Any],
    db: AsyncSession,
    redis: aioredis.Redis,
) -> None:
    """
    Called for each dataplane.documents.created event.
    """
    document_id: str = event["document_id"]
    org_id: str = event["org_id"]

    logger.info("knowledge-index processing document_id=%s org_id=%s", document_id, org_id)

    # 1. Fetch full content from Documents Service gRPC endpoint
    doc = await get_document_by_id(document_id=document_id, org_id=org_id)
    if not doc:
        logger.warning("document_id=%s not found, skipping", document_id)
        return

    # 2. Update document status → processing
    await update_document_status(db, document_id=document_id, status="processing")

    # 3. Chunk
    chunks = chunk_text(
        doc["content"],
        chunk_size=settings.chunk_size,
        chunk_overlap=settings.chunk_overlap,
    )
    if not chunks:
        logger.warning("document_id=%s produced 0 chunks, marking failed", document_id)
        await update_document_status(db, document_id=document_id, status="failed")
        return

    logger.info("document_id=%s → %d chunks", document_id, len(chunks))

    # 4. Delete stale chunks from a previous crawl of this document, then save fresh ones.
    deleted = await delete_knowledge_units(db, document_id=document_id)
    if deleted:
        logger.info("knowledge-index deleted %d stale chunks for document_id=%s", deleted, document_id)

    doc_metadata = {
        "source": doc.get("source", ""),
        "type": doc.get("type", ""),
        "title": doc.get("title", ""),
        "org_id": org_id,
        **doc.get("metadata", {}),
    }
    knowledge_ids = await save_knowledge_units(
        db,
        document_id=document_id,
        org_id=org_id,
        chunks=chunks,
        doc_metadata=doc_metadata,
    )

    # 5. Publish embedding jobs (one message per knowledge unit)
    for kid, (chunk_index, chunk_text_) in zip(knowledge_ids, chunks):
        await redis.xadd(
            STREAM_KU_CREATED,
            {
                "knowledge_id": kid,
                "document_id": document_id,
                "org_id": org_id,
                "chunk_index": str(chunk_index),
                "text": chunk_text_,
                "metadata": json.dumps(doc_metadata),
            },
        )

    logger.info(
        "knowledge-index published %d embedding jobs for document_id=%s",
        len(knowledge_ids),
        document_id,
    )

async def handle_document_deleted(
    event: Dict[str, Any],
    db: AsyncSession,
) -> None:
    """
    Called for each dataplane.documents.deleted event.
    KnowledgeUnits are CASCADE deleted by Postgres.
    Just log the count for observability.
    """
    document_id = event["document_id"]
    count = await delete_knowledge_units(db, document_id=document_id)
    logger.info(
        "knowledge-index cleaned %d knowledge_units for deleted document_id=%s",
        count,
        document_id,
    )
