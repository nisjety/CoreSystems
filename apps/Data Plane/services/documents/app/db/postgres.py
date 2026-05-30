"""
Async Postgres connection + document CRUD queries.
Uses asyncpg via SQLAlchemy async engine.
"""
from __future__ import annotations

import json
from typing import Any, Dict, List, Optional

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker

from app.config import settings

engine = create_async_engine(
    settings.database_url,
    pool_size=10,
    max_overflow=20,
    echo=False,
)

SessionLocal = async_sessionmaker(engine, expire_on_commit=False)


async def get_db() -> AsyncSession:
    """FastAPI dependency — yields a DB session."""
    async with SessionLocal() as session:
        yield session


# ── CRUD ─────────────────────────────────────────────────────────────────────

async def create_document(
    db: AsyncSession,
    *,
    org_id: str,
    source: str,
    type: str,
    title: str,
    content: str,
    metadata: Dict[str, Any],
    created_by: Optional[str] = None,
) -> Dict[str, Any]:
    # Crawl documents are deduplicated by (org_id, metadata->>'url') via a partial
    # unique index. Re-crawling the same URL updates content in place rather than
    # creating a duplicate row.
    if source == "quarry":
        result = await db.execute(
            text("""
                INSERT INTO documents (org_id, source, type, title, content, metadata, created_by)
                VALUES (:org_id, :source, :type, :title, :content, :metadata, :created_by)
                ON CONFLICT (org_id, (metadata->>'url')) WHERE source = 'quarry'
                DO UPDATE SET
                    title         = EXCLUDED.title,
                    content       = EXCLUDED.content,
                    metadata      = EXCLUDED.metadata,
                    status        = 'pending',
                    error_message = NULL,
                    updated_at    = NOW()
                RETURNING *
            """),
            {
                "org_id": org_id,
                "source": source,
                "type": type,
                "title": title,
                "content": content,
                "metadata": json.dumps(metadata),
                "created_by": created_by,
            },
        )
    else:
        result = await db.execute(
            text("""
                INSERT INTO documents (org_id, source, type, title, content, metadata, created_by)
                VALUES (:org_id, :source, :type, :title, :content, :metadata, :created_by)
                RETURNING *
            """),
            {
                "org_id": org_id,
                "source": source,
                "type": type,
                "title": title,
                "content": content,
                "metadata": json.dumps(metadata),
                "created_by": created_by,
            },
        )
    await db.commit()
    return dict(result.mappings().one())


async def get_document(
    db: AsyncSession, *, document_id: str, org_id: str
) -> Optional[Dict[str, Any]]:
    result = await db.execute(
        text("SELECT * FROM documents WHERE document_id = :id AND org_id = :org_id"),
        {"id": document_id, "org_id": org_id},
    )
    row = result.mappings().one_or_none()
    return dict(row) if row else None


async def get_document_content(
    db: AsyncSession,
    *,
    document_id: str,
    org_id: str,
) -> Optional[Dict[str, Any]]:
    """Return document_id, title, content, and metadata for Word export."""
    result = await db.execute(
        text("""
            SELECT document_id, title, content, metadata
            FROM documents
            WHERE document_id = :id
              AND org_id     = :org_id
        """),
        {"id": document_id, "org_id": org_id},
    )
    row = result.mappings().one_or_none()
    return dict(row) if row else None


async def list_documents(
    db: AsyncSession,
    *,
    org_id: str,
    q: Optional[str] = None,
    type: Optional[str] = None,
    status: Optional[str] = None,
    limit: int = 50,
    offset: int = 0,
) -> List[Dict[str, Any]]:
    filters = "WHERE org_id = :org_id"
    params: Dict[str, Any] = {"org_id": org_id, "limit": limit, "offset": offset}
    if q:
        filters += " AND title ILIKE :q"
        params["q"] = f"%{q}%"
    if type:
        filters += " AND type = :type"
        params["type"] = type
    if status:
        filters += " AND status = :status"
        params["status"] = status

    result = await db.execute(
        text(f"""
            SELECT document_id, org_id, source, type, title, status,
                   metadata, created_at, updated_at
            FROM documents {filters}
            ORDER BY created_at DESC
            LIMIT :limit OFFSET :offset
        """),
        params,
    )
    return [dict(r) for r in result.mappings()]


async def update_document_status(
    db: AsyncSession,
    *,
    document_id: str,
    status: str,
    error_message: Optional[str] = None,
) -> None:
    await db.execute(
        text("""
            UPDATE documents
            SET status = :status, error_message = :error
            WHERE document_id = :id
        """),
        {"id": document_id, "status": status, "error": error_message},
    )
    await db.commit()


async def delete_document(
    db: AsyncSession, *, document_id: str, org_id: str
) -> bool:
    result = await db.execute(
        text("""
            DELETE FROM documents
            WHERE document_id = :id AND org_id = :org_id
            RETURNING document_id
        """),
        {"id": document_id, "org_id": org_id},
    )
    await db.commit()
    return result.rowcount > 0


async def search_documents(
    db: AsyncSession,
    *,
    org_id: str,
    q: str,
    limit: int = 5,
) -> List[Dict[str, Any]]:
    """
    Lightweight title search for autocomplete.
    Only returns completed documents; excludes content and metadata for speed.
    """
    result = await db.execute(
        text("""
            SELECT document_id, title, type, source
            FROM documents
            WHERE org_id  = :org_id
              AND title    ILIKE :q
              AND status   = 'indexed'
            ORDER BY updated_at DESC
            LIMIT :limit
        """),
        {"org_id": org_id, "q": f"%{q}%", "limit": limit},
    )
    return [dict(r) for r in result.mappings()]
