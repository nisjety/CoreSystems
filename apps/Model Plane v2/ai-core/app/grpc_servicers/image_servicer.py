"""gRPC ImageService servicer.

Implements:
  - GenerateImage  (Unary)
  - AnalyzeImage   (Unary — vision)
  - ExtractText    (Unary — OCR via Content Understanding or Document Intelligence)
"""

from __future__ import annotations

import base64
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

    class ImageServicer(ai_core_pb2_grpc.ImageServiceServicer):  # type: ignore[misc]
        """gRPC wrapper around image generation and vision APIs."""

        async def GenerateImage(self, request: Any, context: Any) -> Any:
            """Generate an image from a text prompt via the images API router."""
            from app.config import get_settings
            from app.services.image_service import generate_image

            settings = get_settings()
            request_id = request.request_id or str(uuid.uuid4())

            try:
                result = await generate_image(
                    prompt=request.prompt,
                    model=request.model or "dall-e-3",
                    size=request.size or "1024x1024",
                    quality=request.quality or "standard",
                    n=request.n or 1,
                    settings=settings,
                )

                images = []
                for img in result.get("images", []):
                    images.append(
                        ai_core_pb2.GeneratedImage(
                            url=img.get("url", ""),
                            revised_prompt=img.get("revised_prompt", ""),
                            b64_json=img.get("b64_json", ""),
                        )
                    )

                return ai_core_pb2.GenerateImageResponse(
                    request_id=request_id,
                    images=images,
                )
            except Exception as exc:
                logger.exception("grpc_generate_image_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def AnalyzeImage(self, request: Any, context: Any) -> Any:
            """Analyze an image using vision models (GPT-4o, etc)."""
            from reasoning_runtime.domain import CompletionRequest, Message, Provider
            from reasoning_runtime.providers import chat_provider  # type: ignore[import]

            request_id = request.request_id or str(uuid.uuid4())

            image_content: list[dict[str, Any]] = []
            if request.image_url:
                image_content.append(
                    {"type": "image_url", "image_url": {"url": request.image_url}}
                )
            elif request.image_data:
                b64 = base64.b64encode(request.image_data).decode()
                mime = request.mime_type or "image/png"
                image_content.append(
                    {
                        "type": "image_url",
                        "image_url": {"url": f"data:{mime};base64,{b64}"},
                    }
                )
            if request.prompt:
                image_content.insert(0, {"type": "text", "text": request.prompt})

            messages = [Message(role="user", content=image_content)]
            req = CompletionRequest(
                request_id=request_id,
                org_id=request.org_id,
                model_id=request.model or "gpt-4o",
                provider=Provider.AZURE_OPENAI,
                messages=messages,
                max_tokens=request.max_tokens or 1024,
            )

            try:
                resp = await chat_provider.complete(req)
                return ai_core_pb2.AnalyzeImageResponse(
                    request_id=request_id,
                    description=resp.content,
                    model_used=resp.model_used,
                )
            except Exception as exc:
                logger.exception("grpc_analyze_image_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def ExtractText(self, request: Any, context: Any) -> Any:
            """OCR / text extraction from an image.

            Delegates to Azure Document Intelligence read model for
            high-fidelity text extraction.
            """
            from app.config import get_settings

            settings = get_settings()
            request_id = request.request_id or str(uuid.uuid4())

            try:
                from azure.ai.documentintelligence.aio import (
                    DocumentIntelligenceClient,
                )
                from azure.core.credentials import AzureKeyCredential

                endpoint = settings.azure_document_intelligence_endpoint
                key = settings.azure_document_intelligence_key
                if not endpoint or not key:
                    await context.abort(
                        grpc.StatusCode.FAILED_PRECONDITION,
                        "Azure Document Intelligence not configured",
                    )
                    return

                client = DocumentIntelligenceClient(
                    endpoint=endpoint,
                    credential=AzureKeyCredential(key),
                )

                analyze_request: dict[str, Any] = {}
                if request.image_url:
                    analyze_request["url_source"] = request.image_url
                elif request.image_data:
                    analyze_request["bytes_source"] = request.image_data
                else:
                    await context.abort(
                        grpc.StatusCode.INVALID_ARGUMENT,
                        "image_url or image_data required",
                    )
                    return

                async with client:
                    poller = await client.begin_analyze_document(
                        model_id="prebuilt-read",
                        analyze_request=analyze_request,
                    )
                    result = await poller.result()

                extracted_text = result.content if result.content else ""
                return ai_core_pb2.ExtractTextResponse(
                    request_id=request_id,
                    text=extracted_text,
                    page_count=len(result.pages) if result.pages else 0,
                )
            except Exception as exc:
                logger.exception("grpc_extract_text_error request_id=%s", request_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

else:  # grpc not available — provide importable stub
    class ImageServicer:  # type: ignore[no-redef]
        pass
