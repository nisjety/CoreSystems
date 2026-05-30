"""Cost-core-v2 HTTP client — cost persistence and analytics events.

Fire-and-forget side-effect offload. Never raises on transport errors —
in-process CostTracker remains the source of truth for budget enforcement.
"""

from __future__ import annotations

import logging
from typing import Any

import httpx

from app.config import settings

logger = logging.getLogger(__name__)


class CostClient:
    """Async HTTP client to cost-core-v2 (Model Plane v2).

    Persists cost records (V7) and emits analytics events (V9) that used to
    live inside agent-core. Transport failures are logged at WARNING level
    and swallowed so callers can treat both methods as best-effort.
    """

    def __init__(self) -> None:
        self._client: httpx.AsyncClient | None = None

    async def open(self) -> None:
        self._client = httpx.AsyncClient(
            base_url=settings.cost_core_url,
            timeout=httpx.Timeout(5.0, connect=2.0),
            headers=self._base_headers(),
        )

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    def _base_headers(self) -> dict[str, str]:
        h: dict[str, str] = {}
        if settings.internal_api_key:
            h["x-internal-api-key"] = settings.internal_api_key
        return h

    @property
    def client(self) -> httpx.AsyncClient:
        if self._client is None:
            raise RuntimeError("CostClient not opened")
        return self._client

    # ---- Cost persistence (V7) ----

    async def record_cost(
        self,
        org_id: str,
        run_id: str,
        model: str,
        input_tokens: int,
        output_tokens: int,
        cost_usd: float,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Persist a cost record. Best-effort — never raises."""
        body: dict[str, Any] = {
            "org_id": org_id,
            "run_id": run_id,
            "model": model,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "cost_usd": cost_usd,
        }
        if metadata:
            body["metadata"] = metadata
        try:
            resp = await self.client.post("/v1/cost/record", json=body)
            resp.raise_for_status()
        except Exception as exc:  # noqa: BLE001 — intentional graceful failure
            logger.warning("cost_client.record_cost failed: %s", exc)

    # ---- Analytics events (V9) ----

    async def emit_event(
        self,
        event_type: str,
        org_id: str,
        run_id: str | None = None,
        user_id: str | None = None,
        agent_id: str | None = None,
        props: dict[str, Any] | None = None,
    ) -> None:
        """Emit an analytics event. Best-effort — never raises."""
        body: dict[str, Any] = {
            "event_type": event_type,
            "org_id": org_id,
        }
        if run_id is not None:
            body["run_id"] = run_id
        if user_id is not None:
            body["user_id"] = user_id
        if agent_id is not None:
            body["agent_id"] = agent_id
        if props:
            body["props"] = props
        try:
            resp = await self.client.post("/v1/analytics/events", json=body)
            resp.raise_for_status()
        except Exception as exc:  # noqa: BLE001 — intentional graceful failure
            logger.warning("cost_client.emit_event failed: %s", exc)
