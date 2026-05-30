from __future__ import annotations

import asyncio
import json
from dataclasses import dataclass

from prometheus_client import CONTENT_TYPE_LATEST, Counter, Gauge, Histogram, generate_latest


WORKER_MESSAGES_TOTAL = Counter(
    "dataplane_worker_messages_total",
    "Messages processed by async workers.",
    ["worker", "stream", "result"],
)

WORKER_MESSAGE_DURATION_SECONDS = Histogram(
    "dataplane_worker_message_duration_seconds",
    "Latency for worker stream processing.",
    ["worker", "stream"],
    buckets=(0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0),
)

WORKER_PENDING_CLAIMED_TOTAL = Counter(
    "dataplane_worker_pending_claimed_total",
    "Pending messages reclaimed for retry.",
    ["worker", "stream"],
)

WORKER_DLQ_TOTAL = Counter(
    "dataplane_worker_dlq_total",
    "Messages moved into the worker dead-letter queue.",
    ["worker", "stream"],
)

WORKER_LOOP_ERRORS_TOTAL = Counter(
    "dataplane_worker_loop_errors_total",
    "Top-level worker loop errors.",
    ["worker"],
)

WORKER_READY = Gauge(
    "dataplane_worker_ready",
    "Whether the worker completed startup and is ready to process messages.",
    ["worker"],
)


@dataclass
class WorkerState:
    ready: bool = False
    last_error: str | None = None


def set_worker_ready(worker_name: str, ready: bool) -> None:
    WORKER_READY.labels(worker=worker_name).set(1 if ready else 0)


async def start_admin_server(
    host: str,
    port: int,
    *,
    worker_name: str,
    state: WorkerState,
) -> asyncio.base_events.Server:
    async def _handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            request_line = await reader.readline()
            while True:
                header_line = await reader.readline()
                if not header_line or header_line == b"\r\n":
                    break

            parts = request_line.decode("latin-1").strip().split()
            path = parts[1] if len(parts) >= 2 else "/"

            if path == "/metrics":
                status_line = "HTTP/1.1 200 OK\r\n"
                body = generate_latest()
                content_type = CONTENT_TYPE_LATEST
            elif path in {"/health", "/healthz"}:
                status_line = "HTTP/1.1 200 OK\r\n"
                body = json.dumps({"status": "ok", "worker": worker_name}).encode("utf-8")
                content_type = "application/json"
            elif path == "/readyz":
                ready = state.ready
                status_line = (
                    "HTTP/1.1 200 OK\r\n"
                    if ready
                    else "HTTP/1.1 503 Service Unavailable\r\n"
                )
                body = json.dumps(
                    {
                        "status": "ready" if ready else "not_ready",
                        "worker": worker_name,
                        "last_error": state.last_error,
                    }
                ).encode("utf-8")
                content_type = "application/json"
            else:
                status_line = "HTTP/1.1 404 Not Found\r\n"
                body = json.dumps({"error": "not_found"}).encode("utf-8")
                content_type = "application/json"

            headers = (
                f"Content-Type: {content_type}\r\n"
                f"Content-Length: {len(body)}\r\n"
                "Connection: close\r\n\r\n"
            )
            writer.write(status_line.encode("ascii") + headers.encode("ascii") + body)
            await writer.drain()
        finally:
            writer.close()
            await writer.wait_closed()

    return await asyncio.start_server(_handle, host=host, port=port)