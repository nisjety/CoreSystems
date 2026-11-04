"""WS14-S6: the accepted paths of POST /api/v1/import/jobs/space-upload."""

import asyncio
from types import SimpleNamespace
from uuid import UUID

import pytest
from sqlalchemy.exc import IntegrityError

from app import service
from app.schemas import ImportDocument, SpaceImportIntent
from app.service import ImportService
from space_upload_support import JOB_ID, SPACE_REF, decision_token, docx_bytes, headers, install, uninstall

DOCX = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"


@pytest.fixture(autouse=True)
def _clean_overrides():
    yield
    uninstall()


def _two_files() -> list[tuple[str, tuple[str, bytes, str]]]:
    return [
        ("files", ("plan.docx", docx_bytes(), DOCX)),
        ("files", ("table.csv", b"name,qty\nbolt,4\nnut,8\n", "text/csv")),
    ]


def test_a_valid_decision_with_docx_and_csv_is_accepted_as_one_space_upload_job(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)

    response = client.post("/api/v1/import/jobs/space-upload", files=_two_files(), headers=headers(token))

    assert response.status_code == 202, response.text
    assert response.headers["cache-control"] == "no-store"
    data = response.json()["data"]
    assert data["upload_id"] == str(JOB_ID)
    assert data["deduplicated"] is False
    assert data["status"] == "queued"
    assert [f["file_name"] for f in data["files"]] == ["plan.docx", "table.csv"]
    assert {f["status"] for f in data["files"]} == {"queued"}

    assert len(recorder.create_calls) == 1
    call = recorder.create_calls[0]
    assert call["org_id"] == "org-1" and call["user_id"] == "user-1"
    intent = call["space_import_intent"]
    assert isinstance(intent, SpaceImportIntent)
    assert intent.source_type == "space_upload"
    assert intent.space_ref == SPACE_REF
    assert intent.idempotency_key == "upload-1"
    assert "token" not in intent.model_dump(mode="json")
    documents = call["documents"]
    assert len(documents) == 2
    assert "Quarterly plan" in documents[0].text
    assert documents[0].metadata["created_by_kind"] == "human"
    assert documents[0].metadata["scan_state"] == "not_scanned_text_only"
    assert documents[0].metadata["text_chars"] == len(documents[0].text)
    assert "bolt" in documents[1].text
    assert recorder.quota_calls == [("org-1", 2)]
    assert recorder.dispatched == [JOB_ID]


def test_the_same_decision_idempotency_key_replays_without_a_second_job_or_a_reparse(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)
    recorder.existing = JOB_ID

    from app import main

    def never_parsed(*_args, **_kwargs):
        raise AssertionError("a replay must not read or parse the files again")

    monkeypatch.setattr(main, "prepare_space_upload", never_parsed)

    response = client.post("/api/v1/import/jobs/space-upload", files=_two_files(), headers=headers(token))

    assert response.status_code == 200, response.text
    data = response.json()["data"]
    assert data["upload_id"] == str(JOB_ID)
    assert data["deduplicated"] is True
    assert [f["file_name"] for f in data["files"]] == ["plan.docx", "table.csv"]
    assert recorder.find_calls == [("org-1", "user-1", SPACE_REF, "upload-1")]
    assert recorder.create_calls == []
    assert recorder.quota_calls == []
    assert recorder.dispatched == []


def test_losing_the_unique_index_race_returns_the_winner_and_does_not_dispatch_again(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)
    recorder.created = False  # create_space_upload_job reports the winner's job

    response = client.post("/api/v1/import/jobs/space-upload", files=_two_files(), headers=headers(token))

    assert response.status_code == 200, response.text
    assert response.json()["data"]["deduplicated"] is True
    assert response.json()["data"]["upload_id"] == str(JOB_ID)
    assert recorder.dispatched == []


def test_idempotency_key_header_is_optional_when_the_decision_carries_one(monkeypatch) -> None:
    client, recorder = install(monkeypatch)
    token = decision_token(monkeypatch)
    response = client.post(
        "/api/v1/import/jobs/space-upload", files=_two_files(), headers=headers(token, key=None)
    )
    assert response.status_code == 202, response.text
    assert recorder.find_calls[0][3] == "upload-1"


# ── ImportService.create_space_upload_job (the job row and the race) ──────────


def _intent() -> SpaceImportIntent:
    return SpaceImportIntent.model_validate({
        "space_ref": SPACE_REF, "subject_id": "user-1", "resource_authorization_ref": "resource:upload-1",
        "recipient_audience_ref": "audience:user-1", "privacy_policy_ref": "privacy:org-1",
        "authority_revision": 1, "membership_revision": 1, "privacy_revision": 1,
        "recipient_audience_revision": 1, "entitlement_revision": 1,
        "action_schema_hash": "sha256:ingestion-import-v1", "payload_digest": "sha256:payload",
        "idempotency_key": "upload-1", "source_type": "space_upload", "purpose": "room_file",
        "lawful_basis": "contract", "privacy_class": "internal", "third_party_allowed": False,
        "retention_class": "standard", "residency": "eu-north-1", "deletion_scope": "space",
        "zero_data_retention": False,
    })


