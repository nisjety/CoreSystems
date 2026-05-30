"""Usage reporter — publishes LLM usage events to billing-core via controlplane-nats.

billing-core subscribes to ``usage.>`` on controlplane-nats (CONTROL_PLANE_EVENTS
stream) and records usage for billing/quota enforcement.

Expected payload schema (from billing-core/internal/nats/subscriber.go):
  {
    "event_id":    str,   # idempotency key (SHA-256 of org+metric+quantity+ts)
    "org_id":      str,
    "metric":      str,   # e.g. "llm.tokens"
    "quantity":    float, # total tokens (input + output)
    "source":      str,   # "agent-core-v2"
    "occurred_at": str,   # ISO 8601
    "metadata":    dict,  # model, run_id, session_id, input_tokens, output_tokens
  }
"""

from __future__ import annotations

import hashlib
import json
import logging
from datetime import datetime, timezone
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from app.nats_client import NatsManager

logger = logging.getLogger(__name__)

_SOURCE = "agent-core-v2"


class UsageReporter:
    """Fire-and-forget usage event publisher for billing-core."""

    def __init__(self, nats_mgr: NatsManager) -> None:
        self._nats = nats_mgr

    async def report_llm_usage(
        self,
        *,
        org_id: str,
        model: str,
        input_tokens: int,
        output_tokens: int,
        run_id: str,
        session_id: str,
    ) -> None:
        """Publish a usage event to billing-core for LLM token consumption.

        Subject: ``usage.{org_id}.llm`` — matched by billing-core's ``usage.>``
        subscription.
        Silently skips if org_id is empty or falsy (system/internal calls).
        """
        if not org_id:
            return

        quantity = float(input_tokens + output_tokens)
        occurred_at = _iso_now()
        event_id = _derive_event_id(org_id, "llm.tokens", quantity, occurred_at, run_id)

        payload: dict[str, Any] = {
            "event_id": event_id,
            "org_id": org_id,
            "metric": "llm.tokens",
            "quantity": quantity,
            "source": _SOURCE,
            "occurred_at": occurred_at,
            "metadata": {
                "model": model,
                "run_id": run_id,
                "session_id": session_id,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
            },
        }

        subject = f"usage.{org_id}.llm"
        await self._nats.publish_usage_event(subject, payload)
        logger.debug(
            "usage_event_published",
            extra={
                "org_id": org_id,
                "model": model,
                "quantity": quantity,
                "run_id": run_id,
            },
        )


def _iso_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _derive_event_id(
    org_id: str,
    metric: str,
    quantity: float,
    occurred_at: str,
    run_id: str,
) -> str:
    """Deterministic idempotency key from event fields."""
    raw = json.dumps([org_id, metric, quantity, occurred_at, run_id], sort_keys=True)
    return hashlib.sha256(raw.encode()).hexdigest()[:32]
