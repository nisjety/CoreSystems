"""LangGraph stateful workflow graphs — Phase 3.

Provides pre-built StateGraph definitions for common agent patterns:
  - ``multi_step_graph``: sequential tool execution with state checkpointing
  - ``hitl_graph``: human-in-the-loop approval at critical steps
  - ``decompose_graph``: task decomposition → parallel subtasks → merge

State is *not* persisted by LangGraph's built-in checkpointer — instead we
push state snapshots into Temporal activities for crash-proof durability.

All graphs use a ``WorkflowState`` TypedDict so they compose cleanly.
"""
from __future__ import annotations

import logging
from typing import Any, Literal, Sequence, TypedDict

logger = logging.getLogger(__name__)

_LANGGRAPH_AVAILABLE = False
try:
    from langgraph.graph import END, StateGraph  # type: ignore[import-untyped]

    _LANGGRAPH_AVAILABLE = True
except ImportError:
    logger.info("langgraph not installed — LangGraph workflows disabled")


def is_available() -> bool:
    return _LANGGRAPH_AVAILABLE


# ── Shared state schema ───────────────────────────────────────────────────────


class WorkflowState(TypedDict, total=False):
    """State flowing through every LangGraph node."""

    run_id: str
    goal: str
    org_id: str
    session_id: str
    plan: list[dict[str, Any]]          # planned actions (dicts from PydanticAI)
    current_step: int
    results: list[dict[str, Any]]       # accumulated action results
    approval_required: bool
    approved: bool | None               # None = pending, True/False = decided
    error: str | None
    status: str                         # running | paused | completed | failed
    subtasks: list[dict[str, Any]]      # for decompose graph
    merged_output: str


# ── Node functions ────────────────────────────────────────────────────────────
# These are pure functions of WorkflowState — side effects happen in the caller
# (the adapter) which passes the real AgentService.


def plan_node(state: WorkflowState) -> WorkflowState:
    """Marker node — the adapter fills state['plan'] before entering the graph."""
    logger.debug("plan_node run_id=%s steps=%d", state.get("run_id"), len(state.get("plan", [])))
    return {**state, "current_step": 0, "status": "running"}


def execute_step_node(state: WorkflowState) -> WorkflowState:
    """Execute the current step and advance the index.

    The *actual* tool call happens in the adapter's callback passed
    via ``config["configurable"]["execute_fn"]``. The graph only
    records the result in state.
    """
    step_idx = state.get("current_step", 0)
    plan = state.get("plan", [])

    if step_idx >= len(plan):
        return {**state, "status": "completed"}

    # The adapter injects the result before this node re-enters
    return {
        **state,
        "current_step": step_idx + 1,
        "status": "running",
    }


def check_approval_node(state: WorkflowState) -> WorkflowState:
    """Gate node — pauses the graph if HITL approval is required."""
    if state.get("approval_required") and state.get("approved") is None:
        return {**state, "status": "paused"}
    return state


def merge_node(state: WorkflowState) -> WorkflowState:
    """Merge subtask results into a single output string."""
    results = state.get("results", [])
    summaries = [r.get("summary", str(r)) for r in results]
    return {**state, "merged_output": "\n---\n".join(summaries), "status": "completed"}


def _should_continue(state: WorkflowState) -> Literal["execute_step", "merge"]:
    """Routing function: more steps to run, or merge results."""
    step_idx = state.get("current_step", 0)
    plan = state.get("plan", [])
    if step_idx < len(plan):
        return "execute_step"
    return "merge"


def _approval_router(state: WorkflowState) -> Literal["execute_step", "__end__"]:
    """Route after approval check: continue or halt."""
    if state.get("status") == "paused":
        return "__end__"  # caller resumes later
    return "execute_step"


# ── Graph factories ───────────────────────────────────────────────────────────


def build_multi_step_graph() -> Any:
    """Graph: plan → execute_step (loop) → merge."""
    if not _LANGGRAPH_AVAILABLE:
        raise RuntimeError("langgraph is not installed")

    graph = StateGraph(WorkflowState)
    graph.add_node("plan", plan_node)
    graph.add_node("execute_step", execute_step_node)
    graph.add_node("merge", merge_node)

    graph.set_entry_point("plan")
    graph.add_edge("plan", "execute_step")
    graph.add_conditional_edges("execute_step", _should_continue)
    graph.add_edge("merge", END)

    return graph.compile()


def build_hitl_graph() -> Any:
    """Graph: plan → check_approval → execute_step (loop) → merge.

    If approval is required at any step, the graph halts at
    ``check_approval`` and the adapter resumes it after receiving
    a Temporal signal.
    """
    if not _LANGGRAPH_AVAILABLE:
        raise RuntimeError("langgraph is not installed")

    graph = StateGraph(WorkflowState)
    graph.add_node("plan", plan_node)
    graph.add_node("check_approval", check_approval_node)
    graph.add_node("execute_step", execute_step_node)
    graph.add_node("merge", merge_node)

    graph.set_entry_point("plan")
    graph.add_edge("plan", "check_approval")
    graph.add_conditional_edges("check_approval", _approval_router)
    graph.add_conditional_edges("execute_step", _should_continue)
    graph.add_edge("merge", END)

    return graph.compile()


def build_decompose_graph() -> Any:
    """Graph: plan (decompose) → execute_step per subtask → merge.

    The decompose graph treats each planned action as an independent
    subtask. The adapter may run them in parallel via the WorkerManager.
    """
    # Structurally identical to multi-step for now — the adapter
    # controls parallelism. The graph ensures state bookkeeping.
    return build_multi_step_graph()