def _service(monkeypatch, *, existing, create) -> tuple[ImportService, dict]:
    instance = ImportService()
    instance._settings = SimpleNamespace()
    seen: dict = {"finds": 0, "creates": []}
    answers = list(existing)

    async def find(org_id, user_id, space_ref, key):
        seen["finds"] += 1
        return answers.pop(0) if answers else None

    async def create_job(**kwargs):
        seen["creates"].append(kwargs)
        return await create(kwargs)

    monkeypatch.setattr(instance, "find_space_upload_job", find)
    monkeypatch.setattr(instance, "create_job", create_job)
    return instance, seen


def test_create_space_upload_job_creates_a_space_upload_job_carrying_the_intent(monkeypatch) -> None:
    async def create(_kwargs):
        return JOB_ID

    instance, seen = _service(monkeypatch, existing=[None], create=create)
    docs = [ImportDocument(source_name="a.txt", text="hello")]

    job_id, created = asyncio.run(
        instance.create_space_upload_job("org-1", "user-1", docs, {"upload_count": 1}, _intent())
    )

    assert (job_id, created) == (JOB_ID, True)
    kwargs = seen["creates"][0]
    assert kwargs["source_type"] == "space_upload"
    assert kwargs["space_import_intent"] == _intent()
    assert kwargs["user_id"] == "user-1"


def test_create_space_upload_job_returns_an_existing_job_without_creating(monkeypatch) -> None:
    async def create(_kwargs):
        raise AssertionError("must not create")

    other = UUID("00000000-0000-0000-0000-0000000000b2")
    instance, seen = _service(monkeypatch, existing=[other], create=create)
    result = asyncio.run(instance.create_space_upload_job("org-1", "user-1", [], {}, _intent()))
    assert result == (other, False)
    assert seen["creates"] == []


def test_create_space_upload_job_maps_a_unique_violation_to_the_winner(monkeypatch) -> None:
    winner = UUID("00000000-0000-0000-0000-0000000000c3")

    async def create(_kwargs):
        raise IntegrityError("insert", {}, Exception("uq_import_jobs_space_upload_idem"))

    instance, seen = _service(monkeypatch, existing=[None, winner], create=create)
    result = asyncio.run(instance.create_space_upload_job("org-1", "user-1", [], {}, _intent()))
    assert result == (winner, False)
    assert seen["finds"] == 2


def test_create_space_upload_job_reraises_a_violation_that_has_no_winner(monkeypatch) -> None:
    async def create(_kwargs):
        raise IntegrityError("insert", {}, Exception("some other constraint"))

    instance, _ = _service(monkeypatch, existing=[None, None], create=create)
    with pytest.raises(IntegrityError):
        asyncio.run(instance.create_space_upload_job("org-1", "user-1", [], {}, _intent()))


def test_find_space_upload_job_is_scoped_to_org_uploader_space_and_key(monkeypatch) -> None:
    class _Result:
        def scalars(self):
            return self

        def first(self):
            return None

    class _Session:
        async def __aenter__(self):
            return self

        async def __aexit__(self, *_):
            return False

        async def execute(self, query):
            self.query = query
            return _Result()

    session = _Session()
    monkeypatch.setattr(service, "SessionLocal", lambda: session)

    asyncio.run(ImportService().find_space_upload_job("org-a", "user-a", SPACE_REF, "key-1"))

    from sqlalchemy.dialects import postgresql

    sql = str(session.query.compile(dialect=postgresql.dialect(), compile_kwargs={"literal_binds": True}))
    for needle in ("import_jobs.org_id = 'org-a'", "import_jobs.user_id = 'user-a'", "import_jobs.source_type = 'space_upload'"):
        assert needle in sql, sql
    assert SPACE_REF in sql and "key-1" in sql


def test_the_idempotency_migration_is_a_partial_unique_index_on_space_uploads() -> None:
    from pathlib import Path

    migration = (
        Path(__file__).parents[1] / "migrations" / "005_space_upload_idempotency.sql"
    ).read_text(encoding="utf-8").lower()
    assert "create unique index if not exists uq_import_jobs_space_upload_idem" in migration
    for column in ("org_id", "user_id", "space_import_intent->>'space_ref'", "space_import_intent->>'idempotency_key'"):
        assert column in migration
    assert "where source_type = 'space_upload'" in migration
