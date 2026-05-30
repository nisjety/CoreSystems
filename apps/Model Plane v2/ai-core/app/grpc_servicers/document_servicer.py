"""gRPC DocumentService servicer."""

from __future__ import annotations

import json
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
    class DocumentServicer(ai_core_pb2_grpc.DocumentServiceServicer):  # type: ignore[misc]

        async def _analyze(self, model_id: str, request, context):
            from app.api.documents import DocumentRequest, _analyze

            doc_req = DocumentRequest(
                url=request.url or None,
                content_base64=__import__("base64").b64encode(request.content_bytes).decode()
                if request.content_bytes else None,
                model=model_id,
                org_id=request.org_id,
            )

            try:
                raw = await _analyze(model_id, doc_req)
                fields = {}
                if docs := raw.get("documents"):
                    for doc in docs:
                        for k, v in (doc.get("fields") or {}).items():
                            fields[k] = v.get("content") or v.get("value") or ""
                paragraphs = [p.get("content", "") for p in (raw.get("paragraphs") or [])]
                tables = raw.get("tables") or []

                return ai_core_pb2.DocumentAnalysisResponse(
                    request_id=request.request_id or str(uuid.uuid4()),
                    model=model_id,
                    status="succeeded",
                    fields_json=json.dumps(fields),
                    tables_json=json.dumps(tables),
                    paragraphs=paragraphs,
                )
            except Exception as exc:
                logger.exception("grpc_document_analyze_error model=%s", model_id)
                await context.abort(grpc.StatusCode.INTERNAL, str(exc))

        async def AnalyzeDocument(self, request, context):
            return await self._analyze(request.model or "prebuilt-document", request, context)

        async def ExtractLayout(self, request, context):
            return await self._analyze("prebuilt-layout", request, context)

        async def ExtractForm(self, request, context):
            return await self._analyze("prebuilt-document", request, context)

        async def ParseReceipt(self, request, context):
            return await self._analyze("prebuilt-receipt", request, context)

        async def ParseInvoice(self, request, context):
            return await self._analyze("prebuilt-invoice", request, context)

else:
    class DocumentServicer:  # type: ignore[no-redef]
        """Stub used when grpc_gen stubs are not yet compiled."""
