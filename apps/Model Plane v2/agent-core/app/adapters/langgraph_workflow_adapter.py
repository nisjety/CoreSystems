"""LangGraph workflow adapter — stateful HITL and multi-step workflows.

Phase 3: wired to real ``langgraph`` running in-process inside agent-core.
State snapshots are pushed into Temporal activities for crash-proof
durability (handled by the caller / workflow layer).

The adapter translates ``AgentAction`` and ``RunRecord`` into the
``WorkflowState`` that the graphs consume, runs the compiled graph,
and returns the result.
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import AgentAction, RunRecord

logger = logging.getLogger(__name__)

_WIRED = True


async def execute(action: AgentAction, run: RunRecord) -> Any:
    """Execute an action via a LangGraph workflow graph.

    Selects the appropriate graph based on ``action.name``:
      - ``multi_step`` → sequential plan-execute-merge
      - ``hitl``       → plan with approval gates
      - ``decompose``  → parallel subtask decomposition

    Returns a dict with ``status``, ``output``, and ``state``.
    """
    from app.graphs import (
        WorkflowState,
        build_decompose_graph,
        build_hitl_graph,
        build_multi_step_graph,
        is_available,
    )

    if not is_available():
        msg = "langgraph package is not installed"
        logger.warning("langgraph_not_available", extra={"run_id": run.id})
        return {"error": msg, "adapter": "langgraph_workflow", "status": "not_available"}

    # Build the initial state from the action and run
    plan_actions = action.input.get("plan", []) if action.input else []
    approval_required = action.input.get("approval_required", False) if action.input else False

    initial_state: WorkflowState = {
        "run_id": run.id,
        "goal": run.goal,
        "org_id": run.org_id or "",
        "session_id": run.session_id,
        "plan": plan_actions,
        "current_step": 0,
        "results": [],
        "approval_required": approval_required,
        "approved": None,
        "error": None,
        "status": "running",
        "subtasks": [],
        "merged_output": "",
    }

    # Select graph
    graph_name = action.name or "multi_step"
    try:
        if graph_name == "hitl":
            graph = build_hitl_graph()
        elif graph_name == "decompose":
            graph = build_decompose_graph()
        else:
            graph = build_multi_step_graph()
    except RuntimeError as exc:
        return {"error": str(exc), "adapter": "langgraph_workflow", "status": "error"}

    # Run the graph
    try:
        final_state = graph.invoke(initial_state)
        logger.info(
            "langgraph_workflow_completed",
            extra={
                "run_id": run.id,
                "graph": graph_name,
                "status": final_state.get("status"),
                "steps": final_state.get("current_step", 0),
            },
        )
        return {
            "status": final_state.get("status", "completed"),
            "output": final_state.get("merged_output", ""),
            "state": dict(final_state),
            "adapter": "langgraph_workflow",
        }
    except Exception:
        logger.exception("langgraph_workflow_failed", extra={"run_id": run.id, "graph": graph_name})
        return {"error": "LangGraph workflow execution failed", "adapter": "langgraph_workflow", "status": "error"}
