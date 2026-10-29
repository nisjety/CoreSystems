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
