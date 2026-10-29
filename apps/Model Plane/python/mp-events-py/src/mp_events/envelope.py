"""Canonical event envelope mirroring the Go and Rust implementations."""

from __future__ import annotations

import json
from datetime import datetime
from typing import Any

import blake3
from pydantic import BaseModel, ConfigDict, Field


class Envelope(BaseModel):
    """Canonical 12-field envelope for Model Plane events.

    Wire format: JSON with sorted keys and UTC ISO-8601 timestamps ending in ``Z``.
    """

    model_config = ConfigDict(extra="forbid")

    event_id: str = Field(min_length=1)
    event_type: str = Field(min_length=1)
    schema_version: int = Field(ge=0)
    ts: datetime
    producer: str = Field(min_length=1)
    correlation_id: str
    causation_id: str
    idempotency_key: str
    org_id: str = Field(min_length=1)
    user_id: str = Field(min_length=1)
    resource_ref: str = Field(min_length=1)
    payload: dict[str, Any]

    def encode(self) -> bytes:
        """Encode to canonical JSON bytes with sorted keys."""
        data = self.model_dump(mode="json")
        return json.dumps(data, sort_keys=True, separators=(",", ":")).encode("utf-8")

    @classmethod
    def decode(cls, raw: bytes) -> "Envelope":
        """Decode canonical JSON bytes into an :class:`Envelope`."""
        return cls.model_validate_json(raw)


def derive_idempotency_hash(
    producer: str,
    event_type: str,
    resource_ref: str,
    idempotency_key: str,
) -> str:
    """Return the blake3 hex digest of ``producer|event_type|resource_ref|idempotency_key``.

    Must match the Go (``pkg/envelope.DeriveIdempotencyHash``) and Rust
    (``mp_events::derive_idempotency_hash``) outputs byte-for-byte.
    """
    material = f"{producer}|{event_type}|{resource_ref}|{idempotency_key}".encode("utf-8")
    return blake3.blake3(material).hexdigest()
