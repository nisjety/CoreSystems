"""Cron domain types."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


class CronTask(BaseModel):
    """Persistent cron schedule stored in agent_cron_tasks table."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    org_id: str
    session_id: str
    name: str
    cron_expr: str
    goal: str
    policy: dict[str, Any] = Field(default_factory=dict)
    enabled: bool = True
    last_run_at: datetime | None = None
    next_run_at: datetime | None = None
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))


class CreateCronRequest(BaseModel):
    org_id: str
    session_id: str
    name: str
    cron_expr: str
    goal: str
    policy: dict[str, Any] = Field(default_factory=dict)


class CronTaskResponse(BaseModel):
    cron_task: CronTask


class CronListResponse(BaseModel):
    cron_tasks: list[CronTask]
