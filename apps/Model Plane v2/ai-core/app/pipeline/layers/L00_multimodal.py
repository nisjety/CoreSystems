"""Layer 0 — Multimodal Normalization.

Runs *before* all other layers (L01-L10).  Detects the modality of the
incoming request and sets ``ctx.modality`` so downstream layers can
branch appropriately without inspecting raw payloads.

Detected modalities
-------------------
TEXT        Plain text chat / completion
DOCUMENT    PDF, DOCX, or image containing a document
IMAGE       Image for generation, vision, or analysis
AUDIO       Audio for TTS, STT, or realtime
VIDEO       Video for generation or analysis
REALTIME    WebSocket / realtime conversation session
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import Modality, PipelineContext

logger = logging.getLogger(__name__)

# Map common MIME prefixes → modality
_MIME_MODALITY: list[tuple[str, Modality]] = [
    ("application/pdf", Modality.DOCUMENT),
    ("application/vnd.openxmlformats-officedocument", Modality.DOCUMENT),
    ("application/msword", Modality.DOCUMENT),
    ("image/", Modality.IMAGE),
    ("audio/", Modality.AUDIO),
    ("video/", Modality.VIDEO),
]

# Intent-type → modality overrides
_INTENT_OVERRIDE: dict[str, Modality] = {
    "image_generation": Modality.IMAGE,
    "speech_tts": Modality.AUDIO,
    "speech_stt": Modality.AUDIO,
    "translation": Modality.TEXT,
}


def _detect_from_mime(mime: str) -> Modality | None:
    """Infer modality from MIME type."""
    lower = mime.lower()
    for prefix, modality in _MIME_MODALITY:
        if lower.startswith(prefix):
            return modality
    return None


def _detect_from_raw(raw: dict[str, Any]) -> Modality | None:
    """Heuristic detection from raw request payload."""
    # Attachments with MIME
    attachments = raw.get("attachments") or raw.get("files") or []
    for att in attachments:
        mime = att.get("mime_type", att.get("content_type", ""))
        detected = _detect_from_mime(mime)
        if detected:
            return detected

    # Explicit content_type field
    content_type = raw.get("content_type") or raw.get("mime_type") or ""
    if content_type:
        detected = _detect_from_mime(content_type)
        if detected:
            return detected

    # Multipart vision messages (OpenAI format with image_url blocks)
    messages = raw.get("messages") or []
    for msg in messages:
        content = msg.get("content")
        if isinstance(content, list):
            for part in content:
                if isinstance(part, dict) and part.get("type") == "image_url":
                    return Modality.IMAGE

    # Realtime session indicator
    if raw.get("realtime") or raw.get("session_type") == "realtime":
        return Modality.REALTIME

    # Audio input
    if raw.get("audio") or raw.get("audio_data") or raw.get("audio_url"):
        return Modality.AUDIO

    # Document URL
    if raw.get("document_url"):
        return Modality.DOCUMENT

    return None


async def run(ctx: PipelineContext) -> PipelineContext:
    """Detect and set the modality for downstream layers."""
    raw = ctx._raw

    # 1. Explicit override in raw request
    explicit = raw.get("modality")
    if explicit:
        try:
            ctx.modality = Modality(explicit)
            logger.debug("L00_modality explicit=%s request_id=%s", ctx.modality.value, ctx.request_id)
            return ctx
        except ValueError:
            logger.warning("L00_unknown_modality value=%s request_id=%s", explicit, ctx.request_id)

    # 2. Intent-based override (if L03 ran in a prior pass, which won't happen at L00)
    if ctx.intent and ctx.intent.value in _INTENT_OVERRIDE:
        ctx.modality = _INTENT_OVERRIDE[ctx.intent.value]
        logger.debug("L00_modality intent_override=%s request_id=%s", ctx.modality.value, ctx.request_id)
        return ctx

    # 3. Auto-detect from payload
    detected = _detect_from_raw(raw)
    if detected:
        ctx.modality = detected
        logger.debug("L00_modality detected=%s request_id=%s", ctx.modality.value, ctx.request_id)
        return ctx

    # 4. Default: text
    ctx.modality = Modality.TEXT
    logger.debug("L00_modality default=text request_id=%s", ctx.request_id)
    return ctx
