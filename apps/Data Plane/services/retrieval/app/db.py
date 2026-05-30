"""
Postgres helpers for the Retrieval Service.

Used only to fetch fresh source metadata (title, type, source) for documents
referenced by retrieved facts. Qdrant payload is set at index time and may be
stale after a document is re-indexed; this query always reflects current state.

Caching:
  Document source metadata is cached in Redis per (org_id, document_id) for
  10 minutes.  Cache is keyed doc:src:{org_id}:{document_id}.  Documents are
  stable after indexing; TTL prevents stale reads after re-indexing.
"""
from __future__ import annotations

import hashlib
import json
import logging
from typing import Any, Dict, List

import redis.asyncio as aioredis
from sqlalchemy import text
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine

from app.config import settings

logger = logging.getLogger(__name__)

_engine = create_async_engine(settings.database_url, pool_size=3, max_overflow=5)
_session = async_sessionmaker(_engine, expire_on_commit=False)

_redis: aioredis.Redis | None = None
_SOURCES_TTL = 600  # 10 minutes


def _get_cache() -> aioredis.Redis | None:
    global _redis
    if _redis is None and settings.redis_url:
        try:
            _redis = aioredis.from_url(
                settings.redis_url,
                decode_responses=True,
                max_connections=getattr(settings, "redis_max_connections", 20),
                socket_timeout=getattr(settings, "redis_socket_timeout", 5.0),
            )
        except Exception as exc:
            logger.warning("doc-source cache: Redis unavailable — %s", exc)
    return _redis


async def fetch_document_sources(
    org_id: str,
    document_ids: List[str],
) -> List[Dict[str, Any]]:
    """
    Fetch current document metadata for a set of document IDs.

    Returns one dict per document found — may be fewer than requested if a
    document was deleted after indexing but before retrieval.
    Enforces org_id so cross-tenant leakage is impossible even here.

    Checks Redis cache first (TTL 10 min) to avoid a Postgres round-trip on
    repeated retrievals referencing the same source documents.
    """
    if not document_ids:
        return []

    r = _get_cache()
    results: List[Dict[str, Any]] = []
    missing_ids: List[str] = []

    # --- cache read pass ---
    if r is not None:
        for doc_id in document_ids:
            cache_key = f"doc:src:{org_id}:{doc_id}"
            try:
                cached = await r.get(cache_key)
                if cached:
                    results.append(json.loads(cached))
                else:
                    missing_ids.append(doc_id)
            except Exception:
                missing_ids.append(doc_id)
    else:
        missing_ids = list(document_ids)

    if not missing_ids:
        return results

    # --- Postgres fetch for cache misses ---
    async with _session() as session:
        result = await session.execute(
            text("""
                SELECT document_id, source, type, title
                FROM   documents
                WHERE  document_id = ANY(:ids)
                  AND  org_id = :org_id
            """),
            {"ids": missing_ids, "org_id": org_id},
        )
        rows = [dict(row) for row in result.mappings()]

    # --- cache write pass ---
    if r is not None:
        for row in rows:
            cache_key = f"doc:src:{org_id}:{row['document_id']}"
            try:
                await r.set(cache_key, json.dumps(row), ex=_SOURCES_TTL)
            except Exception as exc:
                logger.warning("doc-source cache SET failed: %s", exc)

    results.extend(rows)
    return results
