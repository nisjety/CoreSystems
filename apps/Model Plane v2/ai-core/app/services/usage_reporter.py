"""Usage Reporter — publishes inference usage events to billing-core.

billing-core subscribes to ``usage.>`` on plain NATS core (not JetStream).
Expected payload per billing-core/internal/nats/subscriber.go:
  {
    "event_id":    str,      # UUID4 (billing-core auto-derives from hash if absent)
    "org_id":      str,
    "metric":      str,      # "inference_tokens"
    "quantity":    float,    # tokens_in + tokens_out
    "source":      str,      # "ai-core-v2"
    "occurred_at": str,      # RFC3339 / ISO8601
    "metadata":    dict,     # model, provider, intent, latency_ms …
  }

Fire-and-forget: wraps publish in asyncio.create_task; never raises.
"""

from __future__ import annotations

import asyncio
import json
import logging
import uuid
from datetime import datetime, timezone
from typing import Any

logger = logging.getLogger(__name__)

_SOURCE = "ai-core-v2"
_SUBJECT = "usage.inference"

_conn: Any = None  # nats.aio.client.Client set by init()


def init(nats_conn: Any) -> None:
    """Call after NATS connection is established in main.py lifespan."""
    global _conn
    _conn = nats_conn
    logger.info("usage_reporter initialised subject=%s", _SUBJECT)


async def close() -> None:
    global _conn
    _conn = None


async def _publish(payload: dict[str, Any]) -> None:
    if _conn is None:
        logger.warning("usage_reporter: NATS not initialised — dropping usage event org_id=%s", payload.get("org_id"))
        return
    try:
        await _conn.publish(_SUBJECT, json.dumps(payload).encode())
    except Exception as exc:
        logger.error("usage_reporter: publish failed error=%s payload=%s", exc, payload)


def report(
    *,
    org_id: str,
    metric: str = "inference_tokens",
    quantity: float,
    source: str = _SOURCE,
    metadata: dict[str, Any] | None = None,
) -> None:
    """Schedule a fire-and-forget usage event publish.

    Safe to call from sync or async context; always non-blocking.
    """
    payload: dict[str, Any] = {
        "event_id": str(uuid.uuid4()),
        "org_id": org_id,
        "metric": metric,
        "quantity": quantity,
        "source": source,
        "occurred_at": datetime.now(tz=timezone.utc).isoformat(),
        "metadata": metadata or {},
    }
    try:
        loop = asyncio.get_event_loop()
        loop.create_task(_publish(payload))
    except RuntimeError:
        # No running event loop (e.g. test context) — skip silently
        pass
