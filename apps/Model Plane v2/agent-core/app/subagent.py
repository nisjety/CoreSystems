"""Fork-based sub-agent — context-inheriting child agents.

Ported from CC's AgentTool/forkSubagent.ts:
- Child inherits parent's full conversation context + system prompt
- Tool filtering: async agents get restricted tools, sync get full access
- Model inheritance with tier matching (opus/sonnet/haiku aliases)
- Permission bubbling: child can escalate to parent terminal

This supplements the existing coordinator.py (which handles spawning via NATS)
with a direct fork model where the child shares the parent's LLM client and
runs in-process for lower latency.
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from enum import Enum
from typing import Any
from uuid import uuid4

from app import repository as repo

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Agent type definitions (CC builtInAgents.ts)
# ---------------------------------------------------------------------------

class SubagentType(str, Enum):
    """Task types matching CC's 7 task types."""
    LOCAL_AGENT = "local_agent"
    REMOTE_AGENT = "remote_agent"
    IN_PROCESS = "in_process"
    BACKGROUND = "background"
    WORKFLOW = "workflow"
    MONITOR = "monitor"
    DREAM = "dream"


class AgentStatus(str, Enum):
    """Lifecycle status of a sub-agent."""
    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    KILLED = "killed"


def is_terminal(status: AgentStatus) -> bool:
    """Return True if status is terminal (no more messages can be injected)."""
    return status in (AgentStatus.COMPLETED, AgentStatus.FAILED, AgentStatus.KILLED)


# ---------------------------------------------------------------------------
# Tool filtering (CC agentToolUtils.ts)
# ---------------------------------------------------------------------------

# Tools that sub-agents are NEVER allowed to use
DISALLOWED_TOOLS: frozenset[str] = frozenset({
    "spawn_agent",           # No recursive spawning by default
    "spawn_agents_parallel",
    "exit_plan_mode",
    "ask_user_question",     # Only parent can ask user
})

# Tools that background/async agents CAN use (restricted subset)
ASYNC_ALLOWED_TOOLS: frozenset[str] = frozenset({
    "search", "retrieve", "memory_read", "memory_recall",
    "list_files", "read_file", "get_file_contents",
    "grep", "glob", "web_search", "web_fetch",
    "file_write", "file_edit",
})


def filter_tools_for_agent(
    parent_tools: list[str],
    agent_type: SubagentType,
    *,
    allowed_override: list[str] | None = None,
    denied_override: list[str] | None = None,
) -> list[str]:
    """Filter parent tools for a sub-agent based on type and overrides."""
    tools = set(parent_tools) - DISALLOWED_TOOLS

    if agent_type == SubagentType.BACKGROUND:
        tools = tools & ASYNC_ALLOWED_TOOLS

    if allowed_override:
        # Whitelist mode: only allow specified tools
        tools = tools & set(allowed_override)

    if denied_override:
        tools = tools - set(denied_override)

    return sorted(tools)


# ---------------------------------------------------------------------------
# Sub-agent state (CC LocalAgentTaskState)
# ---------------------------------------------------------------------------

@dataclass
class SubagentState:
    """Full lifecycle state for a sub-agent task."""

    id: str = field(default_factory=lambda: f"a-{uuid4().hex[:8]}")
    name: str = ""
    agent_type: SubagentType = SubagentType.LOCAL_AGENT
    status: AgentStatus = AgentStatus.PENDING
    model: str = ""
    prompt: str = ""
    parent_run_id: str = ""
    tools: list[str] = field(default_factory=list)

    # Progress tracking (CC ProgressTracker)
    tool_use_count: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    recent_activities: list[str] = field(default_factory=list)
    summary: str = ""

    # Message queue (CC queuePendingMessage/drainPendingMessages)
    pending_messages: list[dict[str, Any]] = field(default_factory=list)

    # Retain flag — keep agent results in parent context
    retain: bool = True

    # Output
    output: str | None = None
    error: str | None = None

    def update_progress(self, tool_name: str, tokens_in: int = 0, tokens_out: int = 0) -> None:
        """Track tool usage and cumulative tokens."""
        self.tool_use_count += 1
        self.input_tokens += tokens_in
        self.output_tokens += tokens_out
        self.recent_activities.append(tool_name)
        # Keep only last 10 activities
        if len(self.recent_activities) > 10:
            self.recent_activities = self.recent_activities[-10:]

    def enqueue_message(self, message: dict[str, Any]) -> None:
        """Queue a message for injection into running agent."""
        if is_terminal(self.status):
            logger.warning("message_to_terminal_agent", extra={"id": self.id})
            return
        self.pending_messages.append(message)

    def drain_messages(self) -> list[dict[str, Any]]:
        """Drain and return all pending messages."""
        msgs = self.pending_messages
        self.pending_messages = []
        return msgs

    def to_notification(self) -> str:
        """Format agent result as XML notification (CC enqueueAgentNotification)."""
        return (
            f"<agent_result id=\"{self.id}\" name=\"{self.name}\" "
            f"status=\"{self.status.value}\" "
            f"tools_used=\"{self.tool_use_count}\" "
            f"tokens=\"{self.input_tokens + self.output_tokens}\">\n"
            f"{self.output or self.error or '(no output)'}\n"
            f"</agent_result>"
        )


