"""Rich event streaming — SSE, NATS, and webhook delivery of RunEvents.

Provides:
- EventStream: per-run async generator that emits RunEvents
- SSE endpoint helper: converts EventStream to text/event-stream
- WebhookDelivery: POST events to external webhook URLs with retry
- NATSEventBridge: publish RunEvents to NATS for cross-plane subscribers
"""

from __future__ import annotations

import asyncio
import json
import logging
from collections import defaultdict
from datetime import datetime, timezone
from typing import Any, AsyncGenerator

import httpx

from app.messages.types import (
    MessageType,
    RunEvent,
    RunEventBatch,
    build_event,
)
from app.messages.store import append as store_event
from app.resilience import CircuitBreaker, retry

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Per-run event stream (in-memory pub/sub)
# ---------------------------------------------------------------------------


class EventStream:
    """Per-run event stream with fan-out to multiple subscribers.

    The turn loop pushes events here; SSE endpoints and NATS bridge consume.
    """

    def __init__(self, run_id: str, session_id: str) -> None:
        self._run_id = run_id
        self._session_id = session_id
        self._queues: list[asyncio.Queue[RunEvent | None]] = []
        self._closed = False
        self._seq = 0

    def subscribe(self) -> asyncio.Queue[RunEvent | None]:
        """Create a new subscriber queue."""
        q: asyncio.Queue[RunEvent | None] = asyncio.Queue(maxsize=500)
        self._queues.append(q)
        return q

    def unsubscribe(self, q: asyncio.Queue[RunEvent | None]) -> None:
        """Remove a subscriber queue."""
        try:
            self._queues.remove(q)
        except ValueError:
            pass

    async def emit(self, event: RunEvent) -> None:
        """Push an event to all subscribers and persist."""
        if self._closed:
            return

        # Auto-assign sequence
        self._seq += 1
        event.seq = self._seq

        # Persist to Postgres (fire-and-forget, don't block streaming)
        try:
            await store_event(event)
        except Exception:
            logger.warning("event_persist_failed", extra={"run_id": self._run_id, "seq": self._seq})

        # Fan out to all subscribers
        for q in self._queues:
            try:
                q.put_nowait(event)
            except asyncio.QueueFull:
                logger.warning("event_queue_full", extra={"run_id": self._run_id})

    async def emit_many(self, events: list[RunEvent]) -> None:
        """Push multiple events."""
        for event in events:
            await self.emit(event)

    async def close(self) -> None:
        """Signal all subscribers that the stream is done."""
        self._closed = True
        for q in self._queues:
            try:
                q.put_nowait(None)  # sentinel
            except asyncio.QueueFull:
                pass

    @property
    def sequence(self) -> int:
        return self._seq


# Global registry: run_id → EventStream
_streams: dict[str, EventStream] = {}


def get_or_create_stream(run_id: str, session_id: str) -> EventStream:
    """Get (or create) the EventStream for a run."""
    if run_id not in _streams:
        _streams[run_id] = EventStream(run_id, session_id)
    return _streams[run_id]


def remove_stream(run_id: str) -> None:
    """Remove a run's stream from the registry."""
    _streams.pop(run_id, None)


# ---------------------------------------------------------------------------
# SSE helpers
# ---------------------------------------------------------------------------


async def sse_generator(
    run_id: str,
    session_id: str,
    from_seq: int = 0,
) -> AsyncGenerator[str, None]:
    """Async generator that yields SSE-formatted events.

    Usage:
        @app.get("/runs/{run_id}/events")
        async def events(run_id: str):
            return StreamingResponse(
                sse_generator(run_id, session_id),
                media_type="text/event-stream",
            )
    """
    stream = get_or_create_stream(run_id, session_id)
    queue = stream.subscribe()

    try:
        # First, replay any stored events after from_seq
        if from_seq > 0:
            from app.messages.store import replay
            historical = await replay(run_id, from_seq=from_seq)
            for event in historical:
                yield _format_sse(event)

        # Then stream live events
        while True:
            event = await queue.get()
            if event is None:
                # Stream closed
                yield "event: done\ndata: {}\n\n"
                break
            yield _format_sse(event)

    finally:
        stream.unsubscribe(queue)


