"""SSE streaming — converts provider chunks into Server-Sent Events."""

from __future__ import annotations

import json
from typing import Any, AsyncIterator

from starlette.responses import StreamingResponse


async def _sse_generator(
    chunks: AsyncIterator[dict[str, Any]],
    request_id: str,
) -> AsyncIterator[str]:
    """Wrap raw chunk dicts into SSE ``data:`` lines."""
    async for chunk in chunks:
        chunk["request_id"] = request_id
        yield f"data: {json.dumps(chunk)}\n\n"


def sse_response(
    chunks: AsyncIterator[dict[str, Any]],
    request_id: str,
) -> StreamingResponse:
    """Build a ``StreamingResponse`` for Server-Sent Events."""
    return StreamingResponse(
        _sse_generator(chunks, request_id),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",
        },
    )
