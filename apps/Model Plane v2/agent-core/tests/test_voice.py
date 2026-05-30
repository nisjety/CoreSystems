"""Tests for Phase C5: Voice Input/Output."""

from __future__ import annotations

import pytest

from app.voice.config import AudioFormat, STTBackend, TTSBackend, VoiceConfig
from app.voice.stt import SpeechToTextService, TranscriptionResult
from app.voice.tts import SynthesisResult, TextToSpeechService


# ── Config ─────────────────────────────────────────────────────

class TestVoiceConfig:
    def test_defaults(self):
        cfg = VoiceConfig()
        assert cfg.stt_backend == STTBackend.STUB
        assert cfg.tts_backend == TTSBackend.STUB
        assert cfg.sample_rate == 16000


# ── STT ────────────────────────────────────────────────────────

class TestSTT:
    @pytest.mark.asyncio
    async def test_stub_transcribe(self):
        svc = SpeechToTextService()
        audio = b"\x00" * 3200  # 100ms at 16kHz
        result = await svc.transcribe(audio)
        assert result.success
        assert "stub" in result.text
        assert result.confidence > 0

    @pytest.mark.asyncio
    async def test_empty_audio(self):
        svc = SpeechToTextService()
        result = await svc.transcribe(b"")
        assert not result.success
        assert result.error == "empty_audio"

    @pytest.mark.asyncio
    async def test_audio_too_long(self):
        cfg = VoiceConfig(max_audio_duration_seconds=1)
        svc = SpeechToTextService(config=cfg)
        # 2 seconds worth of audio at 16kHz, 16-bit
        audio = b"\x00" * (16000 * 2 * 2)
        result = await svc.transcribe(audio)
        assert not result.success
        assert result.error == "audio_too_long"

    @pytest.mark.asyncio
    async def test_custom_language(self):
        svc = SpeechToTextService()
        audio = b"\x00" * 100
        result = await svc.transcribe(audio, language="no")
        assert result.language == "no"

    def test_backend_property(self):
        svc = SpeechToTextService()
        assert svc.backend == STTBackend.STUB

    def test_transcription_result_model(self):
        r = TranscriptionResult(text="hello", confidence=0.9)
        assert r.success


# ── TTS ────────────────────────────────────────────────────────

class TestTTS:
    @pytest.mark.asyncio
    async def test_stub_synthesize(self):
        svc = TextToSpeechService()
        result = await svc.synthesize("Hello world")
        assert result.success
        assert len(result.audio) > 0
        assert result.duration_seconds > 0

    @pytest.mark.asyncio
    async def test_empty_text(self):
        svc = TextToSpeechService()
        result = await svc.synthesize("")
        assert not result.success
        assert result.error == "empty_text"

    @pytest.mark.asyncio
    async def test_text_too_long(self):
        svc = TextToSpeechService()
        result = await svc.synthesize("x" * 6000)
        assert not result.success
        assert result.error == "text_too_long"

    @pytest.mark.asyncio
    async def test_custom_voice(self):
        svc = TextToSpeechService()
        result = await svc.synthesize("hi", voice="nova")
        assert result.voice == "nova"

    def test_backend_property(self):
        svc = TextToSpeechService()
        assert svc.backend == TTSBackend.STUB

    def test_synthesis_result_model(self):
        r = SynthesisResult(audio=b"\x00", duration_seconds=1.0, voice="test")
        assert r.success

    @pytest.mark.asyncio
    async def test_whitespace_only_text(self):
        svc = TextToSpeechService()
        result = await svc.synthesize("   ")
        assert not result.success
        assert result.error == "empty_text"
