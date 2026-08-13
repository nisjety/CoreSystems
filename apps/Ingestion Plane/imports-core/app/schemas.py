from datetime import datetime
from typing import Any, Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class ImportDocument(BaseModel):
    source_id: str | None = None
    source_name: str | None = None
    title: str | None = None
    text: str
    metadata: dict[str, Any] = Field(default_factory=dict)


class SpaceImportIntent(BaseModel):
    """Durable non-secret record of a Space-scoped import request.

    The short-lived signed decision is deliberately excluded. A recoverable
    worker must obtain fresh authority immediately before each Data write.
    """

    model_config = ConfigDict(extra="forbid", frozen=True)

    space_ref: str = Field(min_length=1, max_length=256)
    subject_id: str = Field(min_length=1, max_length=256)
    resource_authorization_ref: str = Field(min_length=1, max_length=512)
    recipient_audience_ref: str = Field(min_length=1, max_length=512)
    privacy_policy_ref: str = Field(min_length=1, max_length=512)
    authority_revision: int = Field(ge=1)
    membership_revision: int = Field(ge=1)
    privacy_revision: int = Field(ge=1)
    recipient_audience_revision: int = Field(ge=1)
    entitlement_revision: int = Field(ge=1)
    action_id: Literal["ingestion.import.write"] = "ingestion.import.write"
    action_schema_hash: Literal["sha256:ingestion-import-v1"]
    payload_digest: str = Field(pattern=r"^sha256:[0-9A-Za-z._-]{1,256}$")
    idempotency_key: str = Field(min_length=1, max_length=200)
    source_type: str = Field(pattern=r"^[a-z0-9_-]{1,64}$")
    purpose: str = Field(min_length=1, max_length=128)
    lawful_basis: str = Field(min_length=1, max_length=128)
    privacy_class: str = Field(min_length=1, max_length=128)
    third_party_allowed: bool
    retention_class: str = Field(min_length=1, max_length=128)
    residency: str = Field(min_length=1, max_length=128)
    deletion_scope: str = Field(min_length=1, max_length=128)
    zero_data_retention: bool


class SourceImportRequest(BaseModel):
    # Deprecated compatibility fields. Authorization always uses signed claims.
    org_id: str | None = None
    user_id: str | None = None
    source_type: Literal["notion", "crm", "erp", "cms", "pim", "hubspot", "salesforce", "odoo"]
    connection: dict[str, Any] = Field(default_factory=dict)
    options: dict[str, Any] = Field(default_factory=dict)
    zdr: bool = False

    # Keep authorization strictly in the protected header decoded by the
    # ingress boundary. A JSON field named like a decision token must be an
    # explicit 422, not ignored and later accidentally persisted as metadata.
    model_config = ConfigDict(extra="forbid")


class JobResponse(BaseModel):
    id: UUID
    org_id: str
    user_id: str | None = None
    source_type: str
    status: str
    total_items: int
    processed_items: int
    failed_items: int
    error_message: str | None = None
    metadata: dict[str, Any]
    created_at: datetime
    started_at: datetime | None = None
    completed_at: datetime | None = None


class JobItemResponse(BaseModel):
    id: UUID
    job_id: UUID
    source_id: str | None = None
    source_name: str | None = None
    status: str
    error_message: str | None = None
    metadata: dict[str, Any]
    created_at: datetime
    completed_at: datetime | None = None


class JobDetailResponse(JobResponse):
    items: list[JobItemResponse]


class ProgressEvent(BaseModel):
    event: str
    job_id: UUID
    status: str
    processed_items: int
    total_items: int
    failed_items: int
    message: str | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)