def _format_sse(event: RunEvent) -> str:
    """Format a RunEvent as an SSE message."""
    data = event.model_dump(mode="json")
    return f"event: {event.type.value}\ndata: {json.dumps(data)}\n\n"


# ---------------------------------------------------------------------------
# Webhook delivery (with retry + circuit breaker)
# ---------------------------------------------------------------------------

_webhook_breaker = CircuitBreaker(name="webhook", failure_threshold=5, recovery_timeout=120.0)


class WebhookDelivery:
    """Deliver RunEvents to an external webhook URL with retry."""

    def __init__(self, url: str, secret: str = "", timeout: float = 10.0) -> None:
        self._url = url
        self._secret = secret
        self._timeout = timeout
        self._client: httpx.AsyncClient | None = None

    async def _get_client(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = httpx.AsyncClient(timeout=self._timeout)
        return self._client

    @retry(max_retries=3, base_delay=2.0, breaker=_webhook_breaker)
    async def deliver(self, event: RunEvent) -> None:
        """POST a single event to the webhook."""
        client = await self._get_client()
        headers = {"Content-Type": "application/json"}
        if self._secret:
            import hashlib
            import hmac
            body = event.model_dump_json()
            sig = hmac.new(self._secret.encode(), body.encode(), hashlib.sha256).hexdigest()
            headers["X-Webhook-Signature"] = sig
        else:
            body = event.model_dump_json()

        resp = await client.post(self._url, content=body, headers=headers)
        resp.raise_for_status()

    @retry(max_retries=2, base_delay=3.0, breaker=_webhook_breaker)
    async def deliver_batch(self, batch: RunEventBatch) -> None:
        """POST a batch of events to the webhook."""
        client = await self._get_client()
        body = batch.model_dump_json()
        headers = {"Content-Type": "application/json"}
        if self._secret:
            import hashlib
            import hmac
            sig = hmac.new(self._secret.encode(), body.encode(), hashlib.sha256).hexdigest()
            headers["X-Webhook-Signature"] = sig

        resp = await client.post(self._url, content=body, headers=headers)
        resp.raise_for_status()

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None


# ---------------------------------------------------------------------------
# NATS event bridge
# ---------------------------------------------------------------------------


class NATSEventBridge:
    """Bridge RunEvents from EventStream to NATS subjects.

    Subscribes to a run's EventStream and publishes each event
    to ``velion.agent.run.{run_id}.stream`` for cross-plane consumption.
    """

    def __init__(self, nats_mgr: Any) -> None:
        self._nats = nats_mgr
        self._tasks: dict[str, asyncio.Task] = {}

    def start_bridging(self, run_id: str, session_id: str) -> None:
        """Start bridging events for a run."""
        if run_id in self._tasks:
            return
        stream = get_or_create_stream(run_id, session_id)
        queue = stream.subscribe()
        task = asyncio.create_task(self._bridge_loop(run_id, queue, stream))
        self._tasks[run_id] = task

    async def _bridge_loop(
        self,
        run_id: str,
        queue: asyncio.Queue[RunEvent | None],
        stream: EventStream,
    ) -> None:
        """Consume events from the queue and publish to NATS."""
        subject = f"velion.agent.run.{run_id}.stream"
        try:
            while True:
                event = await queue.get()
                if event is None:
                    break
                payload = event.model_dump(mode="json")
                await self._nats.publish_jetstream(subject, payload)
        except Exception:
            logger.warning("nats_bridge_error", extra={"run_id": run_id})
        finally:
            stream.unsubscribe(queue)
            self._tasks.pop(run_id, None)

    async def stop_bridging(self, run_id: str) -> None:
        """Stop bridging events for a run."""
        task = self._tasks.pop(run_id, None)
        if task:
            task.cancel()
            try:
                await task
            except asyncio.CancelledError:
                pass

    async def stop_all(self) -> None:
        """Stop all active bridges."""
        for run_id in list(self._tasks):
            await self.stop_bridging(run_id)
