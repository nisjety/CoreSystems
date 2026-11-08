from pathlib import Path

from app.models import ImportJob, ImportJobItem


def test_job_models_include_recovery_lease_and_durable_payload() -> None:
    job_columns = ImportJob.__table__.columns
    item_columns = ImportJobItem.__table__.columns

    assert {"lease_owner", "lease_expires_at", "attempts"}.issubset(job_columns.keys())
    assert "document_payload" in item_columns


def test_durable_job_migration_is_idempotent_and_indexed() -> None:
    migration = (
        Path(__file__).parents[1] / "migrations" / "003_durable_job_leases.sql"
    ).read_text(encoding="utf-8").lower()

    assert migration.count("add column if not exists") == 4
    assert "idx_import_jobs_recovery" in migration


# ── WS14-S6: room-file item results ──────────────────────────────────────────

import asyncio  # noqa: E402
import json  # noqa: E402
from types import SimpleNamespace  # noqa: E402
from uuid import UUID  # noqa: E402

import httpx  # noqa: E402
import pytest  # noqa: E402

from app import service  # noqa: E402
from app.schemas import ImportDocument  # noqa: E402
from app.service import ImportService, SpaceImportAuthorityUnavailable  # noqa: E402


def _status_error(status: int, body: bytes | dict | None) -> httpx.HTTPStatusError:
    request = httpx.Request("POST", "http://documents-api/v1/documents/")
    if isinstance(body, dict):
        response = httpx.Response(status, json=body, request=request)
    else:
        response = httpx.Response(status, content=body or b"", request=request)
    return httpx.HTTPStatusError("failed", request=request, response=response)


def test_space_upload_error_code_maps_documents_api_answers_to_stable_codes() -> None:
    code = ImportService._space_upload_error_code
    assert code(_status_error(409, {"code": "room_file_duplicate", "error": "the room already holds this file"})) == "room_file_duplicate"
    assert code(_status_error(409, {"code": "room_files_quota_exceeded"})) == "room_files_quota_exceeded"
    assert code(_status_error(403, {"code": "space_files_not_authorized"})) == "space_files_not_authorized"
    assert code(_status_error(404, b"not json")) == "room_files_disabled"
    assert code(_status_error(500, b"<html>boom</html>")) == "room_file_store_failed"
    assert code(_status_error(500, {"error": "boom"})) == "room_file_store_failed"
    assert code(SpaceImportAuthorityUnavailable("Data Plane did not attest")) == "space_files_not_authorized"
    assert code(RuntimeError("secret text inside the file")) == "room_file_store_failed"
    assert code(httpx.ConnectError("refused")) == "room_file_store_failed"


def _store(job_source_type: str, response: httpx.Response):
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/api/data-plane/internal-token":
            return httpx.Response(200, json={"token": "signed-data-plane-token"})
        if request.url.path == "/v1/documents/":
            seen["body"] = json.loads(request.content)
            return response
        return httpx.Response(404)

    seen: dict = {}
    instance = ImportService()
    instance._settings = SimpleNamespace(
        auth_core_url="http://auth-core", ingestion_service_id="imports-core",
        ingestion_service_api_key="registered-key", document_service_url="http://documents-api",
        document_service_import_path="/v1/documents/",
    )
    service.http_client = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    service._data_plane_tokens.clear()
    from app.models import ImportJob

    job = ImportJob(
        id=UUID("00000000-0000-0000-0000-000000000001"), org_id="org-1", user_id="user-1",
        source_type=job_source_type, status="running",
    )

    async def run():
        try:
            return await instance._store_document(
                job, ImportDocument(source_name="a.txt", text="durable content"), UUID("00000000-0000-0000-0000-000000000002")
            )
        finally:
            await service.http_client.aclose()

    return asyncio.run(run()), seen


def test_store_document_returns_the_document_id_for_a_non_space_upload_job_as_none() -> None:
    document_id, seen = _store("upload", httpx.Response(201, json={"document_id": "doc-1"}))
    assert document_id is None
    assert seen["body"]["type"] == "upload"
    assert "upload_id" not in seen["body"]["metadata"]


def test_store_document_for_a_room_upload_returns_the_id_and_tags_the_upload() -> None:
    document_id, seen = _store("space_upload", httpx.Response(201, json={"document_id": "doc-77"}))
    assert document_id == "doc-77"
    assert seen["body"]["metadata"]["upload_id"] == "00000000-0000-0000-0000-000000000001"


@pytest.mark.parametrize(
    "response",
    [
        httpx.Response(201, content=b"not json"),
        httpx.Response(201, json=["document_id"]),
        httpx.Response(201, json={"document_id": ""}),
        httpx.Response(201, json={"document_id": 7}),
        httpx.Response(201, json={}),
    ],
)
def test_store_document_for_a_room_upload_tolerates_a_body_without_a_usable_id(response) -> None:
    document_id, _ = _store("space_upload", response)
    assert document_id is None
