"""Approval API endpoints — request, approve, deny."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, Request

from app.approvals.domain import ApprovalDecision, ApprovalKind, ApprovalRequest
from app.approvals import publisher
from app.domain import ApprovalRecord, ApprovalStatus
from app import repository

router = APIRouter(prefix="/approvals", tags=["approvals"])


@router.post("/request", status_code=201)
async def request_approval(req: ApprovalRequest, request: Request) -> dict:
    """Create an approval record and publish NATS event.

    Called by the run loop when a risky action is detected.
    """
    nats_mgr = request.app.state.nats_mgr

    # Persist in existing approvals table
    record = ApprovalRecord(
        id=req.id,
        session_id=req.session_id,
        run_id=req.run_id,
        action_id=req.action_id,
        action_name=req.action_name,
        reason=req.reason,
        status=ApprovalStatus.PENDING,
    )
    await repository.create_approval(record)

    # Publish to NATS so session-core can notify the user
    await publisher.publish_approval_requested(nats_mgr, req)

    return {"approval_id": req.id, "status": "pending"}


@router.post("/runs/{run_id}/approvals/{approval_id}/approve")
async def approve(run_id: str, approval_id: str, body: ApprovalDecision, request: Request) -> dict:
    """Approve a pending approval and notify the run loop."""
    nats_mgr = request.app.state.nats_mgr

    pending = await repository.get_pending_approvals(run_id)
    match = next((a for a in pending if a.id == approval_id), None)
    if match is None:
        raise HTTPException(status_code=404, detail="pending approval not found")

    await repository.resolve_approval(approval_id, ApprovalStatus.APPROVED, body.decided_by)
    await publisher.publish_approval_resolved(nats_mgr, run_id, approval_id, approved=True)

    return {"approval_id": approval_id, "status": "approved"}


@router.post("/runs/{run_id}/approvals/{approval_id}/deny")
async def deny(run_id: str, approval_id: str, body: ApprovalDecision, request: Request) -> dict:
    """Deny a pending approval and notify the run loop."""
    nats_mgr = request.app.state.nats_mgr

    pending = await repository.get_pending_approvals(run_id)
    match = next((a for a in pending if a.id == approval_id), None)
    if match is None:
        raise HTTPException(status_code=404, detail="pending approval not found")

    await repository.resolve_approval(approval_id, ApprovalStatus.DENIED, body.decided_by)
    await publisher.publish_approval_resolved(nats_mgr, run_id, approval_id, approved=False)

    return {"approval_id": approval_id, "status": "denied"}


@router.get("/runs/{run_id}/pending")
async def list_pending(run_id: str) -> dict:
    """List all pending approvals for a run."""
    pending = await repository.get_pending_approvals(run_id)
    return {"approvals": [a.model_dump() for a in pending]}
