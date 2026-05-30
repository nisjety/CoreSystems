"""AgentRunWorkflow — durable agent run lifecycle via Temporal.

This workflow orchestrates the plan → execute → checkpoint loop with
Temporal's replay safety guarantees:
  - If a worker crashes mid-action, Temporal replays up to the last
    committed activity result and retries from there.
  - HITL approvals use Temporal signals: the workflow pauses until
    an ``approve`` or ``reject`` signal is received.
  - Progress is queryable at any time via the ``status`` query.

The workflow itself is deterministic — all I/O happens in activities.
"""

from __future__ import annotations

import asyncio
from datetime import timedelta
from typing import Any

from temporalio import workflow

# Activities are imported through the sandbox passthrough so they can
# reference non-deterministic modules (httpx, asyncpg, etc.)
with workflow.unsafe.imports_passed_through():
    from app.workflows.activities import (
        ActionInput,
        ActionOutput,
        CheckpointInput,
        FinalizeInput,
        PlanInput,
        checkpoint_run,
        execute_action,
        finalize_run,
        plan_actions,
    )

from pydantic import BaseModel


class RunWorkflowInput(BaseModel):
    """Input to start an agent run workflow."""

    run_id: str
    goal: str
    agent_type: str = "general"
    org_id: str | None = None
    session_id: str = ""
    user_id: str = ""
    loaded_tool_names: list[str] = []
    max_actions: int = 10
    approval_mode: str = "none"


class RunWorkflowOutput(BaseModel):
    """Output from a completed agent run workflow."""

    run_id: str
    status: str  # "completed" | "failed"
    final_output: str | None = None
    actions_executed: int = 0
    error: str | None = None


@workflow.defn
class AgentRunWorkflow:
    """Temporal workflow wrapping the agent run plan→execute loop."""

    def __init__(self) -> None:
        self._status: str = "running"
        self._current_action: str = ""
        self._actions_completed: int = 0
        self._awaiting_approval: bool = False
        self._approval_granted: bool = False
        self._cancelled: bool = False

    # ------------------------------------------------------------------
    # Main execution
    # ------------------------------------------------------------------

    @workflow.run
    async def run(self, input: RunWorkflowInput) -> RunWorkflowOutput:
        """Execute the full agent run lifecycle."""
        self._status = "planning"

        try:
            # Step 1: Plan actions via LLM
            plan_result = await workflow.execute_activity(
                plan_actions,
                PlanInput(
                    run_id=input.run_id,
                    goal=input.goal,
                    agent_type=input.agent_type,
                    org_id=input.org_id,
                    session_id=input.session_id,
                    loaded_tool_names=input.loaded_tool_names,
                    max_actions=input.max_actions,
                ),
                start_to_close_timeout=timedelta(minutes=2),
                heartbeat_timeout=timedelta(seconds=30),
            )

            if not plan_result:
                return await self._finalize(
                    input.run_id, "completed", "No actions planned."
                )

            # Step 2: Execute each action sequentially
            self._status = "executing"
            final_output: str | None = None

            for i, action_dict in enumerate(plan_result):
                if self._cancelled:
                    return await self._finalize(
                        input.run_id, "failed", error="Cancelled by signal"
                    )

                if i >= input.max_actions:
                    break

                action_name = action_dict.get("name", "unknown")
                action_kind = action_dict.get("kind", "reasoning")
                self._current_action = action_name

                # HITL approval check
                if (
                    input.approval_mode == "plan"
                    and action_kind == "tool_call"
                ):
                    self._awaiting_approval = True
                    self._approval_granted = False
                    self._status = "awaiting_approval"

                    # Wait for approval signal (up to 30 minutes)
                    try:
                        await workflow.wait_condition(
                            lambda: self._approval_granted or self._cancelled,
                            timeout=timedelta(minutes=30),
                        )
                    except asyncio.TimeoutError:
                        return await self._finalize(
                            input.run_id,
                            "failed",
                            error=f"Approval timeout for {action_name}",
                        )

                    self._awaiting_approval = False
                    if self._cancelled:
                        return await self._finalize(
                            input.run_id, "failed", error="Cancelled"
                        )
                    self._status = "executing"

                # Execute the action
                result: ActionOutput = await workflow.execute_activity(
                    execute_action,
                    ActionInput(
                        run_id=input.run_id,
                        session_id=input.session_id,
                        org_id=input.org_id,
                        user_id=input.user_id,
                        action_kind=action_kind,
                        action_name=action_name,
                        action_input=action_dict.get("input", {}),
                        action_id=action_dict.get("id", ""),
                    ),
                    start_to_close_timeout=timedelta(minutes=5),
                    heartbeat_timeout=timedelta(seconds=60),
                    retry_policy=workflow.RetryPolicy(
                        maximum_attempts=2,
                        initial_interval=timedelta(seconds=1),
                    ),
                )

                self._actions_completed = i + 1

                # Track last successful output
                if result.status == "completed" and result.output:
                    final_output = (
                        str(result.output)
                        if not isinstance(result.output, str)
                        else result.output
                    )

                # Checkpoint after each action
                await workflow.execute_activity(
                    checkpoint_run,
                    CheckpointInput(
                        run_id=input.run_id,
                        action_index=i + 1,
                    ),
                    start_to_close_timeout=timedelta(seconds=30),
                )

            # Step 3: Finalize
            return await self._finalize(
                input.run_id, "completed", final_output=final_output
            )

        except Exception as exc:
            return await self._finalize(
                input.run_id, "failed", error=str(exc)
            )

    # ------------------------------------------------------------------
    # Signals
    # ------------------------------------------------------------------

    @workflow.signal
    def approve(self) -> None:
        """Signal to approve the current pending action."""
        self._approval_granted = True

    @workflow.signal
    def reject(self) -> None:
        """Signal to reject and cancel the run."""
        self._cancelled = True

    @workflow.signal
    def cancel(self) -> None:
        """Signal to cancel the run."""
        self._cancelled = True

    # ------------------------------------------------------------------
    # Queries
    # ------------------------------------------------------------------

    @workflow.query
    def status(self) -> dict[str, Any]:
        """Query the current workflow state."""
        return {
            "status": self._status,
            "current_action": self._current_action,
            "actions_completed": self._actions_completed,
            "awaiting_approval": self._awaiting_approval,
        }

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _finalize(
        self,
        run_id: str,
        status: str,
        final_output: str | None = None,
        error: str | None = None,
    ) -> RunWorkflowOutput:
        """Run the finalize activity and return the workflow output."""
        self._status = status

        await workflow.execute_activity(
            finalize_run,
            FinalizeInput(
                run_id=run_id,
                status=status,
                final_output=final_output,
                error=error,
            ),
            start_to_close_timeout=timedelta(seconds=30),
        )

        return RunWorkflowOutput(
            run_id=run_id,
            status=status,
            final_output=final_output,
            actions_executed=self._actions_completed,
            error=error,
        )
