"""
Translation service — wrapper around Azure Translator (azure-ai-translation-text SDK).

Provides text translation, language detection, and batch operations.
Delegates to Azure Cognitive Services Translator API v3.
"""

from __future__ import annotations

import logging
from typing import Any

from app.config import get_settings

logger = logging.getLogger(__name__)


def _make_client(settings: Any):
    """Build a TextTranslationClient from app settings. Returns None if SDK absent."""
    try:
        from azure.ai.translation.text import TextTranslationClient
        from azure.core.credentials import AzureKeyCredential
    except ImportError:
        logger.warning("azure-ai-translation-text not installed")
        return None

    api_key = getattr(settings, "AZURE_TRANSLATOR_API_KEY", "") or ""
    region = getattr(settings, "AZURE_TRANSLATOR_REGION", "eastus") or "eastus"
    endpoint = getattr(settings, "AZURE_TRANSLATOR_ENDPOINT", None)

    if not api_key:
        logger.warning("AZURE_TRANSLATOR_API_KEY not configured")
        return None

    credential = AzureKeyCredential(api_key)
    kwargs: dict[str, Any] = {"credential": credential, "region": region}
    if endpoint:
        kwargs["endpoint"] = endpoint
    return TextTranslationClient(**kwargs)


async def translate_text(
    text: str,
    target_language: str,
    source_language: str | None = None,
    settings: Any = None,
) -> dict[str, Any]:
    """
    Translate text to target language.

    Returns:
        {"translated_text": str, "detected_language": str, "confidence": float}
    """
    if settings is None:
        settings = get_settings()

    client = _make_client(settings)
    if client is None:
        return {
            "translated_text": text,
            "detected_language": source_language or "unknown",
            "confidence": 0.0,
        }

    try:
        from azure.ai.translation.text.models import InputTextItem

        response = client.translate(
            body=[InputTextItem(text=text)],
            to_language=[target_language],
            from_language=source_language or None,
        )
        translation = response[0] if response else None

        if translation and translation.translations:
            detected = translation.detected_language
            detected_lang = detected.language if detected else (source_language or "unknown")
            confidence = detected.score if detected else 0.95
            return {
                "translated_text": translation.translations[0].text,
                "detected_language": detected_lang,
                "confidence": confidence,
            }

        return {
            "translated_text": text,
            "detected_language": source_language or "unknown",
            "confidence": 0.0,
        }
    except Exception:
        logger.exception("translate_text_error text_length=%d target=%s", len(text), target_language)
        return {
            "translated_text": text,
            "detected_language": source_language or "unknown",
            "confidence": 0.0,
        }


async def detect_language(text: str, settings: Any = None) -> dict[str, Any]:
    """
    Detect the language of the given text.

    Returns:
        {"detections": [{"language": str, "confidence": float}]}
    """
    if settings is None:
        settings = get_settings()

    client = _make_client(settings)
    if client is None:
        return {"detections": [{"language": "unknown", "confidence": 0.0}]}

    try:
        from azure.ai.translation.text.models import DetectTextInput

        response = client.detect_language(body=[DetectTextInput(text=text)])
        result = response[0] if response else None
        if result:
            return {
                "detections": [
                    {"language": result.language, "confidence": result.score or 0.0}
                ]
            }
        return {"detections": [{"language": "unknown", "confidence": 0.0}]}
    except Exception:
        logger.exception("detect_language_error")
        return {"detections": [{"language": "unknown", "confidence": 0.0}]}


async def list_languages(settings: Any = None) -> dict[str, Any]:
    """
    List supported languages for translation.

    Returns:
        {"languages": [{"code": str, "name": str, "native_name": str}]}
    """
    if settings is None:
        settings = get_settings()

    client = _make_client(settings)
    if client is None:
        return {
            "languages": [
                {"code": "en", "name": "English", "native_name": "English"},
                {"code": "es", "name": "Spanish", "native_name": "Español"},
                {"code": "fr", "name": "French", "native_name": "Français"},
            ]
        }

    try:
        response = client.get_supported_languages()
        languages = []
        if response.translation:
            for code, lang_data in response.translation.items():
                languages.append({
                    "code": code,
                    "name": lang_data.name or "",
                    "native_name": lang_data.native_name or "",
                })
        return {"languages": languages}
    except Exception:
        logger.exception("list_languages_error")
        return {
            "languages": [
                {"code": "en", "name": "English", "native_name": "English"},
                {"code": "es", "name": "Spanish", "native_name": "Español"},
            ]
        }
