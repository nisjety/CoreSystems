"""ASR (Automatic Speech Recognition) Service.

Wraps multiple transcription backends:
1. Deepgram SDK  — when `deepgram_api_key` is set and provider="deepgram"
2. Azure Speech  — via reasoning_runtime speech_provider (existing)

Fail-open: Deepgram errors fall back to returning an empty transcript with an
error flag so callers can degrade gracefully.

Phase 3 of the ai-core v2 gap-fill; lazily imports Deepgram SDK so the
service boots even before the package is installed.
"""

from __future__ import annotations

import base64
import logging
from dataclasses import dataclass

logger = logging.getLogger(__name__)

_instance: ASRService | None = None


@dataclass
class TranscriptResult:
    text: str
    confidence: float
    language: str
    provider: str
    error: bool = False
    error_detail: str = ""


class ASRService:
    """Route transcription requests to Deepgram or Azure."""

    async def transcribe(
        self,
        *,
        audio_bytes: bytes,
        provider: str = "deepgram",
        language: str = "en-US",
        mimetype: str = "audio/wav",
        org_id: str = "",
    ) -> TranscriptResult:
        """Transcribe audio bytes.  Never raises."""
        if provider == "deepgram":
            return await self._transcribe_deepgram(
                audio_bytes=audio_bytes,
                language=language,
                mimetype=mimetype,
                org_id=org_id,
            )
        # Fallback logged — callers should use the /stt endpoint for Azure
        logger.warning("asr_service unknown_provider=%s falling back to error", provider)
        return TranscriptResult(
            text="",
            confidence=0.0,
            language=language,
            provider=provider,
            error=True,
            error_detail=f"unsupported provider: {provider}",
        )

    async def _transcribe_deepgram(
        self,
        *,
        audio_bytes: bytes,
        language: str,
        mimetype: str,
        org_id: str,
    ) -> TranscriptResult:
        from app.config import get_settings
        settings = get_settings()

        if not settings.deepgram_api_key:
            return TranscriptResult(
                text="",
                confidence=0.0,
                language=language,
                provider="deepgram",
                error=True,
                error_detail="deepgram_api_key not configured",
            )

        try:
            from deepgram import DeepgramClient, PrerecordedOptions  # lazy import

            dg = DeepgramClient(api_key=settings.deepgram_api_key)
            options = PrerecordedOptions(
                model="nova-3",
                language=language,
                smart_format=True,
                punctuate=True,
            )
            # deepgram SDK is synchronous — run in executor
            import asyncio
            loop = asyncio.get_event_loop()
            response = await loop.run_in_executor(
                None,
                lambda: dg.listen.prerecorded.v("1").transcribe_file(
                    {"buffer": audio_bytes, "mimetype": mimetype},
                    options,
                ),
            )
            channel = response.results.channels[0]
            alt = channel.alternatives[0]
            text = alt.transcript
            confidence = alt.confidence

            logger.debug(
                "asr_deepgram_ok len=%d confidence=%.2f org_id=%s",
                len(text), confidence, org_id,
            )
            return TranscriptResult(
                text=text,
                confidence=confidence,
                language=language,
                provider="deepgram",
            )

        except Exception as exc:
            logger.warning("asr_deepgram_failed error=%s org_id=%s", exc, org_id)
            return TranscriptResult(
                text="",
                confidence=0.0,
                language=language,
                provider="deepgram",
                error=True,
                error_detail=str(exc),
            )


def get_asr_service() -> ASRService:
    global _instance
    if _instance is None:
        _instance = ASRService()
    return _instance
