from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any, Mapping

import grpc
from google.protobuf import json_format
from google.protobuf.struct_pb2 import Struct
from google.protobuf.timestamp_pb2 import Timestamp

from app.config import settings
from app.db.postgres import SessionLocal, delete_document, get_document, list_documents
from app.grpc_runtime import load_documents_proto_modules

logger = logging.getLogger(__name__)


def _normalize_metadata(raw_metadata: Any) -> dict[str, Any]:
    if isinstance(raw_metadata, dict):
        return raw_metadata
    if isinstance(raw_metadata, str):
        try:
            loaded = json.loads(raw_metadata)
        except json.JSONDecodeError:
            return {}
        return loaded if isinstance(loaded, dict) else {}
    return {}


def _struct_from_mapping(raw_metadata: Any) -> Struct:
    message = Struct()
    json_format.ParseDict(_normalize_metadata(raw_metadata), message)
    return message


def _timestamp_from_datetime(value: datetime) -> Timestamp:
    message = Timestamp()
    if value.tzinfo is None:
        value = value.replace(tzinfo=timezone.utc)
    else:
        value = value.astimezone(timezone.utc)
    message.FromDatetime(value)
    return message


def _build_document_message(documents_pb2: Any, document: Mapping[str, Any]) -> Any:
    return documents_pb2.Document(
        document_id=document["document_id"],
        org_id=document["org_id"],
        source=document.get("source", ""),
        type=document.get("type", ""),
        title=document.get("title", ""),
        content=document.get("content", "") or "",
        status=document.get("status", ""),
        metadata=_struct_from_mapping(document.get("metadata")),
        created_at=_timestamp_from_datetime(document["created_at"]),
        updated_at=_timestamp_from_datetime(document["updated_at"]),
    )


async def create_grpc_server() -> grpc.aio.Server:
    documents_pb2, documents_pb2_grpc = load_documents_proto_modules()

    class DocumentServiceServicer(documents_pb2_grpc.DocumentServiceServicer):
        async def GetDocument(self, request: Any, context: grpc.aio.ServicerContext) -> Any:
            async with SessionLocal() as db:
                document = await get_document(
                    db,
                    document_id=request.document_id,
                    org_id=request.org_id,
                )

            if not document:
                await context.abort(grpc.StatusCode.NOT_FOUND, "Document not found")

            return documents_pb2.GetDocumentResponse(
                document=_build_document_message(documents_pb2, document)
            )

        async def ListDocuments(self, request: Any, context: grpc.aio.ServicerContext) -> Any:
            page_size = request.page_size or 50
            if page_size <= 0 or page_size > 500:
                await context.abort(
                    grpc.StatusCode.INVALID_ARGUMENT,
                    "page_size must be between 1 and 500",
                )

            try:
                offset = int(request.page_token or "0")
            except ValueError:
                await context.abort(grpc.StatusCode.INVALID_ARGUMENT, "page_token must be numeric")

            if offset < 0:
                await context.abort(
                    grpc.StatusCode.INVALID_ARGUMENT,
                    "page_token must be non-negative",
                )

            async with SessionLocal() as db:
                documents = await list_documents(
                    db,
                    org_id=request.org_id,
                    type=request.type or None,
                    status=request.status or None,
                    limit=page_size,
                    offset=offset,
                )

            next_page_token = ""
            if len(documents) == page_size:
                next_page_token = str(offset + len(documents))

            return documents_pb2.ListDocumentsResponse(
                documents=[
                    _build_document_message(documents_pb2, document)
                    for document in documents
                ],
                next_page_token=next_page_token,
            )

        async def DeleteDocument(self, request: Any, context: grpc.aio.ServicerContext) -> Any:
            async with SessionLocal() as db:
                deleted = await delete_document(
                    db,
                    document_id=request.document_id,
                    org_id=request.org_id,
                )

            return documents_pb2.DeleteDocumentResponse(success=deleted)

    server = grpc.aio.server(interceptors=[_service_auth_interceptor()])
    documents_pb2_grpc.add_DocumentServiceServicer_to_server(
        DocumentServiceServicer(),
        server,
    )
    bind_address = f"[::]:{settings.grpc_port}"
    server.add_insecure_port(bind_address)
    logger.info("documents-service configured gRPC listener on %s", bind_address)
    return server


def _service_auth_interceptor():
    """
    gRPC server interceptor that validates X-Service-Auth metadata.
    Workers (knowledge-index, embedding-worker) must send INTERNAL_API_KEY.
    """

    class ServiceAuthInterceptor(grpc.aio.ServerInterceptor):
        async def intercept_service(self, continuation, handler_call_details):
            metadata = dict(handler_call_details.invocation_metadata or [])
            expected = settings.internal_api_key

            if not expected:
                # No key configured — dev mode, allow all
                return await continuation(handler_call_details)

            service_key = metadata.get("x-service-auth", "")
            if service_key != expected:
                logger.warning(
                    "gRPC request rejected — invalid x-service-auth for %s",
                    handler_call_details.method,
                )

                async def _deny(request, context):
                    await context.abort(
                        grpc.StatusCode.UNAUTHENTICATED,
                        "Invalid or missing x-service-auth metadata",
                    )

                return grpc.unary_unary_rpc_method_handler(_deny)

            return await continuation(handler_call_details)

    return ServiceAuthInterceptor()