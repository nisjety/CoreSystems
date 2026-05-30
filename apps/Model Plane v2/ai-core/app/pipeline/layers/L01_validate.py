"""Layer 1 — Request validation.

Validates that required fields are present and within bounds.
"""

from __future__ import annotations

import logging

from app.domain import PipelineContext

logger = logging.getLogger(__name__)

_MAX_MESSAGE_LEN = 128_000  # ~32k tokens, generous
_MAX_TOOLS = 64


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw

    message: str = raw.get("message", "")
    if not message or not message.strip():
        raise ValueError("message is required and must not be blank")

    if len(message) > _MAX_MESSAGE_LEN:
        raise ValueError(f"message exceeds {_MAX_MESSAGE_LEN} characters")

    tools = raw.get("tools", [])
    if len(tools) > _MAX_TOOLS:
        raise ValueError(f"at most {_MAX_TOOLS} tools allowed")

    if not ctx.org_id:
        raise ValueError("org_id is required")

    temperature = raw.get("temperature", 0.7)
    if not 0.0 <= temperature <= 2.0:
        raise ValueError("temperature must be between 0.0 and 2.0")

    max_tokens = raw.get("max_tokens")
    if max_tokens is not None and max_tokens < 1:
        raise ValueError("max_tokens must be >= 1")

    logger.debug("L01_validate ok request_id=%s", ctx.request_id)
    return ctx
