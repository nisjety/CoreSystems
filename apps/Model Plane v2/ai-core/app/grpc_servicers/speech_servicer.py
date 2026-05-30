"""gRPC SpeechService servicer."""

from __future__ import annotations

import base64
import logging
import uuid

logger = logging.getLogger(__name__)

try:
    import grpc
    from app.grpc_gen import ai_core_pb2, ai_core_pb2_grpc  # type: ignore[import]
    _GRPC_AVAILABLE = True
except ImportError:
    _GRPC_AVAILABLE = False


if _GRPC_AVAILABLE:
    class SpeechServicer(ai_core_pb2_grpc.SpeechServiceServicer):  # type: ignore[misc]

        async def TextToSpeech(self, request, context):
            from reasoning_runtime.domain import CompletionRequest, Message, Provider
            from reasoning_runtime.providers import speech_provider  # type: ignore[import]

            req = CompletionRequest(
                request_id=request.request_id or str(uuid.uuid4()),
                org_id=request.org_id,
                model_id=request.voice or "en-US-JennyNeural",
                provider=Provider.OPENAI if request.provider == "openai" else Provider.AZURE_OPENAI,
                messages=[Message(role="user", content=request.text)],
            )
            try:
                resp = await speech_provider.complete(req)
                audio_bytes = base64.b64decode(resp.content) if resp.content else b""
                return ai_core_pb2.TTSResponse(
                    request_id=req.request_id,
                    audio_bytes=audio_bytes,
                    model_used=resp.model_used,
                )
            except Exception as exc:
                logger.exception("grpc_tts_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def Transcribe(self, request, context):
            from app.services.asr_service import get_asr_service

            audio_b64 = base64.b64encode(request.audio_bytes).decode()
            try:
                result = await get_asr_service().transcribe(
                    audio_bytes=request.audio_bytes,
                    provider=request.provider or "deepgram",
                    language=request.language or "en-US",
                    mimetype=request.mimetype or "audio/wav",
                    org_id=request.org_id,
                )
                return ai_core_pb2.TranscribeResponse(
                    request_id=request.request_id,
                    text=result.text,
                    confidence=result.confidence,
                    language=result.language,
                    model_used=result.provider,
                )
            except Exception as exc:
                logger.exception("grpc_transcribe_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def TranscribeStream(self, request, context):
            """Streaming transcription — delegates to same ASR service; emits one final chunk."""
            result = await self.Transcribe(request, context)
            if result:
                yield ai_core_pb2.TranscribeChunk(
                    request_id=request.request_id,
                    partial=result.text,
                    is_final=True,
                )

        async def DetectLanguage(self, request, context):
            from app.services.asr_service import get_asr_service

            try:
                result = await get_asr_service().transcribe(
                    audio_bytes=request.audio_bytes,
                    provider="deepgram",
                    language="auto",
                    mimetype=request.mimetype or "audio/wav",
                    org_id=request.org_id,
                )
                return ai_core_pb2.DetectLanguageResponse(
                    request_id=request.request_id,
                    language=result.language,
                    confidence=result.confidence,
                )
            except Exception as exc:
                logger.exception("grpc_detect_language_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def ListVoices(self, request, context):
            from app.api.speech import _AZURE_VOICES, _OPENAI_VOICES

            voices = _AZURE_VOICES + _OPENAI_VOICES
            if request.provider:
                voices = [v for v in voices if v["provider"] == request.provider]
            if request.language:
                voices = [v for v in voices if v["language"].startswith(request.language)]

            return ai_core_pb2.ListVoicesResponse(
                voices=[
                    ai_core_pb2.VoiceInfo(
                        id=v["id"], name=v["name"], language=v["language"],
                        gender=v["gender"], provider=v["provider"],
                    )
                    for v in voices
                ]
            )

else:
    class SpeechServicer:  # type: ignore[no-redef]
        """Stub used when grpc_gen stubs are not yet compiled."""
