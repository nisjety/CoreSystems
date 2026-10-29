"""Control-issued ingress authority for Space-bound imports.

The BFF forwards this short-lived token only over the server-to-server import
request. Imports Core verifies it, turns it into a non-secret durable intent,
and discards the token. A different Control decision is reacquired at every
later Data Plane write by :mod:`app.service`.
"""

import base64
import json
import os
from datetime import datetime, timedelta, timezone
from typing import Any

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from app.auth_middleware import AuthContext
from app.schemas import SpaceImportIntent

_MAX_TOKEN_BYTES = 16 * 1024
_DECISION_VERSION = "v2"
_INGRESS_AUDIENCE = "ingestion-plane-import"
_ACTION_ID = "ingestion.import.write"
_SCHEMA_HASH = "sha256:ingestion-import-v1"


class SpaceImportIngressDenied(ValueError):
    """The supplied ingress decision is absent, invalid, or mismatched."""


def _b64(value: str) -> bytes:
    padded = value + "=" * (-len(value) % 4)
    return base64.urlsafe_b64decode(padded.encode("ascii"))


def _configured_keys() -> dict[str, Ed25519PublicKey]:
    raw = os.getenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON", "").strip()
    if raw:
        try:
            encoded = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise SpaceImportIngressDenied("Control Space decision key set is invalid") from exc
    else:
        key_id = os.getenv("CONTROL_SPACE_DECISION_KEY_ID", "").strip()
        key = os.getenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64", "").strip()
        encoded = {key_id: key} if key_id and key else {}
    if not isinstance(encoded, dict) or not encoded:
        raise SpaceImportIngressDenied("Control Space decision keys are unavailable")
    keys: dict[str, Ed25519PublicKey] = {}
    for key_id, encoded_key in encoded.items():
        if not isinstance(key_id, str) or not key_id.strip() or not isinstance(encoded_key, str):
            raise SpaceImportIngressDenied("Control Space decision key is invalid")
        try:
            key = _b64(encoded_key.strip())
            keys[key_id] = Ed25519PublicKey.from_public_bytes(key)
        except (ValueError, TypeError) as exc:
            raise SpaceImportIngressDenied("Control Space decision key is invalid") from exc
    return keys


def _required_string(claims: dict[str, Any], field: str) -> str:
    value = claims.get(field)
    if not isinstance(value, str) or not value.strip():
        raise SpaceImportIngressDenied("Space import decision is incomplete")
    return value.strip()


def verify_space_import_ingress_decision(token: str, auth: AuthContext) -> SpaceImportIntent:
    """Verify Control's BFF-only import decision and derive durable intent.

    The authenticated ingestion identity is independently matched to the signed
    decision, so a user cannot select another member's personal Space by
    supplying a copied reference or a forged browser header.
    """
    if auth.principal_type != "user" or not auth.user_id:
        raise SpaceImportIngressDenied("Space imports require an authenticated user")
    token = token.strip()
    if not token or len(token.encode("utf-8")) > _MAX_TOKEN_BYTES:
        raise SpaceImportIngressDenied("Space import decision is invalid")
    parts = token.split(".")
    if len(parts) != 4 or parts[0] != _DECISION_VERSION:
        raise SpaceImportIngressDenied("Space import decision is invalid")
    try:
        key_id = _b64(parts[1]).decode("utf-8")
        payload = _b64(parts[2])
        signature = _b64(parts[3])
    except (UnicodeDecodeError, ValueError, TypeError) as exc:
        raise SpaceImportIngressDenied("Space import decision is invalid") from exc
    if not key_id or not payload or len(payload) > _MAX_TOKEN_BYTES:
        raise SpaceImportIngressDenied("Space import decision is invalid")
    key = _configured_keys().get(key_id)
    if key is None:
        raise SpaceImportIngressDenied("Space import decision key is not trusted")
    try:
        key.verify(signature, ".".join(parts[:3]).encode("utf-8"))
        claims = json.loads(payload)
    except (ValueError, json.JSONDecodeError) as exc:
        raise SpaceImportIngressDenied("Space import decision signature is invalid") from exc
    if not isinstance(claims, dict):
        raise SpaceImportIngressDenied("Space import decision claims are invalid")

    now = datetime.now(timezone.utc)
    try:
        issued_at = datetime.fromisoformat(_required_string(claims, "issued_at").replace("Z", "+00:00"))
        expires_at = datetime.fromisoformat(_required_string(claims, "expires_at").replace("Z", "+00:00"))
    except ValueError as exc:
        raise SpaceImportIngressDenied("Space import decision timestamps are invalid") from exc
    if issued_at.tzinfo is None or expires_at.tzinfo is None or issued_at > now + timedelta(minutes=1) or expires_at <= now:
        raise SpaceImportIngressDenied("Space import decision is expired or not yet valid")

    if (
        _required_string(claims, "org_id") != auth.org_id
        or _required_string(claims, "subject_id") != auth.user_id
        or _required_string(claims, "service_audience") != _INGRESS_AUDIENCE
        or _required_string(claims, "action_id") != _ACTION_ID
        or _required_string(claims, "action_schema_hash") != _SCHEMA_HASH
        or claims.get("zero_data_retention") is not False
        or "ingestion:import" not in claims.get("permissions", [])
    ):
        raise SpaceImportIngressDenied("Space import decision does not authorize this request")

    try:
        return SpaceImportIntent.model_validate({
            "space_ref": _required_string(claims, "space_ref"),
            "subject_id": auth.user_id,
            "resource_authorization_ref": _required_string(claims, "resource_authorization_ref"),
            "recipient_audience_ref": _required_string(claims, "recipient_audience_ref"),
            "privacy_policy_ref": _required_string(claims, "privacy_policy_ref"),
            "authority_revision": claims.get("authority_revision"),
            "membership_revision": claims.get("membership_revision"),
            "privacy_revision": claims.get("privacy_revision"),
            "recipient_audience_revision": claims.get("recipient_audience_revision"),
            "entitlement_revision": claims.get("entitlement_revision"),
            "action_schema_hash": _required_string(claims, "action_schema_hash"),
            "payload_digest": _required_string(claims, "payload_digest"),
            "idempotency_key": _required_string(claims, "idempotency_key"),
            "source_type": _required_string(claims, "import_source_type"),
            "purpose": _required_string(claims, "purpose"),
            "lawful_basis": _required_string(claims, "lawful_basis"),
            "privacy_class": _required_string(claims, "privacy_class"),
            "third_party_allowed": claims.get("third_party_processing_allowed"),
            "retention_class": _required_string(claims, "retention_class"),
            "residency": _required_string(claims, "residency"),
            "deletion_scope": _required_string(claims, "deletion_scope"),
            "zero_data_retention": claims.get("zero_data_retention"),
        })
    except Exception as exc:
        raise SpaceImportIngressDenied("Space import decision claims are invalid") from exc
