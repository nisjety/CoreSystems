"""Snapshot and teleport — session persistence, resume, and cross-session replay.

Ported from CC's conversationRecovery.ts and session storage patterns.

Features:
- create_snapshot: save run state + messages to Postgres for later resume
- restore_from_snapshot: rebuild a run's state from a snapshot
- teleport: resume a run on a different worker from its last snapshot
- cleanup: expire old snapshots per retention policy
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timedelta, timezone
from typing import Any

from app.database import get_pool
from app.domain import RunRecord, RunStatus
from app.messages.store import replay as replay_events, latest_seq
from app.messages.types import MessageType, RunEvent, build_event

logger = logging.getLogger(__name__)

# Snapshot retention: keep snapshots for 7 days
DEFAULT_RETENTION = timedelta(days=7)

# ---------------------------------------------------------------------------
# Schema
# ---------------------------------------------------------------------------

_CREATE_TABLE = """
CREATE TABLE IF NOT EXISTS run_snapshots (
    id              BIGSERIAL PRIMARY KEY,
    run_id          TEXT NOT NULL,
    session_id      TEXT NOT NULL,
    worker_id       TEXT,
    event_seq       INT NOT NULL,
    turn_index      INT NOT NULL DEFAULT 0,
    run_state       JSONB NOT NULL,
    conversation     JSONB NOT NULL DEFAULT '[]',
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT uq_snapshot_run_seq UNIQUE (run_id, event_seq)
);

CREATE INDEX IF NOT EXISTS idx_snapshots_run_id
    ON run_snapshots (run_id, event_seq DESC);

CREATE INDEX IF NOT EXISTS idx_snapshots_session
    ON run_snapshots (session_id, created_at DESC);
"""


async def apply_schema() -> None:
    """Ensure the run_snapshots table exists."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(_CREATE_TABLE)
    logger.info("run_snapshots_schema_applied")


# ---------------------------------------------------------------------------
# Snapshot create / restore
# ---------------------------------------------------------------------------


async def create_snapshot(
    run: RunRecord,
    *,
    conversation: list[dict[str, Any]] | None = None,
    worker_id: str = "",
    turn_index: int = 0,
    metadata: dict[str, Any] | None = None,
) -> int:
    """Create a snapshot of a run's current state.

    Args:
        run: The current run record.
        conversation: Full conversation history (messages sent to LLM).
        worker_id: ID of the worker that created the snapshot.
        turn_index: Current turn index in the reactive loop.
        metadata: Extra metadata (cost summary, cache stats, etc.).

    Returns:
        The snapshot event_seq.
    """
    seq = await latest_seq(run.id)
    run_state = run.model_dump(mode="json")
    conv = conversation or []

    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO run_snapshots (run_id, session_id, worker_id, event_seq,
                                       turn_index, run_state, conversation, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (run_id, event_seq) DO UPDATE SET
                run_state = EXCLUDED.run_state,
                conversation = EXCLUDED.conversation,
                metadata = EXCLUDED.metadata
            """,
            run.id,
            run.session_id,
            worker_id,
            seq,
            turn_index,
            json.dumps(run_state),
            json.dumps(conv),
            json.dumps(metadata or {}),
        )

    logger.info(
        "snapshot_created",
        extra={"run_id": run.id, "seq": seq, "turn": turn_index},
    )
    return seq


async def get_latest_snapshot(
    run_id: str,
) -> dict[str, Any] | None:
    """Get the most recent snapshot for a run."""
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            """
            SELECT * FROM run_snapshots
            WHERE run_id = $1
            ORDER BY event_seq DESC
            LIMIT 1
            """,
            run_id,
        )
    if row is None:
        return None

    return {
        "run_id": row["run_id"],
        "session_id": row["session_id"],
        "worker_id": row["worker_id"],
        "event_seq": row["event_seq"],
        "turn_index": row["turn_index"],
        "run_state": json.loads(row["run_state"]) if isinstance(row["run_state"], str) else row["run_state"],
        "conversation": json.loads(row["conversation"]) if isinstance(row["conversation"], str) else row["conversation"],
        "metadata": json.loads(row["metadata"]) if isinstance(row["metadata"], str) else row["metadata"],
        "created_at": row["created_at"],
    }


async def restore_from_snapshot(
    run_id: str,
) -> tuple[RunRecord, list[dict[str, Any]], int] | None:
    """Restore a run from its latest snapshot.

    Returns:
        (run_record, conversation, turn_index) or None if no snapshot exists.
    """
    snapshot = await get_latest_snapshot(run_id)
    if snapshot is None:
        return None

    run = RunRecord(**snapshot["run_state"])
    conversation = snapshot["conversation"]
    turn_index = snapshot["turn_index"]

    # Replay any events that happened after the snapshot
    from_seq = snapshot["event_seq"]
    newer_events = await replay_events(run_id, from_seq=from_seq)

    logger.info(
        "snapshot_restored",
        extra={
            "run_id": run_id,
            "snapshot_seq": from_seq,
            "newer_events": len(newer_events),
            "turn_index": turn_index,
        },
    )

    return run, conversation, turn_index


# ---------------------------------------------------------------------------
# Teleport — resume on a different worker
# ---------------------------------------------------------------------------


async def teleport(
    run_id: str,
    new_worker_id: str,
) -> tuple[RunRecord, list[dict[str, Any]], int] | None:
    """Resume a run on a different worker by restoring from snapshot.

    Steps:
    1. Load latest snapshot
    2. Rebuild run state
    3. Return (run, conversation, turn_index) ready for turn_loop

    The caller (execute_run) is responsible for:
    - Acquiring a new lease
    - Emitting a SESSION_RESTORED event
    - Resuming the turn loop from turn_index
    """
    result = await restore_from_snapshot(run_id)
    if result is None:
        logger.warning("teleport_no_snapshot", extra={"run_id": run_id})
        return None

    run, conversation, turn_index = result

    # Update run to reflect new worker
    run.lease_owner = new_worker_id
    run.status = RunStatus.RUNNING

    logger.info(
        "teleport_ready",
        extra={
            "run_id": run_id,
            "new_worker": new_worker_id,
            "resume_turn": turn_index,
        },
    )

    return run, conversation, turn_index


# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------


async def cleanup_expired_snapshots(
    retention: timedelta = DEFAULT_RETENTION,
) -> int:
    """Delete snapshots older than the retention period.

    Returns the number of deleted rows.
    """
    pool = await get_pool()
    cutoff = datetime.now(timezone.utc) - retention

    async with pool.acquire() as conn:
        result = await conn.execute(
            "DELETE FROM run_snapshots WHERE created_at < $1",
            cutoff,
        )

    count = int(result.split()[-1]) if result else 0
    if count > 0:
        logger.info("snapshots_cleaned", extra={"deleted": count, "cutoff": cutoff.isoformat()})
    return count
