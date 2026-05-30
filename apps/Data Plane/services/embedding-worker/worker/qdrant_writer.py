"""
Qdrant Writer — manages the vector collection and upserts knowledge unit vectors.

Collection schema:
  vector: float[], dim=3072  (Azure OpenAI text-embedding-3-large)
  payload:
    knowledge_id  str
    document_id   str
    org_id        str
    chunk_index   int
    text          str          (stored for retrieval display)
    metadata      dict

Why store text in the payload?
  Retrieval service needs to return the actual text of each fact.
  Qdrant payload is the right place — no round-trip to Postgres for
  every retrieval call.
"""
from __future__ import annotations

import logging
from typing import Dict, Any, List

from qdrant_client import QdrantClient
from qdrant_client.http import models as qdrant_models

from worker.config import settings

logger = logging.getLogger(__name__)

VECTOR_DIM = 3072   # Azure OpenAI text-embedding-3-large

_client: QdrantClient | None = None


def get_client() -> QdrantClient:
    global _client
    if _client is None:
        _client = QdrantClient(host=settings.qdrant_host, port=settings.qdrant_port)
    return _client


def ensure_collection() -> None:
    """
    Create the Qdrant collection if it does not exist.
    Idempotent — safe to call on every worker startup.
    """
    client = get_client()
    existing = {c.name for c in client.get_collections().collections}

    if settings.qdrant_collection not in existing:
        client.create_collection(
            collection_name=settings.qdrant_collection,
            vectors_config=qdrant_models.VectorParams(
                size=VECTOR_DIM,
                distance=qdrant_models.Distance.COSINE,
            ),
        )
        # Payload indexes for fast metadata filtering
        for field, schema_type in [
            ("org_id",       qdrant_models.PayloadSchemaType.KEYWORD),
            ("document_id",  qdrant_models.PayloadSchemaType.KEYWORD),
            ("type",         qdrant_models.PayloadSchemaType.KEYWORD),
            ("department",   qdrant_models.PayloadSchemaType.KEYWORD),
            ("language",     qdrant_models.PayloadSchemaType.KEYWORD),
        ]:
            client.create_payload_index(
                collection_name=settings.qdrant_collection,
                field_name=field,
                field_schema=schema_type,
            )
        logger.info(
            "qdrant collection created collection=%s dim=%d",
            settings.qdrant_collection,
            VECTOR_DIM,
        )
    else:
        logger.info(
            "qdrant collection exists collection=%s", settings.qdrant_collection
        )


def upsert_vectors(
    points: List[Dict[str, Any]],
) -> None:
    """
    Upsert a batch of vectors into Qdrant.

    Each point:
      id           str   (knowledge_id used as Qdrant point ID)
      vector       List[float]
      payload      dict
    """
    client = get_client()
    qdrant_points = [
        qdrant_models.PointStruct(
            id=p["id"],
            vector=p["vector"],
            payload=p["payload"],
        )
        for p in points
    ]
    client.upsert(
        collection_name=settings.qdrant_collection,
        points=qdrant_points,
        wait=True,
    )
    logger.debug("qdrant upserted %d vectors", len(qdrant_points))


def delete_vectors_by_document(document_id: str) -> int:
    """
    Delete all vectors belonging to a document.
    Called when a document is deleted — keeps Qdrant in sync.
    """
    client = get_client()
    result = client.delete(
        collection_name=settings.qdrant_collection,
        points_selector=qdrant_models.FilterSelector(
            filter=qdrant_models.Filter(
                must=[
                    qdrant_models.FieldCondition(
                        key="document_id",
                        match=qdrant_models.MatchValue(value=document_id),
                    )
                ]
            )
        ),
        wait=True,
    )
    logger.info(
        "qdrant deleted vectors for document_id=%s result=%s", document_id, result
    )
    return result.result if hasattr(result, "result") else 0
