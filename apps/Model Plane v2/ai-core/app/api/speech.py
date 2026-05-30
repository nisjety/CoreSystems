"""Speech API — TTS, STT, and transcription endpoints."""

from __future__ import annotations

import base64
import uuid

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from reasoning_runtime.domain import CompletionRequest, Message, Provider
from reasoning_runtime.providers import speech_provider

logger = __import__("logging").getLogger(__name__)

router = APIRouter(prefix="/api/v1/speech", tags=["speech"])


class TTSRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=5000)
    voice: str = "en-US-JennyNeural"
    provider: str = "azure"
    org_id: str = ""


class STTRequest(BaseModel):
    audio_base64: str = Field(..., min_length=1)
    language: str = "en"
    provider: str = "azure"
    org_id: str = ""


class SpeechResponse(BaseModel):
    request_id: str
    content: str
    model_used: str
    metadata: dict | None = None


@router.post("/tts", response_model=SpeechResponse)
async def tts(body: TTSRequest):
    """Convert text to speech audio (base64)."""
    request_id = str(uuid.uuid4())

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id=body.voice,
        provider=Provider.OPENAI if body.provider == "openai" else Provider.AZURE_OPENAI,
        messages=[Message(role="user", content=body.text)],
    )

    try:
        resp = await speech_provider.complete(req)
        return SpeechResponse(
            request_id=request_id,
            content=resp.content,
            model_used=resp.model_used,
            metadata=resp.metadata,
        )
    except Exception as exc:
        logger.exception("tts_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


@router.post("/stt", response_model=SpeechResponse)
async def stt(body: STTRequest):
    """Transcribe audio (base64) to text."""
    request_id = str(uuid.uuid4())

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id="stt",
        provider=Provider.AZURE_OPENAI,
        messages=[Message(role="user", content=body.audio_base64)],
    )

    try:
        resp = await speech_provider.complete(req)
        return SpeechResponse(
            request_id=request_id,
            content=resp.content,
            model_used=resp.model_used,
            metadata=resp.metadata,
        )
    except Exception as exc:
        logger.exception("stt_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


class TranscribeRequest(BaseModel):
    """Multi-provider audio transcription request."""
    audio_base64: str = Field(..., min_length=1, description="Base64-encoded audio bytes")
    mimetype: str = Field("audio/wav", description="MIME type of the audio, e.g. audio/wav, audio/mp3")
    provider: str = Field("deepgram", description="Transcription provider: deepgram")
    language: str = Field("en-US", description="BCP-47 language tag")
    org_id: str = ""


class TranscribeResponse(BaseModel):
    request_id: str
    text: str
    confidence: float
    language: str
    provider: str
    error: bool = False


@router.post("/transcribe", response_model=TranscribeResponse)
async def transcribe(body: TranscribeRequest):
    """Transcribe audio using Deepgram (or other supported providers)."""
    request_id = str(uuid.uuid4())
    try:
        audio_bytes = base64.b64decode(body.audio_base64)
    except Exception:
        raise HTTPException(status_code=400, detail="Invalid base64 audio data")

    from app.services.asr_service import get_asr_service
    result = await get_asr_service().transcribe(
        audio_bytes=audio_bytes,
        provider=body.provider,
        language=body.language,
        mimetype=body.mimetype,
        org_id=body.org_id,
    )
    return TranscribeResponse(
        request_id=request_id,
        text=result.text,
        confidence=result.confidence,
        language=result.language,
        provider=result.provider,
        error=result.error,
    )


# ── voices ──────────────────────────────────────────────────────────────────


class VoiceInfo(BaseModel):
    id: str
    name: str
    language: str
    gender: str
    provider: str


_AZURE_VOICES: list[dict] = [
    {"id": "en-US-JennyNeural",    "name": "Jenny",    "language": "en-US", "gender": "Female", "provider": "azure"},
    {"id": "en-US-GuyNeural",      "name": "Guy",      "language": "en-US", "gender": "Male",   "provider": "azure"},
    {"id": "en-GB-SoniaNeural",    "name": "Sonia",    "language": "en-GB", "gender": "Female", "provider": "azure"},
    {"id": "en-AU-NatashaNeural",  "name": "Natasha",  "language": "en-AU", "gender": "Female", "provider": "azure"},
    {"id": "nb-NO-PernilleNeural", "name": "Pernille", "language": "nb-NO", "gender": "Female", "provider": "azure"},
    {"id": "nb-NO-FinnNeural",     "name": "Finn",     "language": "nb-NO", "gender": "Male",   "provider": "azure"},
    {"id": "de-DE-KatjaNeural",    "name": "Katja",    "language": "de-DE", "gender": "Female", "provider": "azure"},
    {"id": "fr-FR-DeniseNeural",   "name": "Denise",   "language": "fr-FR", "gender": "Female", "provider": "azure"},
    {"id": "es-ES-ElviraNeural",   "name": "Elvira",   "language": "es-ES", "gender": "Female", "provider": "azure"},
]

_OPENAI_VOICES: list[dict] = [
    {"id": "alloy",   "name": "Alloy",   "language": "*", "gender": "Neutral", "provider": "openai"},
    {"id": "echo",    "name": "Echo",    "language": "*", "gender": "Male",    "provider": "openai"},
    {"id": "nova",    "name": "Nova",    "language": "*", "gender": "Female",  "provider": "openai"},
    {"id": "shimmer", "name": "Shimmer", "language": "*", "gender": "Female",  "provider": "openai"},
]


@router.get("/voices", response_model=list[VoiceInfo])
async def list_voices(
    provider: str | None = None,
    language: str | None = None,
):
    """List available TTS voices, optionally filtered by provider or language prefix."""
    voices = _AZURE_VOICES + _OPENAI_VOICES
    if provider:
        voices = [v for v in voices if v["provider"] == provider]
    if language:
        voices = [v for v in voices if v["language"].startswith(language) or v["language"] == "*"]
    return [VoiceInfo(**v) for v in voices]


# ── detect spoken language ───────────────────────────────────────────────────


class DetectLanguageRequest(BaseModel):
    audio_base64: str = Field(..., min_length=1, description="Base64-encoded audio bytes")
    mimetype: str = "audio/wav"
    org_id: str = ""


class DetectLanguageResponse(BaseModel):
    request_id: str
    language: str
    confidence: float


@router.post("/detect-language", response_model=DetectLanguageResponse)
async def detect_language(body: DetectLanguageRequest):
    """Detect the spoken language in an audio clip."""
    request_id = str(uuid.uuid4())
    try:
        audio_bytes = base64.b64decode(body.audio_base64)
    except Exception:
        raise HTTPException(status_code=400, detail="Invalid base64 audio data")

    from app.services.asr_service import get_asr_service
    result = await get_asr_service().transcribe(
        audio_bytes=audio_bytes,
        provider="deepgram",
        language="auto",
        mimetype=body.mimetype,
        org_id=body.org_id,
    )
    return DetectLanguageResponse(
        request_id=request_id,
        language=result.language,
        confidence=result.confidence,
    )
