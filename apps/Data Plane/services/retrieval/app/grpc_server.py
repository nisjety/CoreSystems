from __future__ import annotations

import json
import logging
from typing import Any

import grpc
from google.protobuf import json_format
from google.protobuf.struct_pb2 import Struct

from app.config import settings
from app.grpc_runtime import load_retrieval_proto_modules
from app.retrieval.service import QuotaExceededError, execute_retrieval
from app.db import _session
from sqlalchemy import text

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


async def _get_user_document_ids(*, user_id: str, org_id: str) -> list[str]:
    """Return document_ids the user has been explicitly granted access to."""
    async with _session() as session:
        result = await session.execute(
            text(
                "SELECT document_id FROM document_acl "
                "WHERE user_id = :uid AND org_id = :oid"
            ),
            {"uid": user_id, "oid": org_id},
        )
        return [row["document_id"] for row in result.mappings()]


async def create_grpc_server() -> grpc.aio.Server:
    retrieval_pb2, retrieval_pb2_grpc = load_retrieval_proto_modules()

    class RetrievalServiceServicer(retrieval_pb2_grpc.RetrievalServiceServicer):
        async def Retrieve(self, request: Any, context: grpc.aio.ServicerContext) -> Any:
            filters = request.filters

            # ACL gate: if a user_id is provided, restrict to their allowed documents
            document_ids: list[str] | None = list(filters.document_ids) or None
            if request.HasField("user_id") and request.user_id:
                allowed_ids = await _get_user_document_ids(
                    user_id=request.user_id, org_id=request.org_id
                )
                if not allowed_ids:
                    await context.abort(grpc.StatusCode.PERMISSION_DENIED, "No document access")
                    return retrieval_pb2.RetrieveResponse()
                document_ids = allowed_ids

            try:
                result = await execute_retrieval(
                    org_id=request.org_id,
                    query=request.query,
                    document_types=list(filters.document_types) or None,
                    departments=list(filters.departments) or None,
                    languages=list(filters.languages) or None,
                    document_ids=document_ids,
                    region=filters.region or None,
                    top_k=request.top_k or None,
                    top_n=None,
                )
            except QuotaExceededError as exc:
                await context.abort(grpc.StatusCode.RESOURCE_EXHAUSTED, str(exc))
            except ValueError as exc:
                await context.abort(grpc.StatusCode.INVALID_ARGUMENT, str(exc))
            except Exception as exc:
                logger.exception("retrieval gRPC request failed: %s", exc)
                await context.abort(grpc.StatusCode.INTERNAL, "Retrieval failed")

            return retrieval_pb2.RetrieveResponse(
                facts=[
                    retrieval_pb2.Fact(
                        knowledge_id=fact["knowledge_id"],
                        document_id=fact["document_id"],
                        text=fact["text"],
                        score=float(fact.get("rerank_score") or fact.get("score") or 0.0),
                        metadata=_struct_from_mapping(fact.get("metadata")),
                    )
                    for fact in result["facts"]
                ],
                sources=[
                    retrieval_pb2.Source(
                        document_id=source["document_id"],
                        title=source.get("title", ""),
                        source=source.get("source", ""),
                        type=source.get("type", ""),
                    )
                    for source in result["sources"]
                ],
                query=result["query"],
                org_id=result["org_id"],
            )

    server = grpc.aio.server()
    retrieval_pb2_grpc.add_RetrievalServiceServicer_to_server(
        RetrievalServiceServicer(),
        server,
    )
    bind_address = f"[::]:{settings.grpc_port}"
    server.add_insecure_port(bind_address)
    logger.info("retrieval-service configured gRPC listener on %s", bind_address)
    return server