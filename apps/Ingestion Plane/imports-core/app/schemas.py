from datetime import datetime
from typing import Any, Literal
from uuid import UUID

from pydantic import BaseModel, Field


class ImportDocument(BaseModel):
    source_id: str | None = None
    source_name: str | None = None
    title: str | None = None
    text: str
    metadata: dict[str, Any] = Field(default_factory=dict)


class SourceImportRequest(BaseModel):
    # Deprecated compatibility fields. Authorization always uses signed claims.
    org_id: str | None = None
    user_id: str | None = None
    source_type: Literal["notion", "crm", "erp", "cms", "pim", "hubspot", "salesforce", "odoo"]
    connection: dict[str, Any] = Field(default_factory=dict)
    options: dict[str, Any] = Field(default_factory=dict)
    zdr: bool = False


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
