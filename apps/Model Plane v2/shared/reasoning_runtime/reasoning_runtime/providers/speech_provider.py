"""Speech provider (Azure Speech TTS/STT + OpenAI TTS)."""

from __future__ import annotations

import io
import logging
import time
from typing import Any, AsyncIterator

import httpx
import openai

from reasoning_runtime.config import get_config
from reasoning_runtime.domain import CompletionRequest, CompletionResponse, Provider

logger = logging.getLogger(__name__)


def _extract_text(req: CompletionRequest) -> str:
    for m in reversed(req.messages):
        if m.role == "user" and isinstance(m.content, str):
            return m.content
    return ""


# ─── Azure Speech TTS ──────────────────────────────────────────────

async def _azure_tts(req: CompletionRequest) -> CompletionResponse:
    cfg = get_config()
    t0 = time.monotonic()
    text = _extract_text(req)

    voice = req.model_id if req.model_id != "default" else "en-US-JennyNeural"
    ssml = (
        f'<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xml:lang="en-US">'
        f'<voice name="{voice}">{text}</voice></speak>'
    )

    url = f"https://{cfg.azure_speech_region}.tts.speech.microsoft.com/cognitiveservices/v1"

    async with httpx.AsyncClient() as client:
        resp = await client.post(
            url,
            content=ssml,
            headers={
                "Ocp-Apim-Subscription-Key": cfg.azure_speech_key,
                "Content-Type": "application/ssml+xml",
                "X-Microsoft-OutputFormat": "audio-16khz-128kbitrate-mono-mp3",
            },
        )
        resp.raise_for_status()

    import base64

    audio_b64 = base64.b64encode(resp.content).decode("utf-8")
    latency_ms = int((time.monotonic() - t0) * 1000)

    return CompletionResponse(
        request_id=req.request_id,
        content=audio_b64,
        finish_reason="stop",
        model_used=voice,
        provider="azure_speech",
        latency_ms=latency_ms,
        metadata={"type": "audio_base64", "mime_type": "audio/mpeg"},
    )


# ─── OpenAI TTS ────────────────────────────────────────────────────

async def _openai_tts(req: CompletionRequest) -> CompletionResponse:
    cfg = get_config()
    client = openai.AsyncOpenAI(api_key=cfg.openai_api_key)
    t0 = time.monotonic()
    text = _extract_text(req)

    model = req.model_id if req.model_id != "default" else "tts-1"
    resp = await client.audio.speech.create(
        model=model,
        voice="alloy",
        input=text,
    )

    import base64

    audio_bytes = b""
    async for chunk in resp.iter_bytes():
        audio_bytes += chunk

    audio_b64 = base64.b64encode(audio_bytes).decode("utf-8")
    latency_ms = int((time.monotonic() - t0) * 1000)

    return CompletionResponse(
        request_id=req.request_id,
        content=audio_b64,
        finish_reason="stop",
        model_used=model,
        provider="openai",
        latency_ms=latency_ms,
        metadata={"type": "audio_base64", "mime_type": "audio/mpeg"},
    )


# ─── Azure Speech STT ──────────────────────────────────────────────

async def _azure_stt(req: CompletionRequest) -> CompletionResponse:
    cfg = get_config()
    t0 = time.monotonic()

    # Expect audio_data in the last user message content
    audio_data = b""
    for m in reversed(req.messages):
        if m.role == "user" and isinstance(m.content, str):
            import base64

            try:
                audio_data = base64.b64decode(m.content)
            except Exception:
                audio_data = m.content.encode("utf-8")
            break

    url = (
        f"https://{cfg.azure_speech_region}.stt.speech.microsoft.com"
        f"/speech/recognition/conversation/cognitiveservices/v1"
        f"?language=en-US"
    )

    async with httpx.AsyncClient() as client:
        resp = await client.post(
            url,
            content=audio_data,
            headers={
                "Ocp-Apim-Subscription-Key": cfg.azure_speech_key,
                "Content-Type": "audio/wav",
            },
        )
        resp.raise_for_status()

    result = resp.json()
    text = result.get("DisplayText", "")
    latency_ms = int((time.monotonic() - t0) * 1000)

    return CompletionResponse(
        request_id=req.request_id,
        content=text,
        finish_reason="stop",
        model_used="azure-stt",
        provider="azure_speech",
        latency_ms=latency_ms,
        metadata={"type": "transcription"},
    )


# ─── Dispatch ───────────────────────────────────────────────────────

async def complete(req: CompletionRequest) -> CompletionResponse:
    """Route to appropriate speech backend based on model_id hints."""
    model = req.model_id.lower()

    if "stt" in model or "whisper" in model or "transcri" in model:
        return await _azure_stt(req)
    elif "openai" in model or "tts-1" in model:
        return await _openai_tts(req)
    else:
        return await _azure_tts(req)


async def stream(req: CompletionRequest) -> AsyncIterator[dict[str, Any]]:
    """Speech is not streamable — yield single result."""
    resp = await complete(req)
    yield {"type": "content", "content": resp.content}
    yield {
        "type": "done",
        "metadata": {
            "model_used": resp.model_used,
            "finish_reason": "stop",
            "latency_ms": resp.latency_ms,
            **(resp.metadata or {}),
        },
    }
