import base64
import json
from datetime import datetime, timedelta, timezone

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from app.auth_middleware import AuthContext
from app.schemas import SourceImportRequest
from app.space_import_authority import SpaceImportIngressDenied, verify_space_import_ingress_decision


def _token(monkeypatch, **overrides: object) -> str:
    private = Ed25519PrivateKey.generate()
    public = private.public_key().public_bytes_raw()
    monkeypatch.setenv("CONTROL_SPACE_DECISION_KEY_ID", "control-key-1")
    monkeypatch.setenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64", base64.urlsafe_b64encode(public).rstrip(b"=").decode())
    monkeypatch.setenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON", "")
    now = datetime.now(timezone.utc)
    claims: dict[str, object] = {
        "decision_ref": "decision-1", "org_id": "org-1", "space_ref": "space:org-1:user-1", "subject_id": "user-1",
        "service_audience": "ingestion-plane-import", "action_id": "ingestion.import.write", "action_schema_hash": "sha256:ingestion-import-v1",
        "payload_digest": "sha256:payload", "idempotency_key": "import-1", "recipient_audience_ref": "audience:user-1",
        "import_source_type": "notion",
        "privacy_policy_ref": "privacy:org-1", "resource_authorization_ref": "resource:import-1", "authority_revision": 1,
        "membership_revision": 1, "privacy_revision": 1, "recipient_audience_revision": 1, "entitlement_revision": 1,
        "permissions": ["ingestion:import"], "purpose": "knowledge_import", "lawful_basis": "contract", "privacy_class": "internal",
        "third_party_processing_allowed": False, "retention_class": "standard", "residency": "eu-north-1", "deletion_scope": "space",
        "zero_data_retention": False, "issued_at": now.isoformat(), "expires_at": (now + timedelta(minutes=1)).isoformat(), "nonce": "nonce-1",
    }
    claims.update(overrides)
    key_id = base64.urlsafe_b64encode(b"control-key-1").rstrip(b"=").decode()
    payload = base64.urlsafe_b64encode(json.dumps(claims, separators=(",", ":")).encode()).rstrip(b"=").decode()
    signed = f"v2.{key_id}.{payload}"
    signature = base64.urlsafe_b64encode(private.sign(signed.encode())).rstrip(b"=").decode()
    return f"{signed}.{signature}"


def test_ingress_decision_becomes_non_secret_space_import_intent(monkeypatch) -> None:
    intent = verify_space_import_ingress_decision(_token(monkeypatch), AuthContext(org_id="org-1", user_id="user-1"))

    assert intent.space_ref == "space:org-1:user-1"
    assert intent.subject_id == "user-1"
    assert "token" not in intent.model_dump(mode="json")


def test_ingress_decision_cannot_cross_user_or_org(monkeypatch) -> None:
    token = _token(monkeypatch)
    with pytest.raises(SpaceImportIngressDenied):
        verify_space_import_ingress_decision(token, AuthContext(org_id="org-1", user_id="other-user"))


def test_source_import_request_rejects_a_body_bearer() -> None:
    with pytest.raises(Exception):
        SourceImportRequest.model_validate({
            "source_type": "notion", "space_import_ingress_decision": "forged"
        })
