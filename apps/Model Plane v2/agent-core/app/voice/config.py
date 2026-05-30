"""Voice configuration."""

from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, Field


class STTBackend(str, Enum):
    WHISPER = "whisper"
    AZURE_SPEECH = "azure_speech"
    ANTHROPIC = "anthropic"
    STUB = "stub"


class TTSBackend(str, Enum):
    AZURE_SPEECH = "azure_speech"
    ELEVENLABS = "elevenlabs"
    STUB = "stub"


class AudioFormat(str, Enum):
    PCM_16KHZ_MONO = "pcm_16khz_mono"
    WAV = "wav"
    MP3 = "mp3"
    OGG = "ogg"


class VoiceConfig(BaseModel):
    """Voice I/O configuration."""

    stt_backend: STTBackend = STTBackend.STUB
    tts_backend: TTSBackend = TTSBackend.STUB
    audio_format: AudioFormat = AudioFormat.PCM_16KHZ_MONO
    sample_rate: int = 16000
    silence_threshold_ms: int = 500
    max_audio_duration_seconds: int = 120
    language: str = "en"
    tts_voice: str = "default"
