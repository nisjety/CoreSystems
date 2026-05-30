"""NATS subscriber wiring (ADR-002 envelope)."""
from __future__ import annotations

import logging
from decimal import Decimal
from typing import Any, Dict

from app import nats_client, repository

logger = logging.getLogger(__name__)

TURN_SUBJECT = "velion.cost.turn.recorded"
RUN_UPDATED_SUBJECT = "velion.cost.run.updated"


async def _handle_turn_recorded(envelope: Dict[str, Any]) -> None:
    """Persist a turn cost event delivered via NATS.

    ADR-002 envelope shape:
    {
      "event_id": "...",
      "event_type": "velion.cost.turn.recorded",
      "org_id": "...",
      "ts": "...",
      "data": {
        "run_id": "...",
        "model": "...",
        "input_tokens": N,
        "output_tokens": N,
        "cost_usd": "0.000123",
        "turn_index": 0,
        "metadata": {...}
      }
    }
    """
    data = envelope.get("data") or {}
    org_id = envelope.get("org_id") or data.get("org_id") or ""
    run_id = data.get("run_id")
    if not run_id:
        logger.warning("turn.recorded missing run_id; dropping")
        return

    await repository.insert_run_cost_turn(
        run_id=run_id,
        org_id=org_id,
        model=data.get("model") or "unknown",
        input_tokens=int(data.get("input_tokens") or 0),
        output_tokens=int(data.get("output_tokens") or 0),
        cost_usd=Decimal(str(data.get("cost_usd") or "0")),
        turn_index=data.get("turn_index"),
        metadata=data.get("metadata"),
    )

    agg = await repository.get_run_cost_aggregate(run_id)
    await nats_client.publish(
        RUN_UPDATED_SUBJECT,
        {
            "event_type": RUN_UPDATED_SUBJECT,
            "org_id": org_id,
            "data": {
                "run_id": run_id,
                "total_cost_usd": str(agg["total_cost_usd"]),
                "turn_count": agg["turn_count"],
                "total_input_tokens": agg["total_input_tokens"],
                "total_output_tokens": agg["total_output_tokens"],
            },
        },
    )


async def start() -> None:
    await nats_client.subscribe(
        subject=TURN_SUBJECT,
        durable="cost-core-turn-recorded",
        handler=_handle_turn_recorded,
    )
