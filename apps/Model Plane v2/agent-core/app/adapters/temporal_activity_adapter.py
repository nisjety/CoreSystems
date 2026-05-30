"""Temporal activity adapter — dispatch actions as Temporal workflow executions.

Phase 1.3: Wired to the real ``temporalio`` SDK.  When an agent run is
dispatched through this adapter, it starts an ``AgentRunWorkflow`` on the
Temporal server, giving the run durable execution guarantees.
"""

from __future__ import annotations

import logging
from typing import Any

from app.config import settings
from app.domain import AgentAction, RunRecord

logger = logging.getLogger(__name__)

_WIRED = True


async def execute(action: AgentAction, run: RunRecord) -> Any:
    """Start or signal a Temporal workflow for this action.

    If Temporal is disabled, falls back to a clear error message.
    """
    if not settings.temporal_enabled:
        return {
            "error": "Temporal is disabled in configuration",
            "adapter": "temporal_activity",
            "status": "disabled",
        }

    try:
        from app.workflows.worker import get_temporal_client
        from app.workflows.agent_workflow import RunWorkflowInput

        client = await get_temporal_client()
        if client is None:
            return {
                "error": "Could not connect to Temporal server",
                "adapter": "temporal_activity",
                "status": "connection_failed",
            }

        workflow_id = f"agent-run-{run.id}"

        result = await client.execute_workflow(
            "AgentRunWorkflow",
            RunWorkflowInput(
                run_id=run.id,
                goal=run.goal,
                agent_type=run.agent_type.value,
                org_id=run.org_id,
                session_id=run.session_id,
                user_id=run.user_id,
                loaded_tool_names=run.loaded_tool_names or [],
                max_actions=run.policy.max_actions,
                approval_mode=run.policy.approval_mode.value,
            ),
            id=workflow_id,
            task_queue=settings.temporal_task_queue,
        )

        return {
            "adapter": "temporal_activity",
            "status": "completed",
            "workflow_id": workflow_id,
            "result": result.model_dump() if hasattr(result, "model_dump") else result,
        }

    except Exception as exc:
        logger.error(
            "temporal_adapter_error",
            extra={"run_id": run.id, "error": str(exc)},
        )
        return {
            "error": str(exc),
            "adapter": "temporal_activity",
            "status": "error",
        }
