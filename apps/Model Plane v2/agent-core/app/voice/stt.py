"""Speech-to-text service — transcribe audio to text.

Pluggable backend: Whisper, Azure Speech, Anthropic, or stub.
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field

from app.voice.config import AudioFormat, STTBackend, VoiceConfig

logger = logging.getLogger(__name__)


class TranscriptionResult(BaseModel):
    """Result of speech-to-text transcription."""

    text: str = ""
    language: str = ""
    confidence: float = 0.0
    duration_seconds: float = 0.0
    segments: list[dict[str, Any]] = Field(default_factory=list)
    error: str | None = None

    @property
    def success(self) -> bool:
        return self.error is None and len(self.text) > 0


class SpeechToTextService:
    """Transcribe audio bytes to text.

    Accepts raw audio (PCM 16kHz mono) and delegates to the
    configured backend.
    """

    def __init__(self, config: VoiceConfig | None = None) -> None:
        self._config = config or VoiceConfig()

    @property
    def backend(self) -> STTBackend:
        return self._config.stt_backend

    async def transcribe(
        self,
        audio: bytes,
        *,
        language: str | None = None,
        format: AudioFormat | None = None,
    ) -> TranscriptionResult:
        """Transcribe audio bytes to text."""
        if not audio:
            return TranscriptionResult(error="empty_audio")

        max_bytes = self._config.max_audio_duration_seconds * self._config.sample_rate * 2
        if len(audio) > max_bytes:
            return TranscriptionResult(error="audio_too_long")

        lang = language or self._config.language
        backend = self._config.stt_backend

        if backend == STTBackend.STUB:
            return self._stub_transcribe(audio, lang)

        # Real backends would be implemented here
        return TranscriptionResult(
            error=f"backend_{backend.value}_not_implemented"
        )

    def _stub_transcribe(self, audio: bytes, language: str) -> TranscriptionResult:
        """Stub backend for testing — returns placeholder text."""
        duration = len(audio) / (self._config.sample_rate * 2)
        return TranscriptionResult(
            text=f"[stub transcription of {len(audio)} bytes]",
            language=language,
            confidence=0.95,
            duration_seconds=duration,
        )
