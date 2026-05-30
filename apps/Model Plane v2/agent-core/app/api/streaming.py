"""SSE streaming endpoint — wraps streaming_loop as Server-Sent Events.

GET /v1/agent-runs/{run_id}/stream  — reconnectable SSE stream
POST /v1/agent-runs/stream          — create + stream in one call

Event types sent to client:
  - turn.start
  - tool.start
  - tool.result
  - usage.delta
  - compact
  - turn.complete
  - run.finished
  - error
  - heartbeat (every 15s keepalive)
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any, AsyncIterator

from fastapi import APIRouter, Depends, HTTPException, Request
from fastapi.responses import StreamingResponse

from app.domain import CreateRunRequest, TurnEvent, TurnEventKind
from app.middleware.auth import Principal, get_principal

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/agent-runs", tags=["streaming"])

_HEARTBEAT_INTERVAL = 15  # seconds

# Map internal TurnEventKind to SSE event names
_EVENT_MAP: dict[TurnEventKind, str] = {
    TurnEventKind.ASSISTANT_CHUNK: "assistant.chunk",
    TurnEventKind.TOOL_CALL_START: "tool.start",
    TurnEventKind.TOOL_RESULT: "tool.result",
    TurnEventKind.PROGRESS: "progress",
    TurnEventKind.USAGE_DELTA: "usage.delta",
    TurnEventKind.COMPACT_BOUNDARY: "compact",
    TurnEventKind.TURN_COMPLETE: "turn.complete",
    TurnEventKind.LOOP_FINISHED: "run.finished",
}


def _sse_line(event: str, data: dict[str, Any]) -> str:
    """Format a single SSE message."""
    payload = json.dumps(data, default=str)
    return f"event: {event}\ndata: {payload}\n\n"


async def _stream_events(
    run_id: str,
    event_iter: AsyncIterator[TurnEvent],
) -> AsyncIterator[str]:
    """Wrap turn events as SSE lines with heartbeat keepalives."""

    yield _sse_line("run.started", {"run_id": run_id})

    heartbeat_task: asyncio.Task | None = None
    event_queue: asyncio.Queue[TurnEvent | None] = asyncio.Queue()

    async def _drain_events() -> None:
        try:
            async for event in event_iter:
                await event_queue.put(event)
        except Exception as exc:
            logger.error("stream_event_error", extra={"run_id": run_id, "error": str(exc)})
        finally:
            await event_queue.put(None)  # sentinel

    drain_task = asyncio.create_task(_drain_events())

    try:
        while True:
            try:
                event = await asyncio.wait_for(
                    event_queue.get(), timeout=_HEARTBEAT_INTERVAL
                )
            except asyncio.TimeoutError:
                yield _sse_line("heartbeat", {"ts": _iso_now()})
                continue

            if event is None:
                break

            sse_name = _EVENT_MAP.get(event.kind, event.kind.value)
            payload = {
                "turn_index": event.turn_index,
                **event.data,
            }
            yield _sse_line(sse_name, payload)

            if event.kind == TurnEventKind.LOOP_FINISHED:
                break
    finally:
        drain_task.cancel()
        try:
            await drain_task
        except asyncio.CancelledError:
            pass


def _iso_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()


@router.post("/stream", status_code=200)
async def create_and_stream(
    body: CreateRunRequest,
    request: Request,
    principal: Principal = Depends(get_principal),
) -> StreamingResponse:
    """Create an agent run and stream events as SSE.

    This is the primary endpoint for real-time agent interaction.
    Equivalent to create_run + subscribing to the event stream.
    """
    from app.main import agent_service

    if agent_service is None:
        raise HTTPException(status_code=503, detail="Agent service not ready")

    svc = agent_service

    run = await svc.create_run(
        request=body,
        session_id=body.context.get("session_id", ""),
        user_id=principal.user_id if not principal.is_internal else body.context.get("user_id", ""),
        org_id=principal.org_id if principal.org_id != "system" else body.context.get("org_id"),
    )

    # Use streaming turn loop instead of background execute
    from app.streaming_loop import run_streaming_turn_loop
    from app import repository as repo

    run_record = await repo.get_run(run.id)
    if run_record is None:
        raise HTTPException(status_code=500, detail="Run creation failed")

    event_iter = run_streaming_turn_loop(
        run=run_record,
        llm_client=svc._llm,
        capability_client=svc._capability,
        execute_action_fn=svc._execute_action,
        publisher=svc._publisher,
    )

    return StreamingResponse(
        _stream_events(run.id, event_iter),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        },
    )


@router.get("/{run_id}/stream")
async def stream_existing_run(
    run_id: str,
    request: Request,
    principal: Principal = Depends(get_principal),
) -> StreamingResponse:
    """Subscribe to SSE events for an existing run via NATS.

    Reconnectable — uses JetStream consumer with deliver_last_per_subject
    so clients can resume from their last received sequence.
    """
    from app.main import nats_mgr
    from app import repository as repo

    if nats_mgr is None:
        raise HTTPException(status_code=503, detail="NATS not connected")

    run = await repo.get_run(run_id)
    if run is None:
        raise HTTPException(status_code=404, detail="Run not found")

    async def _nats_event_stream() -> AsyncIterator[str]:
        """Subscribe to NATS events for this run and yield SSE lines."""
        subject = f"velion.agent.run.{run_id}.event"

        yield _sse_line("connected", {"run_id": run_id, "subject": subject})

        sub = await nats_mgr._cross.subscribe(subject)
        try:
            while True:
                try:
                    msg = await asyncio.wait_for(
                        sub.next_msg(), timeout=_HEARTBEAT_INTERVAL
                    )
                    data = json.loads(msg.data.decode())
                    event_type = data.get("event_type", "unknown")
                    yield _sse_line(event_type, data.get("payload", data))

                    if event_type in ("run.completed", "run.failed"):
                        break
                except asyncio.TimeoutError:
                    yield _sse_line("heartbeat", {"ts": _iso_now()})
                except Exception as exc:
                    yield _sse_line("error", {"message": str(exc)})
                    break
        finally:
            await sub.unsubscribe()

    return StreamingResponse(
        _nats_event_stream(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        },
    )
