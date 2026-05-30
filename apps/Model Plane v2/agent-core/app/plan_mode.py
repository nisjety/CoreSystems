"""Plan mode — plan-then-execute with approval gating.

When a run is created in PLAN mode:
1. Actions are planned but NOT executed
2. A PlanRecord is stored in Postgres
3. The run status becomes PLANNED
4. User reviews plan and approves/rejects
5. On approval, run transitions to EXECUTE and resumes from checkpoint

When approval_mode=PLAN on individual tool_call actions:
1. An ApprovalRecord is created before execution
2. Run pauses with status=AWAITING_APPROVAL
3. User approves → run resumes; user denies → action skipped
"""

from __future__ import annotations

import logging
from typing import TYPE_CHECKING, Any

from app import repository as repo
from app.domain import (
    AgentAction,
    AgentEvent,
    ApprovalRecord,
    ApprovalStatus,
    PlanRecord,
    PlanStatus,
    RunRecord,
    get_query_depth,
)

if TYPE_CHECKING:
    from app.nats_publisher import EventPublisher

logger = logging.getLogger(__name__)


async def create_plan_for_run(
    run: RunRecord,
    actions: list[AgentAction],
    publisher: EventPublisher,
) -> PlanRecord:
    """Store a plan for review and publish an event."""
    plan = PlanRecord(
        session_id=run.session_id,
        run_id=run.id,
        status=PlanStatus.PENDING,
        steps=[
            {
                "action_id": a.id,
                "kind": a.kind.value,
                "target": a.target.value,
                "name": a.name,
                "description": a.description,
                "input_summary": _summarize_input(a.input),
            }
            for a in actions
        ],
    )
    await repo.create_plan(plan)

    await publisher.publish(
        AgentEvent(
            event_type="plan.created",
            run_id=run.id,
            session_id=run.session_id,
            payload={
                "plan_id": plan.id,
                "step_count": len(plan.steps),
                "steps": plan.steps,
                "query_depth": get_query_depth(run.metadata),
            },
        )
    )

    logger.info(
        "plan_created",
        extra={"run_id": run.id, "plan_id": plan.id, "steps": len(plan.steps)},
    )
    return plan


async def approve_plan(plan_id: str, user_id: str) -> PlanRecord:
    """Approve a pending plan. The run can then be resumed."""
    plan = await repo.get_plan(plan_id)
    if plan is None:
        raise ValueError(f"Plan {plan_id} not found")
    if plan.status != PlanStatus.PENDING:
        raise ValueError(f"Plan {plan_id} is not pending (status={plan.status})")

    await repo.update_plan_status(plan_id, PlanStatus.APPROVED)
    plan.status = PlanStatus.APPROVED

    logger.info("plan_approved", extra={"plan_id": plan_id, "by": user_id})
    return plan


async def reject_plan(plan_id: str, user_id: str) -> PlanRecord:
    """Reject a pending plan. The run should be cancelled."""
    plan = await repo.get_plan(plan_id)
    if plan is None:
        raise ValueError(f"Plan {plan_id} not found")

    await repo.update_plan_status(plan_id, PlanStatus.REJECTED)
    plan.status = PlanStatus.REJECTED

    logger.info("plan_rejected", extra={"plan_id": plan_id, "by": user_id})
    return plan


async def request_approval(
    run: RunRecord,
    action: AgentAction,
    publisher: EventPublisher,
) -> ApprovalRecord:
    """Create an approval request for a specific action."""
    approval = ApprovalRecord(
        session_id=run.session_id,
        run_id=run.id,
        action_id=action.id,
        action_name=action.name,
        reason=f"Tool call '{action.name}' requires approval (approval_mode=plan)",
    )
    await repo.create_approval(approval)

    await publisher.approval_requested(
        run.id, run.session_id, approval.id, action.name, approval.reason
    )

    logger.info(
        "approval_requested",
        extra={"run_id": run.id, "action": action.name, "approval_id": approval.id},
    )
    return approval


async def decide_approval(
    approval_id: str,
    decision: ApprovalStatus,
    user_id: str,
) -> None:
    """Approve or deny a pending approval request."""
    if decision not in (ApprovalStatus.APPROVED, ApprovalStatus.DENIED):
        raise ValueError(f"Invalid decision: {decision}")

    await repo.resolve_approval(approval_id, decision, user_id)
    logger.info(
        "approval_decided",
        extra={"approval_id": approval_id, "decision": decision.value, "by": user_id},
    )


def _summarize_input(inp: dict[str, Any]) -> str:
    """Create a brief summary of action input for plan display."""
    if not inp:
        return ""
    keys = list(inp.keys())[:5]
    return ", ".join(f"{k}=..." for k in keys)
