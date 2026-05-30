"""NATS publisher for approval events."""

from __future__ import annotations

import logging

from app.approvals.domain import ApprovalRequest
from app.nats_client import NatsManager

logger = logging.getLogger(__name__)


async def publish_approval_requested(
    nats: NatsManager,
    request: ApprovalRequest,
) -> None:
    """Publish velion.agent.approval.requested so session-core can notify the user."""
    subject = f"velion.agent.approval.requested.{request.session_id}"
    payload = request.model_dump()
    payload["created_at"] = payload["created_at"].isoformat() if hasattr(payload["created_at"], "isoformat") else str(payload["created_at"])
    try:
        await nats.publish_jetstream(subject, payload)
        logger.info(
            "approval_requested_published",
            extra={
                "approval_id": request.id,
                "run_id": request.run_id,
                "action": request.action_name,
            },
        )
    except Exception:
        logger.exception("approval_publish_failed", extra={"approval_id": request.id})
        raise


async def publish_approval_resolved(
    nats: NatsManager,
    run_id: str,
    approval_id: str,
    approved: bool,
) -> None:
    """Publish velion.agent.approval.resolved so the run loop can resume."""
    subject = f"velion.agent.approval.resolved.{run_id}"
    payload = {"approval_id": approval_id, "run_id": run_id, "approved": approved}
    try:
        await nats.publish_jetstream(subject, payload)
        logger.info(
            "approval_resolved_published",
            extra={"approval_id": approval_id, "approved": approved},
        )
    except Exception:
        logger.exception("approval_resolve_publish_failed")
