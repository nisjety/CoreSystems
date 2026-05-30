"""Translation API — text translation endpoint. Phase 4.4."""
from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.providers import translation_provider

router = APIRouter(prefix="/v1/translate", tags=["translate"])


class TranslateRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=10_000)
    source_language: str = Field("", description="ISO-639-1 code or empty for auto-detect")
    target_language: str = Field(..., description="ISO-639-1 code, e.g. 'no', 'en', 'de'")


class TranslateResponse(BaseModel):
    translated_text: str
    detected_language: str | None = None


@router.post("", response_model=TranslateResponse)
async def translate_endpoint(body: TranslateRequest):
    """Translate text between languages."""
    try:
        result = await translation_provider.translate(
            text=body.text,
            source_language=body.source_language or None,
            target_language=body.target_language,
        )
        return TranslateResponse(
            translated_text=result["translated_text"],
            detected_language=result.get("detected_language"),
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc))
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail=str(exc))
