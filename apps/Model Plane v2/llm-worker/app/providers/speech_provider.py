"""Speech provider — TTS and STT via Azure Speech + OpenAI.

Phase 4.1: ports speech modalities from v1 into llm-worker.

TTS:
  - Azure Speech (Neural TTS): fast, multi-language, SSML support
  - OpenAI gpt-4o-mini-tts: high-quality, English-primary

STT:
  - Azure Speech (Whisper via Azure OpenAI): batch transcription
  - faster-whisper (local): low-latency, CPU-friendly
"""
from __future__ import annotations

import io
import logging
from typing import Any

import httpx

from app.config import get_settings

logger = logging.getLogger(__name__)

_http_client: httpx.AsyncClient | None = None


async def _get_client() -> httpx.AsyncClient:
    global _http_client
    if _http_client is None:
        _http_client = httpx.AsyncClient(timeout=30.0)
    return _http_client


async def close() -> None:
    global _http_client
    if _http_client is not None:
        await _http_client.aclose()
        _http_client = None


# ── TTS ───────────────────────────────────────────────────────────────────────


async def text_to_speech(
    text: str,
    *,
    voice: str = "en-US-JennyNeural",
    output_format: str = "audio-24khz-96kbitrate-mono-mp3",
    provider: str = "azure",
) -> bytes:
    """Convert text to speech audio bytes.

    Args:
        text: Input text (max 5000 chars).
        voice: Voice name (Azure Neural voice ID or OpenAI voice).
        output_format: Audio format for Azure.
        provider: "azure" or "openai".

    Returns:
        Raw audio bytes.
    """
    if len(text) > 5000:
        raise ValueError("TTS input exceeds 5000 character limit")

    if provider == "openai":
        return await _tts_openai(text, voice)
    return await _tts_azure(text, voice, output_format)


async def _tts_azure(text: str, voice: str, output_format: str) -> bytes:
    settings = get_settings()
    # Azure Speech Service uses a dedicated regional endpoint separate from Azure OpenAI.
    # Check for AZURE_SPEECH_ENDPOINT; if absent, surface a clear 503 instead of crashing.
    speech_endpoint = getattr(settings, "azure_speech_endpoint", "") or ""
    speech_key = getattr(settings, "azure_speech_key", "") or settings.azure_openai_api_key
    if not speech_endpoint:
        raise RuntimeError(
            "Azure Speech Service not configured: set AZURE_SPEECH_ENDPOINT and AZURE_SPEECH_KEY"
        )

    # Azure Speech REST API
    url = f"{speech_endpoint}/cognitiveservices/v1"
    headers = {
        "Ocp-Apim-Subscription-Key": speech_key,
        "Content-Type": "application/ssml+xml",
        "X-Microsoft-OutputFormat": output_format,
    }
    ssml = (
        f'<speak version="1.0" xml:lang="en-US">'
        f'<voice name="{voice}">{text}</voice>'
        f"</speak>"
    )

    client = await _get_client()
    resp = await client.post(url, headers=headers, content=ssml.encode("utf-8"))
    resp.raise_for_status()
    logger.info("tts_azure voice=%s bytes=%d", voice, len(resp.content))
    return resp.content


async def _tts_openai(text: str, voice: str) -> bytes:
    settings = get_settings()
    if not settings.openai_api_key:
        raise RuntimeError("OpenAI API key not configured")

    client = await _get_client()
    resp = await client.post(
        "https://api.openai.com/v1/audio/speech",
        headers={
            "Authorization": f"Bearer {settings.openai_api_key}",
            "Content-Type": "application/json",
        },
        json={
            "model": "gpt-4o-mini-tts",
            "input": text,
            "voice": voice or "alloy",
            "response_format": "mp3",
        },
    )
    resp.raise_for_status()
    logger.info("tts_openai voice=%s bytes=%d", voice, len(resp.content))
    return resp.content


# ── STT ───────────────────────────────────────────────────────────────────────


async def speech_to_text(
    audio: bytes,
    *,
    language: str = "en",
    provider: str = "azure",
) -> dict[str, Any]:
    """Transcribe audio bytes to text.

    Returns:
        {"text": str, "language": str, "duration_seconds": float | None}
    """
    if provider == "azure":
        return await _stt_azure(audio, language)
    raise ValueError(f"Unsupported STT provider: {provider}")


async def _stt_azure(audio: bytes, language: str) -> dict[str, Any]:
    settings = get_settings()
    if not settings.azure_openai_endpoint or not settings.azure_openai_api_key:
        raise RuntimeError("Azure OpenAI endpoint/key not configured for STT")

    # Use Azure OpenAI Whisper deployment
    url = (
        f"{settings.azure_openai_endpoint}/openai/deployments/whisper"
        f"/audio/transcriptions?api-version={settings.azure_openai_api_version}"
    )

    client = await _get_client()
    resp = await client.post(
        url,
        headers={"api-key": settings.azure_openai_api_key},
        files={"file": ("audio.mp3", io.BytesIO(audio), "audio/mpeg")},
        data={"language": language, "response_format": "json"},
    )
    resp.raise_for_status()
    data = resp.json()
    logger.info("stt_azure language=%s text_length=%d", language, len(data.get("text", "")))
    return {
        "text": data.get("text", ""),
        "language": language,
        "duration_seconds": data.get("duration"),
    }
