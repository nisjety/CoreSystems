"""gRPC TranslationService servicer.

Implements:
  - Translate        (Unary)
  - BatchTranslate   (Unary)
  - DetectLanguage   (Unary)
  - ListLanguages    (Unary)

Delegates to Azure Translator via app.services.translate_service.
"""

from __future__ import annotations

import logging
import uuid
from typing import Any

logger = logging.getLogger(__name__)

try:
    import grpc
    from app.grpc_gen import ai_core_pb2, ai_core_pb2_grpc  # type: ignore[import]

    _GRPC_AVAILABLE = True
except ImportError:
    _GRPC_AVAILABLE = False


if _GRPC_AVAILABLE:  # noqa: C901

    class TranslationServicer(ai_core_pb2_grpc.TranslationServiceServicer):  # type: ignore[misc]
        """gRPC wrapper around Azure Translator."""

        async def Translate(self, request: Any, context: Any) -> Any:
            from app.config import get_settings
            from app.services.translate_service import translate_text

            settings = get_settings()
            request_id = request.request_id or str(uuid.uuid4())

            try:
                result = await translate_text(
                    text=request.text,
                    target_language=request.target_language,
                    source_language=request.source_language or None,
                    settings=settings,
                )
                return ai_core_pb2.TranslateResponse(
                    request_id=request_id,
                    translated_text=result.get("translated_text", ""),
                    model_used="azure-translator",
                    detected_language=result.get("detected_language", ""),
                    confidence=float(result.get("confidence", 0.0)),
                )
            except Exception as exc:
                logger.exception("grpc_translate_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def BatchTranslate(self, request: Any, context: Any) -> Any:
            from app.config import get_settings
            from app.services.translate_service import translate_text

            settings = get_settings()
            request_id = request.request_id or str(uuid.uuid4())

            translations = []
            try:
                for item in request.items:
                    result = await translate_text(
                        text=item.text,
                        target_language=request.target_language,
                        source_language=request.source_language or None,
                        settings=settings,
                    )
                    translations.append(
                        ai_core_pb2.TranslationItem(
                            original_text=item.text,
                            translated_text=result.get("translated_text", ""),
                            detected_language=result.get("detected_language", ""),
                        )
                    )
                return ai_core_pb2.BatchTranslateResponse(
                    request_id=request_id,
                    translations=translations,
                )
            except Exception as exc:
                logger.exception("grpc_batch_translate_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def DetectLanguage(self, request: Any, context: Any) -> Any:
            from app.config import get_settings
            from app.services.translate_service import detect_language

            settings = get_settings()
            request_id = request.request_id or str(uuid.uuid4())

            try:
                result = await detect_language(text=request.text, settings=settings)
                detections = []
                for det in result.get("detections", []):
                    detections.append(
                        ai_core_pb2.LanguageDetection(
                            language=det.get("language", ""),
                            confidence=det.get("confidence", 0.0),
                        )
                    )
                return ai_core_pb2.DetectTextLanguageResponse(
                    request_id=request_id,
                    detections=detections,
                )
            except Exception as exc:
                logger.exception("grpc_detect_language_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def ListLanguages(self, request: Any, context: Any) -> Any:
            from app.config import get_settings
            from app.services.translate_service import list_languages

            settings = get_settings()

            try:
                result = await list_languages(settings=settings)
                languages = []
                for lang in result.get("languages", []):
                    languages.append(
                        ai_core_pb2.SupportedLanguage(
                            code=lang.get("code", ""),
                            name=lang.get("name", ""),
                            native_name=lang.get("native_name", ""),
                        )
                    )
                return ai_core_pb2.ListLanguagesResponse(languages=languages)
            except Exception as exc:
                logger.exception("grpc_list_languages_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

else:  # grpc not available
    class TranslationServicer:  # type: ignore[no-redef]
        pass
