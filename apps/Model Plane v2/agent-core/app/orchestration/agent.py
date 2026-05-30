"""PydanticAI agent factory — create configured agents for different run types.

``create_velion_agent()`` builds a PydanticAI ``Agent`` that:
  - Uses ``VelionModel`` (routes LLM calls through llm-worker)
  - Injects ``AgentDeps`` via ``RunContext``
  - Composes system prompts from memory + skills + context
  - Returns typed structured output (``AgentOutput``)

Phase 1.2 scope: structured planning / reasoning output only.
Tool calling via PydanticAI is deferred to Phase 1.4 after Temporal
wiring is complete.

Usage in agent_service.py::

    agent = create_velion_agent(llm_client)
    result = await agent.run(goal, deps=deps)
    print(result.output.answer)
"""

from __future__ import annotations

import logging
from typing import Any

from pydantic import BaseModel, Field
from pydantic_ai import Agent, RunContext

from app.orchestration.deps import AgentDeps
from app.orchestration.velion_model import VelionModel

logger = logging.getLogger(__name__)


# ------------------------------------------------------------------
# Structured output models
# ------------------------------------------------------------------


class PlannedAction(BaseModel):
    """A single planned action in the agent's execution plan."""

    kind: str = Field(description="One of: reasoning, tool_call, control, final_response")
    name: str = Field(description="Action name (tool name or step label)")
    description: str = Field(default="", description="Why this action is needed")
    input: dict[str, Any] = Field(default_factory=dict, description="Action parameters")


class AgentOutput(BaseModel):
    """Structured output from a Velion agent run.

    PydanticAI enforces this schema on the final LLM response,
    automatically retrying if the model produces invalid JSON.
    """

    answer: str = Field(description="The agent's final response to the user goal.")
    reasoning: str | None = Field(
        default=None,
        description="Brief chain-of-thought summary (omitted when trivial).",
    )
    actions: list[PlannedAction] = Field(
        default_factory=list,
        description="Planned actions to execute (for plan-mode runs).",
    )
    confidence: float = Field(
        default=1.0,
        ge=0.0,
        le=1.0,
        description="Self-assessed confidence in the answer.",
    )


# ------------------------------------------------------------------
# Agent factory
# ------------------------------------------------------------------


def create_velion_agent(
    llm_client: Any,
    *,
    model_id: str | None = None,
    provider: str | None = None,
    org_id: str = "system",
    temperature: float | None = None,
) -> Agent[AgentDeps, AgentOutput]:
    """Build a PydanticAI Agent backed by the v2 llm-worker.

    Parameters
    ----------
    llm_client:
        An opened ``LLMClient``.
    model_id / provider:
        Override the default planner model.
    org_id:
        Organisation scope for cost tracking.
    """
    model = VelionModel(
        llm_client,
        model_id=model_id,
        provider=provider,
        org_id=org_id,
        temperature=temperature,
    )

    agent: Agent[AgentDeps, AgentOutput] = Agent(
        model,
        deps_type=AgentDeps,
        output_type=AgentOutput,
        retries=2,
    )

    # ---- Dynamic system prompt from run context ----

    @agent.system_prompt
    async def _build_system_prompt(ctx: RunContext[AgentDeps]) -> str:
        """Compose the system prompt using memory, skills, and run metadata."""
        from app.context.injector import compose_system_prompt
        from app.context.memory import load_memory_files

        run = ctx.deps.run
        if run is None:
            return "You are a helpful AI assistant."

        tool_names = (
            ", ".join(run.loaded_tool_names)
            if run.loaded_tool_names
            else "none loaded"
        )
        base = (
            f"You are an agent (type: {run.agent_type.value}). "
            f"Available tools: [{tool_names}]. "
            f"Max actions: {run.policy.max_actions}. "
            "Complete the user's goal accurately and concisely.\n\n"
            "Respond with a JSON object matching this schema:\n"
            "  answer: string — your final response\n"
            "  reasoning: string | null — chain-of-thought summary\n"
            "  actions: array of {kind, name, description, input} — planned steps\n"
            "  confidence: number 0.0–1.0"
        )

        memory_snippets = await load_memory_files(
            org_id=run.org_id,
            session_id=run.session_id,
        )
        return compose_system_prompt(
            base_prompt=base,
            memory_snippets=memory_snippets,
            session_id=run.session_id or "",
        )

    return agent
