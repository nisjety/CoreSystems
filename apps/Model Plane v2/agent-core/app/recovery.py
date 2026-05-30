"""Conversation recovery — detect and resume stale/crashed runs.

Mirrors CC's utils/conversationRecovery.ts: on startup (or periodically),
detect runs that are stuck in RUNNING status without an active lease.
These represent runs where the worker crashed before completing.

Recovery strategies:
1. RESUME: restart from last checkpoint (for runs with checkpoint_state)
2. FAIL: mark as failed after timeout (for runs too old to recover)
3. REQUEUE: move back to QUEUED for another worker to pick up
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timedelta, timezone
from typing import Any

from app import repository as repo
from app.database import get_pool
from app.domain import RunRecord, RunStatus
from app.redis_client import acquire_lease

logger = logging.getLogger(__name__)

# Runs stuck longer than this are considered stale
STALE_THRESHOLD = timedelta(minutes=15)

# Runs older than this cannot be recovered — mark as failed
MAX_RECOVERY_AGE = timedelta(hours=24)


async def detect_stale_runs() -> list[RunRecord]:
    """Find runs that are stuck in RUNNING without an active lease.

    A run is stale if:
    - status is RUNNING or QUEUED
    - updated_at is older than STALE_THRESHOLD
    - No active lease in Redis (worker crashed)
    """
    pool = await get_pool()
    now = datetime.now(timezone.utc)
    cutoff = now - STALE_THRESHOLD

    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, session_id, user_id, org_id, agent_type, mode,
                   goal, status, checkpoint_index, checkpoint_state,
                     created_at, updated_at, error, lease_owner, metadata
            FROM agent_runs
            WHERE status IN ($1, $2)
              AND updated_at < $3
            ORDER BY updated_at ASC
            LIMIT 50
            """,
            RunStatus.RUNNING.value,
            RunStatus.QUEUED.value,
            cutoff,
        )

    stale: list[RunRecord] = []
    for row in rows:
        # Check if lease is still active in Redis
        lease_owner = row["lease_owner"]
        if lease_owner:
            # If there's a lease owner but it's stale, the worker crashed
            from app.redis_client import get_lease_owner

            active_owner = await get_lease_owner(row["id"])
            if active_owner:
                continue  # Lease is still active, skip

        meta_raw = row["metadata"]
        meta_dict = json.loads(meta_raw) if isinstance(meta_raw, str) else meta_raw

        run = RunRecord(
            id=row["id"],
            session_id=row["session_id"],
            user_id=row["user_id"],
            org_id=row["org_id"],
            goal=row["goal"],
            status=RunStatus(row["status"]),
            checkpoint_index=row["checkpoint_index"] or 0,
            metadata=meta_dict or {},
            created_at=row["created_at"],
            updated_at=row["updated_at"],
        )
        stale.append(run)

    if stale:
        logger.info(
            "stale_runs_detected",
            extra={"count": len(stale), "run_ids": [r.id for r in stale[:5]]},
        )

    return stale


async def recover_run(run: RunRecord, worker_id: str) -> str:
    """Attempt to recover a stale run.

    Returns the recovery strategy used: 'resumed', 'failed', or 'requeued'.
    """
    now = datetime.now(timezone.utc)
    age = now - run.created_at

    # Too old to recover — mark as failed
    if age > MAX_RECOVERY_AGE:
        await repo.update_run_status(
            run.id,
            RunStatus.FAILED,
            error="Recovery failed: run exceeded maximum recovery age",
            lease_owner=None,
        )
        logger.info(
            "stale_run_failed",
            extra={"run_id": run.id, "age_hours": age.total_seconds() / 3600},
        )
        return "failed"

    # Has checkpoint — try to resume from it
    if run.checkpoint_index > 0:
        # Try to acquire lease for recovery
        if await acquire_lease(run.id, worker_id):
            await repo.update_run_status(
                run.id,
                RunStatus.RUNNING,
                lease_owner=worker_id,
            )
            logger.info(
                "stale_run_resumed",
                extra={
                    "run_id": run.id,
                    "checkpoint": run.checkpoint_index,
                },
            )
            return "resumed"

    # No checkpoint or can't acquire lease — requeue
    await repo.update_run_status(
        run.id,
        RunStatus.QUEUED,
        lease_owner=None,
    )
    logger.info("stale_run_requeued", extra={"run_id": run.id})
    return "requeued"


async def run_recovery_sweep(worker_id: str) -> dict[str, int]:
    """Perform a full recovery sweep — called on startup and periodically.

    Returns summary of actions taken.
    """
    stale = await detect_stale_runs()
    summary: dict[str, int] = {"resumed": 0, "failed": 0, "requeued": 0}

    for run in stale:
        try:
            strategy = await recover_run(run, worker_id)
            summary[strategy] = summary.get(strategy, 0) + 1
        except Exception as exc:
            logger.error(
                "recovery_failed",
                extra={"run_id": run.id, "error": str(exc)},
            )

    if any(v > 0 for v in summary.values()):
        logger.info("recovery_sweep_completed", extra=summary)

    return summary