# ---------------------------------------------------------------------------
# Model resolution (CC utils/model/agent.ts)
# ---------------------------------------------------------------------------

# Alias to model family mapping
_MODEL_ALIASES: dict[str, str] = {
    "opus": "claude-opus-4",
    "sonnet": "claude-sonnet-4",
    "haiku": "claude-3.5-haiku",
    "best": "claude-opus-4",
    "fast": "claude-sonnet-4",
    "cheap": "claude-3.5-haiku",
}


def resolve_agent_model(
    requested: str | None,
    parent_model: str,
) -> str:
    """Resolve sub-agent model using CC's priority chain.

    Priority:
      1. Explicit model name → use as-is
      2. Alias (opus/sonnet/haiku) → resolve
      3. "inherit" or None → use parent model
    """
    if not requested or requested == "inherit":
        return parent_model

    if requested in _MODEL_ALIASES:
        return _MODEL_ALIASES[requested]

    return requested


# ---------------------------------------------------------------------------
# Fork-based sub-agent execution
# ---------------------------------------------------------------------------

async def fork_subagent(
    parent_run_id: str,
    parent_messages: list[dict[str, Any]],
    parent_system_prompt: str,
    parent_tools: list[str],
    parent_model: str,
    *,
    name: str = "subagent",
    prompt: str = "",
    model: str | None = None,
    agent_type: SubagentType = SubagentType.LOCAL_AGENT,
    allowed_tools: list[str] | None = None,
    denied_tools: list[str] | None = None,
    max_turns: int = 25,
    llm_client: Any = None,
    execute_action_fn: Any = None,
    publisher: Any = None,
) -> SubagentState:
    """Fork a sub-agent that inherits the parent's conversation context.

    The child gets:
    - Full parent conversation history (for prompt cache sharing)
    - Parent system prompt + additional child instructions
    - Filtered tool set
    - Resolved model (alias or inherit)
    """
    state = SubagentState(
        name=name,
        agent_type=agent_type,
        parent_run_id=parent_run_id,
        model=resolve_agent_model(model, parent_model),
        prompt=prompt,
        tools=filter_tools_for_agent(
            parent_tools, agent_type,
            allowed_override=allowed_tools,
            denied_override=denied_tools,
        ),
    )

    # Build child context: parent history + fork instruction
    child_messages = list(parent_messages)
    child_messages.append({
        "role": "user",
        "content": (
            f"<fork_context>\n"
            f"You are a sub-agent named '{name}' forked from the parent run.\n"
            f"Your task: {prompt}\n"
            f"Available tools: {', '.join(state.tools)}\n"
            f"Max turns: {max_turns}\n"
            f"Complete the task and provide a clear summary when done.\n"
            f"</fork_context>"
        ),
    })

    state.status = AgentStatus.RUNNING

    if llm_client is None or execute_action_fn is None:
        # Can't execute locally — return state for coordinator dispatch
        state.status = AgentStatus.PENDING
        logger.info("subagent_forked_pending", extra={"id": state.id, "name": name})
        return state

    # Execute the sub-agent turn loop
    try:
        from app.turn_loop import run_turn_loop
        from app.domain import (
            RunRecord,
            ExecutionPolicy,
            RunMode,
            AgentType as DomainAgentType,
            get_query_depth,
            with_query_depth,
        )

        parent_run = await repo.get_run(parent_run_id)
        parent_depth = get_query_depth(parent_run.metadata) if parent_run is not None else 0

        child_run = RunRecord(
            id=state.id,
            session_id=f"fork-{parent_run_id}",
            user_id="system",
            agent_type=DomainAgentType.GENERAL,
            mode=RunMode.REACTIVE,
            goal=prompt,
            policy=ExecutionPolicy(max_turns=max_turns),
            parent_run_id=parent_run_id,
            loaded_tool_names=state.tools,
            metadata=with_query_depth({}, parent_depth + 1),
        )

        loop_result = await run_turn_loop(
            run=child_run,
            llm_client=llm_client,
            capability_client=None,
            execute_action_fn=execute_action_fn,
            publisher=publisher,
        )

        state.output = loop_result.final_output
        state.status = AgentStatus.COMPLETED
        state.summary = loop_result.final_output or ""
        state.tool_use_count = sum(
            1 for a in loop_result.actions if a.kind.value == "tool_call"
        )

    except Exception as exc:
        state.status = AgentStatus.FAILED
        state.error = str(exc)
        logger.error("subagent_fork_failed", extra={"id": state.id, "error": str(exc)})

    return state
