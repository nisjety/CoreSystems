import asyncio
import json
from types import SimpleNamespace
from uuid import UUID

import httpx

from app import service
from app.models import ImportJob
from app.schemas import ImportDocument, SpaceImportIntent
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


def test_space_import_reauthorizes_at_write_time_and_requires_data_plane_attestation() -> None:
    seen: dict[str, object] = {}

    intent = SpaceImportIntent.model_validate({
        "space_ref": "space:org-1:user-1", "subject_id": "user-1",
        "resource_authorization_ref": "resource:import:1", "recipient_audience_ref": "audience:user-1:1",
        "privacy_policy_ref": "privacy:org-1:1", "authority_revision": 3, "membership_revision": 3,
        "privacy_revision": 2, "recipient_audience_revision": 3, "entitlement_revision": 4,
        "action_schema_hash": "sha256:ingestion-import-v1", "payload_digest": "sha256:payload",
        "idempotency_key": "import-1", "source_type": "notion", "purpose": "knowledge_import", "lawful_basis": "contract",
        "privacy_class": "internal", "third_party_allowed": False, "retention_class": "standard",
        "residency": "eu-north-1", "deletion_scope": "space", "zero_data_retention": False,
    })

    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/api/v1/internal/spaces/import-execution-decision":
            seen["reauth_headers"] = dict(request.headers)
            seen["reauth_body"] = json.loads(request.content)
            return httpx.Response(200, json={"data": {"token": "fresh-signed-space-decision", "decision": {
                "org_id": "org-1", "space_ref": intent.space_ref, "subject_id": intent.subject_id,
                "action_id": "ingestion.import.write", "service_audience": "data-plane-import",
                "payload_digest": intent.payload_digest, "permissions": ["documents:write"],
            }}})
        if request.url.path == "/api/data-plane/internal-token":
            return httpx.Response(200, json={"token": "signed-data-plane-token"})
        if request.url.path == "/v1/documents/":
            seen["document_headers"] = dict(request.headers)
            return httpx.Response(201, headers={"X-Space-Import-Authority-Accepted": "true"}, json={"document_id": "doc-1"})
        return httpx.Response(404)

    instance = ImportService()
    instance._settings = SimpleNamespace(
        auth_core_url="http://auth-core", ingestion_service_id="imports-core",
        ingestion_service_api_key="registered-key", document_service_url="http://documents-api",
        document_service_import_path="/v1/documents/", space_authority_reauthorization_url="http://user-core",
        space_authority_service_token="space-authority-service-token",
    )
    service.http_client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    service._data_plane_tokens.clear()
    job = ImportJob(id=UUID("00000000-0000-0000-0000-000000000001"), org_id="org-1", user_id="user-1", source_type="notion", status="running", space_import_intent=intent.model_dump(mode="json"))
    document = ImportDocument(text="durable content")

    async def run() -> None:
        await instance._store_document(job, document, UUID("00000000-0000-0000-0000-000000000002"))
        await service.http_client.aclose()

    asyncio.run(run())

    assert seen["reauth_headers"]["x-service-id"] == "imports-core"
    assert seen["reauth_body"] == {"intent": intent.model_dump(mode="json")}
    assert seen["document_headers"]["x-space-import-decision"] == "fresh-signed-space-decision"
