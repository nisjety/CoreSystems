from __future__ import annotations

import asyncio
from typing import Any

import grpc
from google.protobuf import json_format

from worker.config import settings
from worker.grpc_runtime import load_documents_proto_modules


_channel: grpc.aio.Channel | None = None
_stub: Any | None = None
_stub_lock = asyncio.Lock()


async def get_documents_stub() -> Any:
    global _channel, _stub
    if _stub is None:
        async with _stub_lock:
            if _stub is None:
                documents_pb2, documents_pb2_grpc = load_documents_proto_modules()
                target = f"{settings.documents_grpc_host}:{settings.documents_grpc_port}"
                _channel = grpc.aio.insecure_channel(target)
                _stub = documents_pb2_grpc.DocumentServiceStub(_channel)
                return documents_pb2, _stub

    documents_pb2, _ = load_documents_proto_modules()
    return documents_pb2, _stub


async def close_documents_client() -> None:
    global _channel, _stub
    if _channel is not None:
        await _channel.close()
        _channel = None
        _stub = None


async def get_document_by_id(*, document_id: str, org_id: str) -> dict[str, Any] | None:
    documents_pb2, stub = await get_documents_stub()
    try:
        metadata = []
        if settings.internal_api_key:
            metadata = [("x-service-auth", settings.internal_api_key)]
        response = await stub.GetDocument(
            documents_pb2.GetDocumentRequest(
                document_id=document_id,
                org_id=org_id,
            ),
            metadata=metadata,
            timeout=30,
        )
    except grpc.aio.AioRpcError as exc:
        if exc.code() == grpc.StatusCode.NOT_FOUND:
            return None
        raise

    document = response.document
    return {
        "document_id": document.document_id,
        "org_id": document.org_id,
        "source": document.source,
        "type": document.type,
        "title": document.title,
        "content": document.content,
        "status": document.status,
        "metadata": json_format.MessageToDict(
            document.metadata,
            preserving_proto_field_name=True,
        ),
    }