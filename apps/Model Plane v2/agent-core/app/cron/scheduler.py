"""Background cron scheduler — ticks every 60s, fires due jobs via NATS."""

from __future__ import annotations

import asyncio
import logging
from datetime import datetime, timezone

from croniter import croniter

from app.cron import repository
from app.cron.domain import CronTask
from app.nats_client import NatsManager

logger = logging.getLogger(__name__)

_TICK_INTERVAL = 60  # seconds
_task: asyncio.Task | None = None


def compute_next_run(cron_expr: str, base: datetime | None = None) -> datetime:
    """Compute the next fire time from a cron expression."""
    if base is None:
        base = datetime.now(timezone.utc)
    cron = croniter(cron_expr, base)
    return cron.get_next(datetime).replace(tzinfo=timezone.utc)


async def _fire_cron(nats: NatsManager, cron: CronTask) -> None:
    """Publish a NATS command to start a new run from a cron schedule."""
    subject = "velion.agent.cron.fire"
    payload = {
        "cron_id": cron.id,
        "org_id": cron.org_id,
        "session_id": cron.session_id,
        "goal": cron.goal,
        "policy": cron.policy,
        "fired_at": datetime.now(timezone.utc).isoformat(),
    }

    try:
        await nats.publish_jetstream(subject, payload)
        logger.info("cron_fired", extra={"cron_id": cron.id, "name": cron.name})
    except Exception:
        logger.exception("cron_fire_failed", extra={"cron_id": cron.id})


async def _tick(nats: NatsManager) -> None:
    """Single scheduler tick: find due crons, fire them, update next_run."""
    now = datetime.now(timezone.utc)
    due = await repository.list_due_crons(now)

    for cron in due:
        await _fire_cron(nats, cron)
        next_run = compute_next_run(cron.cron_expr, now)
        await repository.update_last_run(cron.id, now, next_run)


async def _loop(nats: NatsManager) -> None:
    """Background loop that ticks every TICK_INTERVAL seconds."""
    while True:
        try:
            await _tick(nats)
        except asyncio.CancelledError:
            break
        except Exception:
            logger.exception("cron_tick_error")
        await asyncio.sleep(_TICK_INTERVAL)


async def start_scheduler(nats: NatsManager) -> None:
    """Start the background cron scheduler."""
    global _task
    if _task is not None:
        return
    _task = asyncio.create_task(_loop(nats), name="cron-scheduler")
    logger.info("cron_scheduler_started")


async def stop_scheduler() -> None:
    """Stop the background cron scheduler."""
    global _task
    if _task is not None:
        _task.cancel()
        try:
            await _task
        except asyncio.CancelledError:
            pass
        _task = None
        logger.info("cron_scheduler_stopped")
