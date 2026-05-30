from fastapi.testclient import TestClient

from app.api.health import router
from fastapi import FastAPI


def test_health() -> None:
    app = FastAPI()
    app.include_router(router)
    client = TestClient(app)
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.json() == {"status": "ok", "service": "cost-core-v2"}
