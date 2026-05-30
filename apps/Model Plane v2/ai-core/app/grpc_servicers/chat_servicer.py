"""gRPC ChatService servicer.

Generated stubs are expected in app.grpc_gen (created at build time with
  python -m grpc_tools.protoc -I proto --python_out=app/grpc_gen
      --pyi_out=app/grpc_gen --grpc_python_out=app/grpc_gen proto/ai_core.proto
)
Falls back gracefully when grpc_gen is absent so unit tests still work.
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


if _GRPC_AVAILABLE:
    class ChatServicer(ai_core_pb2_grpc.ChatServiceServicer):  # type: ignore[misc]
        """gRPC wrapper around reasoning_runtime chat provider."""

        async def ChatCompletion(self, request: Any, context: Any) -> Any:
            from reasoning_runtime.domain import CompletionRequest, Message, Provider
            from reasoning_runtime.providers import chat_provider  # type: ignore[import]

            messages = [
                Message(role=m.role, content=m.content, name=m.name or None)
                for m in request.messages
            ]
            req = CompletionRequest(
                request_id=request.request_id or str(uuid.uuid4()),
                org_id=request.org_id,
                model_id=request.model or "claude-haiku-4-5-20251001",
                provider=Provider(request.provider) if request.provider else Provider.ANTHROPIC,
                messages=messages,
                temperature=request.temperature or 0.7,
                max_tokens=request.max_tokens or None,
            )

            try:
                resp = await chat_provider.complete(req)
                return ai_core_pb2.ChatResponse(
                    request_id=req.request_id,
                    content=resp.content,
                    model_used=resp.model_used,
                    stop_reason=resp.finish_reason,
                    input_tokens=resp.tokens_in,
                    output_tokens=resp.tokens_out,
                )
            except Exception as exc:
                logger.exception("grpc_chat_completion_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def ChatCompletionStream(self, request: Any, context: Any):
            from reasoning_runtime.domain import CompletionRequest, Message, Provider
            from reasoning_runtime.providers import chat_provider  # type: ignore[import]

            messages = [Message(role=m.role, content=m.content) for m in request.messages]
            req = CompletionRequest(
                request_id=request.request_id or str(uuid.uuid4()),
                org_id=request.org_id,
                model_id=request.model or "claude-haiku-4-5-20251001",
                provider=Provider(request.provider) if request.provider else Provider.ANTHROPIC,
                messages=messages,
                stream=True,
            )

            try:
                async for chunk in chat_provider.stream(req):
                    yield ai_core_pb2.ChatChunk(
                        request_id=req.request_id,
                        delta=chunk.delta,
                        done=chunk.done,
                        model_used=chunk.model_used or "",
                    )
            except Exception as exc:
                logger.exception("grpc_chat_stream_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def CreateEmbedding(self, request: Any, context: Any) -> Any:
            from reasoning_runtime.domain import CompletionRequest, Message, Provider
            from reasoning_runtime.providers import embedding_provider  # type: ignore[import]

            req = CompletionRequest(
                request_id=request.request_id or str(uuid.uuid4()),
                org_id=request.org_id,
                model_id=request.model or "text-embedding-3-large",
                provider=Provider(request.provider) if request.provider else Provider.OPENAI,
                messages=[Message(role="user", content=request.text)],
            )

            try:
                resp = await embedding_provider.complete(req)
                vector = resp.metadata.get("vector", []) if resp.metadata else []
                return ai_core_pb2.EmbeddingResponse(
                    request_id=req.request_id,
                    vector=vector,
                    model_used=resp.model_used,
                )
            except Exception as exc:
                logger.exception("grpc_embedding_error")
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def ListModels(self, request: Any, context: Any) -> Any:
            from app.api.models import _MODEL_CATALOG
            entries = _MODEL_CATALOG
            if request.modality:
                entries = [e for e in entries if e["modality"] == request.modality]
            if request.provider:
                entries = [e for e in entries if e["provider"] == request.provider]

            models = [
                ai_core_pb2.ModelInfo(
                    id=e["id"],
                    provider=e["provider"],
                    modality=e["modality"],
                    streaming=e["streaming"],
                )
                for e in entries
            ]
            return ai_core_pb2.ListModelsResponse(models=models)

else:
    class ChatServicer:  # type: ignore[no-redef]
        """Stub used when grpc_gen stubs are not yet compiled."""
