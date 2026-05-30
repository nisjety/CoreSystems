"""NATS handler — subscribes to AI request subjects and publishes responses.

Enables any service in the system to request AI completions via NATS
without needing direct HTTP access to ai-core.

Subjects:
  velion.ai.chat.request    → run_pipeline → velion.ai.chat.response
  velion.ai.complete.request → direct execute → velion.ai.complete.response
"""

from __future__ import annotations

import asyncio
import json
import logging
from typing import Any

import nats
from nats.aio.client import Client as NATSClient, Msg

from app.config import get_settings
from app.pipeline.runner import run_pipeline

logger = logging.getLogger(__name__)

_nc: NATSClient | None = None

SUBJECT_CHAT_REQUEST = "velion.ai.chat.request"
SUBJECT_CHAT_RESPONSE = "velion.ai.chat.response"
SUBJECT_COMPLETE_REQUEST = "velion.ai.complete.request"


async def connect() -> None:
    """Connect to NATS and subscribe to AI request subjects."""
    global _nc
    settings = get_settings()
    try:
        _nc = await asyncio.wait_for(
            nats.connect(settings.nats_url, token=settings.nats_token or None),
            timeout=10,
        )
        logger.info("nats_connected url=%s", settings.nats_url)
        await _nc.subscribe(SUBJECT_CHAT_REQUEST, cb=_handle_chat_request)
        await _nc.subscribe(SUBJECT_COMPLETE_REQUEST, cb=_handle_complete_request)
        logger.info("nats_subscribed subjects=[%s, %s]", SUBJECT_CHAT_REQUEST, SUBJECT_COMPLETE_REQUEST)
    except (asyncio.TimeoutError, Exception) as e:
        logger.warning("nats_connection_failed: %s (proceeding without NATS)", str(e))
        _nc = None


async def close() -> None:
    """Close NATS connection."""
    global _nc
    if _nc:
        await _nc.close()
        _nc = None
        logger.info("nats_closed")


async def publish(subject: str, data: dict[str, Any]) -> None:
    """Publish a JSON payload to a NATS subject."""
    if _nc is None:
        logger.warning("nats_not_connected skip_publish subject=%s", subject)
        return
    await _nc.publish(subject, json.dumps(data).encode())


async def _handle_chat_request(msg: Msg) -> None:
    """Process a chat pipeline request received via NATS."""
    try:
        body = json.loads(msg.data.decode())

        result = await run_pipeline(
            request_id=body.get("request_id", ""),
            message=body.get("message", ""),
            org_id=body.get("org_id", ""),
            session_id=body.get("session_id", ""),
            model=body.get("model", ""),
            provider=body.get("provider", ""),
            temperature=body.get("temperature", 0.7),
            max_tokens=body.get("max_tokens"),
            tools=body.get("tools"),
            context=body.get("context"),
        )

        response = result.model_dump()

        # Reply directly if NATS request/reply pattern
        if msg.reply:
            await _nc.publish(msg.reply, json.dumps(response).encode())  # type: ignore[union-attr]
        else:
            await publish(SUBJECT_CHAT_RESPONSE, response)

        logger.debug("chat_request_handled request_id=%s", body.get("request_id"))

    except Exception:
        logger.exception("chat_request_error")
        error_resp = {"error": "internal_pipeline_error"}
        if msg.reply:
            await _nc.publish(msg.reply, json.dumps(error_resp).encode())  # type: ignore[union-attr]


async def _handle_complete_request(msg: Msg) -> None:
    """Process a direct completion request (bypass pipeline, use reasoning_runtime)."""
    try:
        from reasoning_runtime import execute
        from reasoning_runtime.domain import CompletionRequest, Message, Provider

        body = json.loads(msg.data.decode())

        messages = [
            Message(role=m.get("role", "user"), content=m.get("content", ""))
            for m in body.get("messages", [])
        ]

        req = CompletionRequest(
            request_id=body.get("request_id", ""),
            provider=Provider(body.get("provider", "openai")),
            model_id=body.get("model_id", body.get("model", "gpt-4o-mini")),
            messages=messages,
            temperature=body.get("temperature", 0.7),
            max_tokens=body.get("max_tokens"),
            stream=False,
            org_id=body.get("org_id", ""),
            run_id=body.get("run_id", ""),
        )

        result = await execute(req)
        response = {
            "content": result.content,
            "model": result.model_used,
            "provider": result.provider,
            "tokens_in": result.tokens_in,
            "tokens_out": result.tokens_out,
            "finish_reason": result.finish_reason,
        }

        if msg.reply:
            await _nc.publish(msg.reply, json.dumps(response).encode())  # type: ignore[union-attr]

        logger.debug("complete_request_handled model=%s", result.model_used)

    except Exception:
        logger.exception("complete_request_error")
        if msg.reply:
            await _nc.publish(msg.reply, json.dumps({"error": "execution_error"}).encode())  # type: ignore[union-attr]
