"""Translation API endpoint."""

from __future__ import annotations

import uuid

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from reasoning_runtime.domain import CompletionRequest, Message, Provider
from reasoning_runtime.providers import translation_provider

logger = __import__("logging").getLogger(__name__)

router = APIRouter(prefix="/api/v1/translate", tags=["translate"])


class TranslateRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=10_000)
    source_language: str = ""
    target_language: str = Field(..., description="ISO-639-1 code")
    org_id: str = ""


class TranslateResponse(BaseModel):
    request_id: str
    translated_text: str
    model_used: str


@router.post("", response_model=TranslateResponse)
async def translate(body: TranslateRequest):
    """Translate text between languages."""
    request_id = str(uuid.uuid4())

    # Encode source-target in model_id
    model_id = f"{body.source_language}-{body.target_language}" if body.source_language else body.target_language

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id=model_id,
        provider=Provider.AZURE_OPENAI,  # Azure Translator
        messages=[Message(role="user", content=body.text)],
    )

    try:
        resp = await translation_provider.complete(req)
        return TranslateResponse(
            request_id=request_id,
            translated_text=resp.content,
            model_used=resp.model_used,
        )
    except Exception as exc:
        logger.exception("translate_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── detect text language ─────────────────────────────────────────────────────


class DetectTextLanguageRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=1000)
    org_id: str = ""


class DetectTextLanguageResponse(BaseModel):
    request_id: str
    language: str
    confidence: float
    is_translation_supported: bool = True


@router.post("/detect", response_model=DetectTextLanguageResponse)
async def detect_text_language(body: DetectTextLanguageRequest):
    """Detect the language of a text string using Azure Translator."""
    request_id = str(uuid.uuid4())

    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id="detect",
        provider=Provider.AZURE_OPENAI,
        messages=[Message(role="user", content=body.text)],
    )

    try:
        resp = await translation_provider.complete(req)
        meta = resp.metadata or {}
        return DetectTextLanguageResponse(
            request_id=request_id,
            language=meta.get("detected_language") or resp.model_used,
            confidence=float(meta.get("confidence", 1.0)),
            is_translation_supported=meta.get("is_translation_supported", True),
        )
    except Exception as exc:
        logger.exception("detect_language_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── transliterate ────────────────────────────────────────────────────────────


class TransliterateRequest(BaseModel):
    text: str = Field(..., min_length=1, max_length=5000)
    language: str = Field(..., description="Source language ISO-639-1 code, e.g. 'zh'")
    from_script: str = Field(..., description="Source script, e.g. 'Hans'")
    to_script: str = Field(..., description="Target script, e.g. 'Latn'")
    org_id: str = ""


class TransliterateResponse(BaseModel):
    request_id: str
    transliterated_text: str
    model_used: str


@router.post("/transliterate", response_model=TransliterateResponse)
async def transliterate(body: TransliterateRequest):
    """Transliterate text between scripts (e.g. Chinese Hanzi → Pinyin)."""
    request_id = str(uuid.uuid4())

    model_id = f"transliterate:{body.language}:{body.from_script}:{body.to_script}"
    req = CompletionRequest(
        request_id=request_id,
        org_id=body.org_id,
        model_id=model_id,
        provider=Provider.AZURE_OPENAI,
        messages=[Message(role="user", content=body.text)],
    )

    try:
        resp = await translation_provider.complete(req)
        return TransliterateResponse(
            request_id=request_id,
            transliterated_text=resp.content,
            model_used=resp.model_used,
        )
    except Exception as exc:
        logger.exception("transliterate_error request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── supported languages ──────────────────────────────────────────────────────


class LanguageInfo(BaseModel):
    code: str
    name: str
    native_name: str | None = None
    direction: str = "ltr"


# Canonical list; fetched from Azure Translator at startup in production.
_LANGUAGES: list[dict] = [
    {"code": "af",  "name": "Afrikaans",          "native_name": "Afrikaans",          "direction": "ltr"},
    {"code": "ar",  "name": "Arabic",              "native_name": "العربية",             "direction": "rtl"},
    {"code": "zh",  "name": "Chinese (Simplified)","native_name": "中文 (简体)",          "direction": "ltr"},
    {"code": "zh-TW","name": "Chinese (Traditional)","native_name": "繁體中文",           "direction": "ltr"},
    {"code": "cs",  "name": "Czech",               "native_name": "Čeština",             "direction": "ltr"},
    {"code": "da",  "name": "Danish",              "native_name": "Dansk",               "direction": "ltr"},
    {"code": "nl",  "name": "Dutch",               "native_name": "Nederlands",          "direction": "ltr"},
    {"code": "en",  "name": "English",             "native_name": "English",             "direction": "ltr"},
    {"code": "fi",  "name": "Finnish",             "native_name": "Suomi",               "direction": "ltr"},
    {"code": "fr",  "name": "French",              "native_name": "Français",            "direction": "ltr"},
    {"code": "de",  "name": "German",              "native_name": "Deutsch",             "direction": "ltr"},
    {"code": "he",  "name": "Hebrew",              "native_name": "עברית",               "direction": "rtl"},
    {"code": "hi",  "name": "Hindi",               "native_name": "हिन्दी",               "direction": "ltr"},
    {"code": "hu",  "name": "Hungarian",           "native_name": "Magyar",              "direction": "ltr"},
    {"code": "id",  "name": "Indonesian",          "native_name": "Bahasa Indonesia",    "direction": "ltr"},
    {"code": "it",  "name": "Italian",             "native_name": "Italiano",            "direction": "ltr"},
    {"code": "ja",  "name": "Japanese",            "native_name": "日本語",               "direction": "ltr"},
    {"code": "ko",  "name": "Korean",              "native_name": "한국어",               "direction": "ltr"},
    {"code": "nb",  "name": "Norwegian (Bokmål)",  "native_name": "Norsk (Bokmål)",      "direction": "ltr"},
    {"code": "pl",  "name": "Polish",              "native_name": "Polski",              "direction": "ltr"},
    {"code": "pt",  "name": "Portuguese",          "native_name": "Português",           "direction": "ltr"},
    {"code": "ro",  "name": "Romanian",            "native_name": "Română",              "direction": "ltr"},
    {"code": "ru",  "name": "Russian",             "native_name": "Русский",             "direction": "ltr"},
    {"code": "es",  "name": "Spanish",             "native_name": "Español",             "direction": "ltr"},
    {"code": "sv",  "name": "Swedish",             "native_name": "Svenska",             "direction": "ltr"},
    {"code": "th",  "name": "Thai",                "native_name": "ภาษาไทย",             "direction": "ltr"},
    {"code": "tr",  "name": "Turkish",             "native_name": "Türkçe",              "direction": "ltr"},
    {"code": "uk",  "name": "Ukrainian",           "native_name": "Українська",          "direction": "ltr"},
    {"code": "vi",  "name": "Vietnamese",          "native_name": "Tiếng Việt",          "direction": "ltr"},
]


@router.get("/languages", response_model=list[LanguageInfo])
async def list_languages():
    """Return all languages supported by the translation provider."""
    return [LanguageInfo(**lang) for lang in _LANGUAGES]
