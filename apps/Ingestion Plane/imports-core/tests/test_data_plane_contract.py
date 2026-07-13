import asyncio
import json
from types import SimpleNamespace
from uuid import UUID

import httpx

from app import service
from app.models import ImportJob
from app.schemas import ImportDocument
from app.service import ImportService


def test_store_document_mints_scoped_token_and_uses_live_data_plane_contract() -> None:
    seen: dict[str, object] = {}

    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/api/data-plane/internal-token":
            seen["token_headers"] = dict(request.headers)
            seen["token_body"] = json.loads(request.content)
            return httpx.Response(200, json={"token": "signed-data-plane-token"})
        if request.url.path == "/v1/documents/":
            seen["document_headers"] = dict(request.headers)
            seen["document_body"] = json.loads(request.content)
            return httpx.Response(201, json={"document_id": "doc-1"})
        return httpx.Response(404)

    instance = ImportService()
    instance._settings = SimpleNamespace(
        auth_core_url="http://auth-core",
        ingestion_service_id="imports-core",
        ingestion_service_api_key="registered-key",
        document_service_url="http://documents-api",
        document_service_import_path="/v1/documents/",
    )
    service.http_client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    service._data_plane_tokens.clear()
    job = ImportJob(
        id=UUID("00000000-0000-0000-0000-000000000001"),
        org_id="org-1",
        user_id="user-1",
        source_type="upload",
        status="running",
    )
    document = ImportDocument(
        source_id="file-1",
        source_name="notes.md",
        title="Notes",
        text="durable content",
        metadata={"content_type": "text/markdown"},
    )

    async def run() -> None:
        await instance._store_document(
            job, document, UUID("00000000-0000-0000-0000-000000000002")
        )
        await service.http_client.aclose()

    asyncio.run(run())

    assert seen["token_body"] == {
        "orgId": "org-1",
        "scopes": ["documents:write"],
        "reason": "imports-core durable document ingest",
    }
    assert seen["token_headers"]["x-service-id"] == "imports-core"
    assert seen["document_headers"]["authorization"] == "Bearer signed-data-plane-token"
    assert seen["document_headers"]["x-org-id"] == "org-1"
    body = seen["document_body"]
    assert body["type"] == "upload"
    assert body["title"] == "Notes"
    assert body["content"] == "durable content"
    assert body["idempotency_key"].endswith("00000000-0000-0000-0000-000000000002")
    assert body["ingest_policy"] == {"zdr_mode": "off", "ephemeral_only": False}
