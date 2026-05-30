"""Run event store — persist and replay RunEvents in Postgres.

Mirrors CC's session storage: events are written to ``run_events``
with a monotonic sequence counter per run. Supports:
- append: write a single event (with atomic seq increment)
- append_batch: write multiple events in a single transaction
- replay: fetch events for a run (optionally from a seq offset)
- latest_seq: get the current sequence for a run
- tail: get the last N events (for resume/teleport)
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

from app.database import get_pool
from app.messages.types import (
    MessageRole,
    MessageType,
    RunEvent,
    RunEventBatch,
)

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Schema (run once at startup via apply_schema)
# ---------------------------------------------------------------------------

_CREATE_TABLE = """
CREATE TABLE IF NOT EXISTS run_events (
    id          BIGSERIAL PRIMARY KEY,
    run_id      TEXT NOT NULL,
    session_id  TEXT NOT NULL,
    seq         INT NOT NULL,
    type        TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'system',
    turn_index  INT NOT NULL DEFAULT 0,
    data        JSONB NOT NULL DEFAULT '{}',
    content     TEXT,
    token_count INT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_run_seq UNIQUE (run_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_run_events_run_id
    ON run_events (run_id, seq);

CREATE INDEX IF NOT EXISTS idx_run_events_session
    ON run_events (session_id, created_at);

CREATE INDEX IF NOT EXISTS idx_run_events_type
    ON run_events (run_id, type);
"""


async def apply_schema() -> None:
    """Ensure the run_events table exists."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(_CREATE_TABLE)
    logger.info("run_events_schema_applied")


# ---------------------------------------------------------------------------
# Sequence counter (Redis-backed for speed, Postgres as source of truth)
# ---------------------------------------------------------------------------

_SEQ_PREFIX = "agent-core-v2:run-seq:"


async def _next_seq_redis(run_id: str) -> int:
    """Atomically increment and return the next sequence number for a run."""
    from app.redis_client import get_redis

    r = await get_redis()
    key = f"{_SEQ_PREFIX}{run_id}"
    return int(await r.incr(key))


async def _sync_seq_from_pg(run_id: str) -> int:
    """Read the max seq from Postgres and seed Redis.

    Called once per run on first event append (or after Redis restart).
    """
    from app.redis_client import get_redis

    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT COALESCE(MAX(seq), 0) AS max_seq FROM run_events WHERE run_id = $1",
            run_id,
        )

    max_seq = row["max_seq"] if row else 0
    r = await get_redis()
    key = f"{_SEQ_PREFIX}{run_id}"
    # SET only if the Redis counter doesn't exist yet (NX)
    await r.set(key, str(max_seq), nx=True)
    return max_seq


async def next_seq(run_id: str) -> int:
    """Get the next monotonic sequence number for a run."""
    return await _next_seq_redis(run_id)


async def latest_seq(run_id: str) -> int:
    """Return the current highest sequence for a run."""
    from app.redis_client import get_redis

    r = await get_redis()
    key = f"{_SEQ_PREFIX}{run_id}"
    val = await r.get(key)
    if val is not None:
        return int(val)
    return await _sync_seq_from_pg(run_id)


# ---------------------------------------------------------------------------
# Write
# ---------------------------------------------------------------------------


async def append(event: RunEvent) -> RunEvent:
    """Persist a single RunEvent, assigning a sequence number if seq==0."""
    if event.seq == 0:
        event.seq = await next_seq(event.run_id)

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO run_events (run_id, session_id, seq, type, role,
                                    turn_index, data, content, token_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (run_id, seq) DO NOTHING
            """,
            event.run_id,
            event.session_id,
            event.seq,
            event.type.value,
            event.role.value,
            event.turn_index,
            json.dumps(event.data),
            event.content,
            event.token_count,
            event.timestamp,
        )

    return event


async def append_batch(events: list[RunEvent]) -> list[RunEvent]:
    """Persist multiple events in a single transaction."""
    if not events:
        return events

    run_id = events[0].run_id
    pool = await get_pool()

    async with pool.acquire() as conn:
        async with conn.transaction():
            for event in events:
                if event.seq == 0:
                    event.seq = await next_seq(run_id)
                await conn.execute(
                    """
                    INSERT INTO run_events (run_id, session_id, seq, type, role,
                                            turn_index, data, content, token_count, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                    ON CONFLICT (run_id, seq) DO NOTHING
                    """,
                    event.run_id,
                    event.session_id,
                    event.seq,
                    event.type.value,
                    event.role.value,
                    event.turn_index,
                    json.dumps(event.data),
                    event.content,
                    event.token_count,
                    event.timestamp,
                )

    return events


# ---------------------------------------------------------------------------
# Read / Replay
# ---------------------------------------------------------------------------


async def replay(
    run_id: str,
    *,
    from_seq: int = 0,
    limit: int = 5000,
    type_filter: MessageType | None = None,
    event_type: str | None = None,
) -> list[RunEvent]:
    """Replay events for a run, optionally from a given sequence."""
    pool = await get_pool()

    query = "SELECT * FROM run_events WHERE run_id = $1 AND seq > $2"
    params: list[Any] = [run_id, from_seq]

    # Accept MessageType or string for type filtering
    effective_type = type_filter.value if type_filter else event_type
    if effective_type is not None:
        query += " AND type = $3"
        params.append(effective_type)

    query += " ORDER BY seq ASC LIMIT $" + str(len(params) + 1)
    params.append(limit)

    async with pool.acquire() as conn:
        rows = await conn.fetch(query, *params)

    return [_row_to_event(row) for row in rows]


async def tail(run_id: str, n: int = 50) -> list[RunEvent]:
    """Get the last N events for a run (for resume/teleport)."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT * FROM (
                SELECT * FROM run_events
                WHERE run_id = $1
                ORDER BY seq DESC
                LIMIT $2
            ) sub ORDER BY seq ASC
            """,
            run_id,
            n,
        )
    return [_row_to_event(row) for row in rows]


async def count_events(run_id: str) -> int:
    """Count total events for a run."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            "SELECT COUNT(*) AS cnt FROM run_events WHERE run_id = $1",
            run_id,
        )
    return row["cnt"] if row else 0


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _row_to_event(row: Any) -> RunEvent:
    """Convert a Postgres row to a RunEvent."""
    data = row["data"]
    if isinstance(data, str):
        data = json.loads(data)

    return RunEvent(
        run_id=row["run_id"],
        session_id=row["session_id"],
        seq=row["seq"],
        type=MessageType(row["type"]),
        role=MessageRole(row["role"]),
        turn_index=row["turn_index"],
        data=data,
        content=row.get("content"),
        token_count=row.get("token_count"),
        timestamp=row["created_at"],
    )
