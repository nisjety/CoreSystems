from __future__ import annotations

from datetime import datetime
from typing import Any, Dict, List, Optional
from uuid import UUID, uuid4

from fastapi import APIRouter, Query
from pydantic import BaseModel, Field

from app import repository

router = APIRouter(prefix="/v1/analytics", tags=["analytics"])


class AnalyticsEventIn(BaseModel):
    event_id: Optional[UUID] = None
    event_type: str
    org_id: str
    run_id: Optional[str] = None
    user_id: Optional[str] = None
    agent_id: Optional[str] = None
    props: Optional[Dict[str, Any]] = Field(default_factory=dict)
    ts: Optional[datetime] = None


class AnalyticsEventOut(BaseModel):
    event_id: UUID


@router.post("/events", response_model=AnalyticsEventOut)
async def ingest_event(body: AnalyticsEventIn) -> AnalyticsEventOut:
    event_id = body.event_id or uuid4()
    await repository.insert_analytics_event(
        event_id=event_id,
        event_type=body.event_type,
        org_id=body.org_id,
        run_id=body.run_id,
        user_id=body.user_id,
        agent_id=body.agent_id,
        props=body.props,
        ts=body.ts,
    )
    return AnalyticsEventOut(event_id=event_id)


@router.get("/events")
async def query_events(
    org_id: str = Query(...),
    event_type: Optional[str] = Query(None),
    since: Optional[datetime] = Query(None),
    until: Optional[datetime] = Query(None),
    limit: int = Query(100, ge=1, le=1000),
) -> List[Dict[str, Any]]:
    rows = await repository.query_analytics_events(
        org_id=org_id,
        event_type=event_type,
        since=since,
        until=until,
        limit=limit,
    )
    for r in rows:
        if isinstance(r.get("event_id"), UUID):
            r["event_id"] = str(r["event_id"])
        if isinstance(r.get("ts"), datetime):
            r["ts"] = r["ts"].isoformat()
    return rows
