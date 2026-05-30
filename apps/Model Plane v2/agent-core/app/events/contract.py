"""Session Authority — agent-core event contract.

This module defines the *canonical* payload shapes for every NATS event
published by agent-core v2.  Every payload extends ``BaseEventPayload``
so consumers can rely on a stable set of lineage fields:

    session_id     — stable across the whole user interaction session
    run_id         — unique per agent run (may be a subagent run)
    parent_run_id  — run_id of the spawning run (None = top-level)
    query_depth    — call-stack depth, 0 = user-initiated
    org_id         — tenant identifier

NATS subject
------------
``velion.agent.run.<run_id>.event``

Each event carries ``event_type`` (a dotted string such as ``run.started``)
so a single subscriber can multiplex all run events.

Usage
-----
    from app.events.contract import RunStartedPayload, make_subject

    payload: RunStartedPayload = {
        "session_id":    run.session_id,
        "run_id":        run.run_id,
        "parent_run_id": run.parent_run_id,
        "query_depth":   get_query_depth(run.metadata),
        "org_id":        run.org_id,
        "event_type":    "run.started",
        "ts":            time.time(),
        "agent_id":      run.agent_id,
        "input_summary": run.input[:120] if run.input else "",
    }
    await nats.publish(make_subject(run.run_id), json.dumps(payload).encode())
"""

from __future__ import annotations

from typing import NotRequired, TypedDict

# ── NATS subject ─────────────────────────────────────────────────────────────

SUBJECT_PATTERN = "velion.agent.run.{run_id}.event"


def make_subject(run_id: str) -> str:
    """Return the NATS subject for a specific run."""
    return SUBJECT_PATTERN.format(run_id=run_id)


# ── Base payload ─────────────────────────────────────────────────────────────


class BaseEventPayload(TypedDict):
    """Fields present on every event emitted by agent-core v2.

    These form the lineage spine that Session Authority uses to build the
    full invocation tree for a user session.
    """

    event_type: str     # dotted verb-noun, e.g. "run.started"
    session_id: str     # stable ID that groups all runs in one user interaction
    run_id: str         # this run's unique identifier
    parent_run_id: str | None   # None ⇒ top-level (query_depth == 0)
    query_depth: int    # 0 = user-initiated; +1 per subagent hop
    org_id: str         # tenant / organisation
    ts: float           # Unix timestamp (UTC seconds with fractional part)


# ── Run lifecycle ─────────────────────────────────────────────────────────────


class RunStartedPayload(BaseEventPayload):
    """Emitted immediately when a run is accepted and execution begins."""

    agent_id: str                      # registered agent definition ID
    input_summary: str                 # first 120 chars of user input for quick display
    model: NotRequired[str]            # resolved model name, if known at start
    plan_mode: NotRequired[bool]       # True iff plan-mode approval loop active


class RunCompletedPayload(BaseEventPayload):
    """Emitted when a run finishes, whether successfully or not."""

    status: str                        # "completed" | "failed" | "cancelled"
    output_summary: NotRequired[str]   # first 120 chars of agent output
    error: NotRequired[str]            # error message if status == "failed"
    input_tokens: NotRequired[int]
    output_tokens: NotRequired[int]
    duration_ms: NotRequired[float]


# ── Action lifecycle ──────────────────────────────────────────────────────────


class ActionStartedPayload(BaseEventPayload):
    """Emitted when the agent begins executing a single tool/skill call."""

    action_id: str     # opaque UUID for this action attempt
    action_name: str   # tool or skill name (e.g. "bash", "read_file")
    action_input: NotRequired[str]   # JSON-serialised input (may be truncated)


class ActionProgressPayload(BaseEventPayload):
    """Emitted 0-N times between ActionStarted and ActionCompleted.

    Used for long-running tools (code execution, web browsing) that want to
    stream partial output to the frontend without completing the action.
    """

    action_id: str
    action_name: str
    partial_output: str   # incremental output chunk


class ActionCompletedPayload(BaseEventPayload):
    """Emitted when a tool/skill call returns (success or error)."""

    action_id: str
    action_name: str
    status: str   # "success" | "error"
    output_summary: NotRequired[str]   # first 200 chars of tool output
    error: NotRequired[str]
    duration_ms: NotRequired[float]


# ── Plan mode ────────────────────────────────────────────────────────────────


class PlanCreatedPayload(BaseEventPayload):
    """Emitted when the agent proposes a plan and is awaiting approval."""

    approval_id: str   # links to the approval gate
    plan_markdown: str # full plan text shown to the user


class ApprovalRequestedPayload(BaseEventPayload):
    """Emitted when any approval gate is opened (plan or dangerous action)."""

    approval_id: str
    approval_type: str   # "plan" | "action" | "budget"
    action_name: NotRequired[str]
    prompt: str           # human-readable description of what needs approving


# ── Todo tracking ─────────────────────────────────────────────────────────────


class TodoUpdatedPayload(BaseEventPayload):
    """Emitted whenever the agent's internal todo list changes."""

    todo_id: str
    action: str   # "created" | "started" | "completed" | "skipped"
    content: str  # todo item text
    position: NotRequired[int]   # 0-based index in the list


# ── Sub-agent lifecycle ───────────────────────────────────────────────────────


class SubagentSpawnedPayload(BaseEventPayload):
    """Emitted by the parent run when it delegates work to a subagent.

    The spawned run's ``parent_run_id`` and ``query_depth`` together let
    Session Authority reconstruct the full invocation tree.
    """

    child_run_id: str         # run_id of the newly spawned subagent run
    child_agent_id: str       # agent definition used for the subagent
    delegation_reason: NotRequired[str]   # why the parent delegated


# ── Recovery ─────────────────────────────────────────────────────────────────


class RecoveryAttemptedPayload(BaseEventPayload):
    """Emitted when the recovery loop is triggered for a stalled or failed run."""

    attempt: int    # 1-based recovery attempt number
    strategy: str   # "checkpoint_restore" | "message_truncation" | "full_restart"
    reason: str     # brief description of the failure that triggered recovery


# ── Session terminal ──────────────────────────────────────────────────────────


class SessionTerminatedPayload(BaseEventPayload):
    """Emitted once when a user session ends and all runs are resolved.

    This is the only event where ``run_id`` refers to the root run
    (the user's first query in the session).
    """

    total_runs: int
    total_actions: int
    total_tokens: NotRequired[int]
    duration_ms: NotRequired[float]
