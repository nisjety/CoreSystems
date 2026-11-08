import pytest

from app import service


class _Result:
    def scalar_one_or_none(self):
        return None


class _Session:
    async def __aenter__(self):
        return self

    async def __aexit__(self, *_):
        return False

    async def execute(self, query):
        self.query = query
        return _Result()


@pytest.mark.asyncio
async def test_job_lookup_scopes_by_org(monkeypatch):
    session = _Session()
    monkeypatch.setattr(service, "SessionLocal", lambda: session)

    await service.import_service.get_job_with_items("00000000-0000-0000-0000-000000000001", "org-a")

    sql = str(session.query.compile(compile_kwargs={"literal_binds": True}))
    assert "import_jobs.org_id = 'org-a'" in sql


@pytest.mark.asyncio
async def test_job_lookup_without_org_is_only_for_internal_service_calls(monkeypatch):
    session = _Session()
    monkeypatch.setattr(service, "SessionLocal", lambda: session)

    await service.import_service.get_job("00000000-0000-0000-0000-000000000001")

    sql = str(session.query.compile(compile_kwargs={"literal_binds": True}))
    assert "WHERE import_jobs.org_id" not in sql
    assert "AND import_jobs.org_id" not in sql


# ── WS14-S6: a room upload is the uploader's alone ───────────────────────────

from datetime import datetime, timezone  # noqa: E402
from types import SimpleNamespace  # noqa: E402
from uuid import UUID  # noqa: E402

import space_upload_support  # noqa: E402,F401  (sets the env app.main needs at import)
from app.auth_middleware import AuthContext  # noqa: E402

_JOB_ID = UUID("00000000-0000-0000-0000-0000000000a1")


def _job(source_type: str, user_id: str = "uploader"):
    return SimpleNamespace(
        id=_JOB_ID, org_id="org-1", user_id=user_id, source_type=source_type, status="completed",
        total_items=1, processed_items=1, failed_items=0, error_message=None, metadata_json={},
        created_at=datetime(2026, 11, 2, tzinfo=timezone.utc), started_at=None, completed_at=None,
    )


def test_job_readable_by_scopes_only_space_uploads_to_the_uploader() -> None:
    from app.main import _job_readable_by

    uploader = AuthContext(org_id="org-1", user_id="uploader")
    other = AuthContext(org_id="org-1", user_id="someone-else")
    service_principal = AuthContext(org_id="org-1", service_name="gateway", principal_type="service", scopes=("imports:read",))
    no_user = AuthContext(org_id="org-1", user_id=None)

    room = _job("space_upload")
    assert _job_readable_by(room, uploader) is True
    assert _job_readable_by(room, other) is False
    assert _job_readable_by(room, service_principal) is False
    assert _job_readable_by(room, no_user) is False
    # A room upload that somehow has no uploader is readable by nobody.
    assert _job_readable_by(_job("space_upload", user_id=None), no_user) is False

    # Every other job stays org-scoped (the org filter is applied by the lookup).
    for source in ("upload", "notion", "hubspot"):
        assert _job_readable_by(_job(source), other) is True
        assert _job_readable_by(_job(source), service_principal) is True


@pytest.mark.parametrize("path", ["/api/v1/import/jobs/{id}", "/api/v1/import/jobs/{id}/events"])
def test_job_and_events_routes_hide_a_room_upload_from_other_users_in_the_same_org(monkeypatch, path) -> None:
    from fastapi.testclient import TestClient

    from app import main
    from app.auth_middleware import require_internal_auth

    async def get_job_with_items(job_id, org_id=None):
        assert org_id == "org-1", "the lookup stays org-scoped"
        return _job("space_upload"), []

    async def get_job(job_id, org_id=None):
        assert org_id == "org-1"
        return _job("space_upload")

    monkeypatch.setattr(main.import_service, "get_job_with_items", get_job_with_items)
    monkeypatch.setattr(main.import_service, "get_job", get_job)
    client = TestClient(main.app)
    url = path.format(id=_JOB_ID)
    try:
        main.app.dependency_overrides[require_internal_auth] = lambda: AuthContext(org_id="org-1", user_id="someone-else")
        assert client.get(url).status_code == 404

        if path.endswith("events"):
            return  # a 200 here is an open SSE stream; the detail route covers the uploader's 200
        main.app.dependency_overrides[require_internal_auth] = lambda: AuthContext(org_id="org-1", user_id="uploader")
        response = client.get(url)
        assert response.status_code == 200, response.text
        assert response.json()["source_type"] == "space_upload"
    finally:
        main.app.dependency_overrides.clear()


def test_a_non_room_job_is_still_readable_by_any_user_of_the_org(monkeypatch) -> None:
    from fastapi.testclient import TestClient

    from app import main
    from app.auth_middleware import require_internal_auth

    async def get_job_with_items(job_id, org_id=None):
        return _job("upload"), []

    monkeypatch.setattr(main.import_service, "get_job_with_items", get_job_with_items)
    main.app.dependency_overrides[require_internal_auth] = lambda: AuthContext(org_id="org-1", user_id="someone-else")
    try:
        assert TestClient(main.app).get(f"/api/v1/import/jobs/{_JOB_ID}").status_code == 200
    finally:
        main.app.dependency_overrides.clear()
