"""Messaging API endpoints — send messages and read inbox."""

from __future__ import annotations

from fastapi import APIRouter, Request
from pydantic import BaseModel

from app.messaging.domain import InboxMessage, MessageKind, SendMessageRequest, AgentMessage
from app.messaging import inbox as inbox_mod
from app.messaging import publisher

router = APIRouter(prefix="/messaging", tags=["messaging"])


class SendResponse(BaseModel):
    message_id: str
    delivered: bool


class InboxResponse(BaseModel):
    messages: list[InboxMessage]


@router.post("/send", response_model=SendResponse)
async def send_message(req: SendMessageRequest, request: Request) -> SendResponse:
    """Send a message to another agent run's mailbox (CC SendMessageTool)."""
    nats_mgr = request.app.state.nats_mgr
    from_agent_id = request.headers.get("x-agent-id", "unknown")
    from_agent_name = request.headers.get("x-agent-name")

    msg = AgentMessage(
        kind=req.kind,
        from_agent_id=from_agent_id,
        from_agent_name=from_agent_name,
        to_run_id=req.target_run_id,
        text=req.text,
        payload=req.payload,
    )
    await publisher.publish_message(nats_mgr, msg)
    return SendResponse(message_id=msg.id, delivered=True)


@router.get("/inbox/{run_id}", response_model=InboxResponse)
async def read_inbox(run_id: str) -> InboxResponse:
    """Drain and return all pending messages for a run."""
    messages = inbox_mod.drain_inbox(run_id)
    return InboxResponse(messages=messages)


@router.post("/inbox/{run_id}/wait", response_model=InboxResponse)
async def wait_inbox(run_id: str, timeout: float = 30.0) -> InboxResponse:
    """Block until a message arrives or timeout."""
    msg = await inbox_mod.wait_for_message(run_id, timeout=min(timeout, 120.0))
    if msg is None:
        return InboxResponse(messages=[])
    return InboxResponse(messages=[msg])
