"""Shared fixtures for the WS14-S6 room-file upload tests (not a test module).

Builds Control-style signed ingress decisions and a TestClient over the real
FastAPI app with the verified-auth dependency replaced, so the route's own
order of checks (flag, ZDR, decision, replay, validation, quota, job) runs for
real while the database, the quota service and the orchestrator are stubbed.
"""

import base64
import io
import json
import os
from datetime import datetime, timedelta, timezone
from types import SimpleNamespace
from typing import Any
from uuid import UUID

os.environ.setdefault("DATABASE_URL", "postgresql+asyncpg://unused:unused@127.0.0.1:1/unused")
os.environ.setdefault("DOCUMENT_SERVICE_URL", "http://127.0.0.1:1")
os.environ.setdefault("ORG_SERVICE_URL", "http://127.0.0.1:1")
os.environ.setdefault("INTERNAL_API_KEY", "space-upload-test-internal-key")

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey  # noqa: E402

JOB_ID = UUID("00000000-0000-0000-0000-0000000000a1")
SPACE_REF = "space:org-1:room-7"


def decision_token(monkeypatch: Any, **overrides: object) -> str:
    """A signed `space_upload` ingress decision for user-1 in org-1."""
    private = Ed25519PrivateKey.generate()
    public = private.public_key().public_bytes_raw()
    monkeypatch.setenv("CONTROL_SPACE_DECISION_KEY_ID", "control-key-1")
    monkeypatch.setenv(
        "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64",
        base64.urlsafe_b64encode(public).rstrip(b"=").decode(),
    )
    monkeypatch.setenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON", "")
    now = datetime.now(timezone.utc)
    claims: dict[str, object] = {
        "decision_ref": "decision-1", "org_id": "org-1", "space_ref": SPACE_REF, "subject_id": "user-1",
        "service_audience": "ingestion-plane-import", "action_id": "ingestion.import.write",
        "action_schema_hash": "sha256:ingestion-import-v1", "payload_digest": "sha256:payload",
        "idempotency_key": "upload-1", "recipient_audience_ref": "audience:user-1",
        "import_source_type": "space_upload", "privacy_policy_ref": "privacy:org-1",
        "resource_authorization_ref": "resource:upload-1", "authority_revision": 1,
        "membership_revision": 1, "privacy_revision": 1, "recipient_audience_revision": 1,
        "entitlement_revision": 1, "permissions": ["ingestion:import"], "purpose": "room_file",
        "lawful_basis": "contract", "privacy_class": "internal", "third_party_processing_allowed": False,
        "retention_class": "standard", "residency": "eu-north-1", "deletion_scope": "space",
        "zero_data_retention": False, "issued_at": now.isoformat(),
        "expires_at": (now + timedelta(minutes=1)).isoformat(), "nonce": "nonce-1",
    }
    claims.update(overrides)
    key_id = base64.urlsafe_b64encode(b"control-key-1").rstrip(b"=").decode()
    payload = base64.urlsafe_b64encode(json.dumps(claims, separators=(",", ":")).encode()).rstrip(b"=").decode()
    signed = f"v2.{key_id}.{payload}"
    signature = base64.urlsafe_b64encode(private.sign(signed.encode())).rstrip(b"=").decode()
    return f"{signed}.{signature}"


def docx_bytes(text: str = "Quarterly plan for the room") -> bytes:
    from docx import Document

    document = Document()
    document.add_paragraph(text)
    buffer = io.BytesIO()
    document.save(buffer)
    return buffer.getvalue()


def headers(token: str | None, key: str | None = "upload-1", **extra: str) -> dict[str, str]:
    out: dict[str, str] = {}
    if token is not None:
        out["X-Space-Import-Ingress-Decision"] = token
    if key is not None:
        out["Idempotency-Key"] = key
    out.update(extra)
    return out


class Recorder:
    """What the stubbed service and orchestrator were asked."""

    def __init__(self) -> None:
        self.find_calls: list[tuple] = []
        self.create_calls: list[dict] = []
        self.quota_calls: list[tuple] = []
        self.dispatched: list[UUID] = []
        self.existing: UUID | None = None
        self.created: bool = True
        self.quota_allowed: bool = True


def install(monkeypatch: Any, *, enabled: bool = True, user_id: str = "user-1") -> tuple[Any, Recorder]:
    """Return (TestClient, Recorder) over the real app with stubbed collaborators."""
    from fastapi.testclient import TestClient

    from app import main
    from app.auth_middleware import AuthContext, require_internal_auth

    recorder = Recorder()

    async def find_space_upload_job(org_id, uploader, space_ref, key):
        recorder.find_calls.append((org_id, uploader, space_ref, key))
        return recorder.existing

    async def check_quota(org_id, items):
        recorder.quota_calls.append((org_id, items))
        return recorder.quota_allowed

    async def create_space_upload_job(**kwargs):
        recorder.create_calls.append(kwargs)
        return JOB_ID, recorder.created

    async def get_job_with_items(job_id, org_id=None):
        job = SimpleNamespace(id=job_id, status="completed")
        items = [
            SimpleNamespace(source_name="plan.docx", status="completed"),
            SimpleNamespace(source_name="table.csv", status="completed"),
        ]
        return job, items

    async def dispatch(job_id, _runner):
        recorder.dispatched.append(job_id)

    monkeypatch.setattr(main.settings, "imports_space_upload", "true" if enabled else "false")
    monkeypatch.setattr(main.import_service, "find_space_upload_job", find_space_upload_job)
    monkeypatch.setattr(main.import_service, "check_quota", check_quota)
    monkeypatch.setattr(main.import_service, "create_space_upload_job", create_space_upload_job)
    monkeypatch.setattr(main.import_service, "get_job_with_items", get_job_with_items)
    monkeypatch.setattr(main.orchestrator, "dispatch", dispatch)
    main.app.dependency_overrides[require_internal_auth] = lambda: AuthContext(org_id="org-1", user_id=user_id)
    client = TestClient(main.app)  # no `with`: the lifespan (NATS, migrations) must not start
    return client, recorder


def uninstall() -> None:
    from app import main

    main.app.dependency_overrides.clear()
