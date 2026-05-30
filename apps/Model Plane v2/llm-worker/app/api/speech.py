"""Speech API — TTS and STT endpoints. Phase 4.1."""
from __future__ import annotations

from fastapi import APIRouter, HTTPException, UploadFile, File
from pydantic import BaseModel, Field

from app.providers import speech_provider

router = APIRouter(prefix="/v1/speech", tags=["speech"])


class TTSRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=5000)
    voice: str = "en-US-JennyNeural"
    output_format: str = "audio-24khz-96kbitrate-mono-mp3"
    provider: str = "azure"


class STTResponse(BaseModel):
    text: str
    language: str
    duration_seconds: float | None = None


@router.post("/tts")
async def tts_endpoint(body: TTSRequest):
    """Convert text to speech audio."""
    from fastapi.responses import Response

    try:
        audio = await speech_provider.text_to_speech(
            body.text,
            voice=body.voice,
            output_format=body.output_format,
            provider=body.provider,
        )
        return Response(content=audio, media_type="audio/mpeg")
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc))


@router.post("/stt", response_model=STTResponse)
async def stt_endpoint(
    file: UploadFile = File(...),
    language: str = "en",
    provider: str = "azure",
):
    """Transcribe audio to text."""
    audio = await file.read()
    if not audio:
        raise HTTPException(status_code=400, detail="Empty audio file")

    try:
        result = await speech_provider.speech_to_text(
            audio, language=language, provider=provider,
        )
        return STTResponse(**result)
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc))
