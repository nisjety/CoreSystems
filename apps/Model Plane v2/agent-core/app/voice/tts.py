"""Text-to-speech service — synthesize audio from text.

Pluggable backend: Azure Speech, ElevenLabs, or stub.
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field

from app.voice.config import AudioFormat, TTSBackend, VoiceConfig

logger = logging.getLogger(__name__)


class SynthesisResult(BaseModel):
    """Result of text-to-speech synthesis."""

    audio: bytes = b""
    format: AudioFormat = AudioFormat.PCM_16KHZ_MONO
    duration_seconds: float = 0.0
    voice: str = ""
    error: str | None = None

    model_config = {"arbitrary_types_allowed": True}

    @property
    def success(self) -> bool:
        return self.error is None and len(self.audio) > 0


class TextToSpeechService:
    """Synthesize text to audio bytes.

    Delegates to the configured TTS backend.
    """

    MAX_TEXT_LENGTH: int = 5000

    def __init__(self, config: VoiceConfig | None = None) -> None:
        self._config = config or VoiceConfig()

    @property
    def backend(self) -> TTSBackend:
        return self._config.tts_backend

    async def synthesize(
        self,
        text: str,
        *,
        voice: str | None = None,
        format: AudioFormat | None = None,
    ) -> SynthesisResult:
        """Synthesize text to audio bytes."""
        if not text.strip():
            return SynthesisResult(error="empty_text")

        if len(text) > self.MAX_TEXT_LENGTH:
            return SynthesisResult(error="text_too_long")

        v = voice or self._config.tts_voice
        fmt = format or self._config.audio_format
        backend = self._config.tts_backend

        if backend == TTSBackend.STUB:
            return self._stub_synthesize(text, v, fmt)

        return SynthesisResult(
            error=f"backend_{backend.value}_not_implemented"
        )

    def _stub_synthesize(
        self, text: str, voice: str, fmt: AudioFormat
    ) -> SynthesisResult:
        """Stub backend for testing."""
        # Generate fake PCM audio (silence) proportional to text length
        duration = len(text) * 0.05  # ~50ms per character
        sample_rate = self._config.sample_rate
        num_samples = int(duration * sample_rate)
        audio = b"\x00\x00" * num_samples

        return SynthesisResult(
            audio=audio,
            format=fmt,
            duration_seconds=duration,
            voice=voice,
        )
