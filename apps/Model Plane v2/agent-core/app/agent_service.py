"""Agent service — core run lifecycle.

Receives commands (via NATS or HTTP) → creates run record →
plans actions via ai-core LLM → executes actions → publishes events.

The execution loop:
1. Create run record in Postgres (status=queued)
2. Acquire lease via Redis
3. Load tool pool from ai-core
4. Plan actions via ai-core LLM
5. For each action:
   a. Check approval mode → maybe block
   b. Execute via adapter (ai-core tool, internal reasoning, control)
   c. Checkpoint to Postgres
   d. Publish event
6. Finalize run (status=completed or failed)
7. Release lease
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any
from uuid import uuid4

from pydantic import ValidationError

from app import repository as repo
from app.capability_client import CapabilityClient
from app.cost_client import CostClient
from app.documents_client import DocumentsClient
from app.llm_client import LLMClient
from app.domain import (
    ActionKind,
    ActionStatus,
    ActionTarget,
    AgentAction,
    AgentType,
    ApprovalMode,
    CreateRunRequest,
    ExecutionPolicy,
    PermissionMode,
    RunMode,
    RunRecord,
    RunResponse,
    RunStatus,
    SessionCommand,
    get_query_depth,
    with_query_depth,
)
from app.context.auto_compact import auto_compact
from app.context.compact import compact_history
from app.context.injector import compose_system_prompt
from app.context.memory import load_memory_files
from app.hooks.domain import HookType
from app.hooks.executor import run_lifecycle_hooks
from app.permissions.domain import PermissionDecision
from app.permissions.evaluator import evaluate_permission
from app.permissions.policy_engine import PolicyEngine
from app.skills.registry import match_skills
from app.nats_publisher import EventPublisher
from app.messages.streaming import EventStream, get_or_create_stream, remove_stream
from app.messages.types import MessageType, build_event
from app.resilience import retry, with_timeout
from app.snapshot import create_snapshot, teleport
from app.trajectory.recorder import TrajectoryRecorder
from app.usage_reporter import UsageReporter
from app.redis_client import (
    acquire_lease,
    cache_run,
    check_idempotency,
    invalidate_run_cache,
    release_lease,
    renew_lease,
)

logger = logging.getLogger(__name__)


class AgentService:
    """Orchestrates agent runs end-to-end."""

    def __init__(
        self,
        capability: CapabilityClient,
        llm: LLMClient,
        documents: DocumentsClient,
        publisher: EventPublisher,
        cost_client: CostClient,
        usage_reporter: UsageReporter | None = None,
    ) -> None:
        self._capability = capability
        self._llm = llm
        self._documents = documents
        self._publisher = publisher
        self._cost_client = cost_client
        self._usage_reporter = usage_reporter
        self._trajectory = TrajectoryRecorder()
        self._worker_id = f"worker-{uuid4().hex[:8]}"

    # ----------------------------------------------------------------
    # Command dispatch (NATS entry point)
    # ----------------------------------------------------------------

    async def handle_command(self, session_id: str, payload: dict[str, Any]) -> None:
        """Dispatch a session command from the NATS command loop."""
        cmd_type = payload.get("command_type", "")

        # Idempotency check
        idem_key = payload.get("idempotency_key")
        if idem_key and await check_idempotency(idem_key):
            logger.info("command_duplicate_skipped", extra={"key": idem_key})
            return

        if cmd_type == "create_run":
            await self._handle_create_run(session_id, payload)
        elif cmd_type == "resume":
            run_id = payload.get("run_id", "")
            user_id = payload.get("user_id", "")
            await self.resume_run(run_id, user_id)
        elif cmd_type == "cancel":
            run_id = payload.get("run_id", "")
            await self.cancel_run(run_id)
        else:
            logger.warning("unknown_command_type", extra={"type": cmd_type})

    async def _handle_create_run(self, session_id: str, payload: dict[str, Any]) -> None:
        """Create and execute a run from a NATS command."""
        request = CreateRunRequest(
            goal=payload.get("goal", ""),
            mode=RunMode(payload.get("mode", "execute")),
            agent_type=AgentType(payload.get("agent_type", "general")),
            policy=ExecutionPolicy(**(payload.get("policy") or {})),
            allowed_tools=payload.get("allowed_tools", []),
            parent_run_id=payload.get("parent_run_id"),
            context=payload.get("context", {}),
        )
        user_id = payload.get("user_id", "")
        org_id = payload.get("org_id")

        run = await self.create_run(request, session_id, user_id, org_id)
        await self.execute_run(run.id)

    # ----------------------------------------------------------------
    # Run lifecycle
    # ----------------------------------------------------------------

    async def create_run(
        self,
        request: CreateRunRequest,
        session_id: str,
        user_id: str,
        org_id: str | None = None,
    ) -> RunRecord:
        """Create a new run record in Postgres."""
        lifecycle_payload = {
            "goal": request.goal,
            "mode": request.mode.value,
            "agent_type": request.agent_type.value,
            "context": dict(request.context),
            "allowed_tools": list(request.allowed_tools),
            "parent_run_id": request.parent_run_id,
            "session_id": session_id,
            "user_id": user_id,
        }
        lifecycle_result = await run_lifecycle_hooks(
            org_id,
            HookType.SESSION_START,
            "session:start",
            lifecycle_payload,
        )
        if not lifecycle_result.proceed:
            raise PermissionError(lifecycle_result.reason or "session start blocked")

        if lifecycle_result.modified_payload is not None:
            merged_payload = {
                **request.model_dump(),
                **lifecycle_result.modified_payload,
            }
            try:
                request = CreateRunRequest.model_validate(merged_payload)
            except ValidationError as exc:
                logger.warning(
                    "session_start_hook_invalid_payload",
                    extra={
                        "session_id": session_id,
                        "org_id": org_id,
                        "error": str(exc),
                    },
                )
                raise PermissionError("invalid session start payload") from exc

        query_depth = 0
        if request.parent_run_id:
            parent_run = await repo.get_run(request.parent_run_id)
            if parent_run is None:
                query_depth = 0
                logger.warning(
                    "parent_run_missing_for_query_depth",
                    extra={"parent_run_id": request.parent_run_id, "session_id": session_id},
                )
            else:
                query_depth = get_query_depth(parent_run.metadata) + 1

        run = RunRecord(
            session_id=session_id,
            user_id=user_id,
            org_id=org_id,
            agent_type=request.agent_type,
            mode=request.mode,
            goal=request.goal,
            policy=request.policy,
            parent_run_id=request.parent_run_id,
            metadata=with_query_depth(request.context, query_depth),
        )

        # Merge allowed_tools into policy
        if request.allowed_tools:
            run.policy = run.policy.model_copy(
                update={"allowed_tools": list(request.allowed_tools)}
            )

        await repo.create_run(run)
        logger.info("run_created", extra={"run_id": run.id, "session_id": session_id})
        return run

    async def execute_run(self, run_id: str) -> RunResponse:
        """Execute a run: plan → execute actions → finalize."""
        run = await repo.get_run(run_id)
        if run is None:
            raise ValueError(f"Run {run_id} not found")

        import time as _time
        _run_started_at = _time.monotonic()

        # Acquire lease
        if not await acquire_lease(run_id, self._worker_id):
            logger.warning("run_lease_contention", extra={"run_id": run_id})
            return RunResponse(run_id=run_id, status=RunStatus.QUEUED)

        try:
            # Transition to running
            run.status = RunStatus.RUNNING
            await repo.update_run_status(run_id, RunStatus.RUNNING, lease_owner=self._worker_id)
            await self._publisher.run_started(run)

            # Check org entitlements before execution
            if run.org_id and run.org_id not in ("system", "dev"):
                from app.permissions.org_gate import check_org_quota

                entitlement = await check_org_quota(run.org_id)
                if not entitlement.allowed:
                    run.status = RunStatus.FAILED
                    run.error = f"Entitlement denied: {entitlement.reason}"
                    await repo.update_run_status(run_id, RunStatus.FAILED, error=run.error)
                    await self._publisher.run_failed(run)
                    return RunResponse(run_id=run_id, status=RunStatus.FAILED, error=run.error)

            # Load tool pool (with MCP tools if manager available)
            mcp_manager = getattr(self, '_mcp_manager', None)
            tool_pool = await self._capability.get_tool_pool(
                run.session_id,
                run.agent_type.value,
                run.policy.allowed_tools or None,
                org_id=run.org_id,
                mcp_manager=mcp_manager,
            )
            run.tool_pool_version = tool_pool.get("version")
            run.loaded_tool_names = tool_pool.get("tool_names", [])

            # --- Temporal durable execution (Phase 1.4) ---
            # When Temporal is enabled and mode is not REACTIVE, delegate
            # the entire plan→execute→finalize lifecycle to a Temporal
            # workflow for crash-proof, replayable execution.
            from app.config import settings

            if (
                settings.temporal_enabled
                and run.mode != RunMode.REACTIVE
                and run.mode != RunMode.PLAN
            ):
                try:
                    return await self._execute_via_temporal(run)
                except Exception:
                    # _execute_via_temporal already logged; fall through
                    # to local execution as graceful degradation.
                    pass

            # --- Reactive turn loop (Phase G) ---
            if run.mode == RunMode.REACTIVE:
                from app.turn_loop import run_turn_loop
                from app.thinking import ThinkingMode

                # Resolve thinking mode from run metadata (request context is
                # stored there during create_run).
                thinking_override = None
                ctx = run.metadata if isinstance(run.metadata, dict) else {}
                if isinstance(ctx, dict):
                    tm = ctx.get("thinking_mode") or ctx.get("thinking")
                    if tm:
                        try:
                            thinking_override = ThinkingMode(tm)
                        except ValueError:
                            pass

                # Create event stream for this run (Phase 8: rich streaming)
                event_stream = get_or_create_stream(run.id, run.session_id)
                await event_stream.emit(build_event(
                    run_id=run.id,
                    session_id=run.session_id,
                    msg_type=MessageType.RUN_STARTED,
                    data={"worker_id": self._worker_id, "mode": "reactive"},
                ))

                loop_result = await run_turn_loop(
                    run=run,
                    llm_client=self._llm,
                    capability_client=self._capability,
                    execute_action_fn=self._execute_action,
                    publisher=self._publisher,
                    policy_engine=PolicyEngine(
                        session_id=run.session_id,
                        run_id=run.id,
                        org_id=run.org_id,
                        policy=run.policy,
                    ),
                    event_stream=event_stream,
                    thinking_override=thinking_override,
                )
                run.actions = loop_result.actions
                run.final_output = loop_result.final_output
                run.status = RunStatus.COMPLETED

                # Phase S+T: persist cost, cache, thinking metadata
                metadata = dict(run.metadata or {})
                if loop_result.cost_summary:
                    metadata["cost_summary"] = loop_result.cost_summary
                if loop_result.cache_stats:
                    metadata["cache_stats"] = loop_result.cache_stats
                if loop_result.thinking_config:
                    metadata["thinking_config"] = loop_result.thinking_config
                metadata["stopped_reason"] = loop_result.stopped_reason
                metadata["consecutive_errors"] = loop_result.consecutive_errors
                metadata["letta_memories_used"] = loop_result.letta_memories_used
                run.metadata = metadata

                await repo.update_run_status(
                    run_id,
                    RunStatus.COMPLETED,
                    final_output=loop_result.final_output,
                    actions=loop_result.actions,
                    lease_owner=None,
                    metadata=metadata,
                )
                await self._publisher.run_completed(run)

                # Phase 3: snapshot for future teleport/resume
                try:
                    await create_snapshot(run, worker_id=self._worker_id)
                except Exception:
                    logger.warning("snapshot_failed", extra={"run_id": run_id})

                # Phase 8: close event stream
                await event_stream.emit(build_event(
                    run_id=run.id,
                    session_id=run.session_id,
                    msg_type=MessageType.RUN_COMPLETED,
                    data=metadata,
                ))
                await event_stream.close()
                remove_stream(run.id)
                # Trajectory — reactive path
                _llm_data = self._llm.last_response_data or {}
                self._trajectory.record_nowait(
                    run,
                    tokens_in=_llm_data.get("tokens_in", 0) or 0,
                    tokens_out=_llm_data.get("tokens_out", 0) or 0,
                    started_at=_run_started_at,
                )
                return RunResponse(
                    run_id=run_id,
                    status=RunStatus.COMPLETED,
                    final_output=loop_result.final_output,
                    actions=loop_result.actions,
                )

            # Plan actions (PydanticAI or legacy planner)
            from app.config import settings

            if settings.pydantic_ai_enabled:
                actions = await self._plan_actions_pydantic(run)
            else:
                actions = await self._plan_actions(run)
            run.actions = actions

            if run.mode == RunMode.PLAN:
                # Plan mode: store plan, await approval
                from app.plan_mode import create_plan_for_run

                plan = await create_plan_for_run(run, actions, self._publisher)
                run.status = RunStatus.PLANNED
                await repo.update_run_status(
                    run_id, RunStatus.PLANNED, actions=actions
                )
                return RunResponse(
                    run_id=run_id,
                    status=RunStatus.PLANNED,
                    actions=actions,
                    plan_id=plan.id,
                )

            # Execute actions sequentially
            for i, action in enumerate(actions):
                if i >= run.policy.max_actions:
                    # Emit quota_exceeded compat event when actions cap is hit
                    await self._publisher.quota_exceeded(
                        run,
                        metric="max_actions",
                        limit=run.policy.max_actions,
                        current=i,
                    )
                    break

                await renew_lease(run_id, self._worker_id)

                # Check approval
                if run.policy.approval_mode == ApprovalMode.PLAN and action.kind == ActionKind.TOOL_CALL:
                    from app.plan_mode import request_approval

                    approval = await request_approval(run, action, self._publisher)
                    run.status = RunStatus.AWAITING_APPROVAL
                    await repo.update_run_status(
                        run_id,
                        RunStatus.AWAITING_APPROVAL,
                        actions=actions,
                        current_action_index=i,
                    )
                    return RunResponse(
                        run_id=run_id,
                        status=RunStatus.AWAITING_APPROVAL,
                        actions=actions,
                        checkpoint_index=i,
                    )

                # Execute action
                action = await self._execute_action(run, action)
                actions[i] = action

                # Checkpoint
                run.current_action_index = i + 1
                run.checkpoint_index = i + 1
                await repo.update_run_status(
                    run_id,
                    RunStatus.RUNNING,
                    actions=actions,
                    current_action_index=i + 1,
                    checkpoint_index=i + 1,
                )
                await invalidate_run_cache(run_id)

            # Finalize with last action's output
            final_output = None
            for action in reversed(actions):
                if action.status == ActionStatus.COMPLETED and action.output:
                    final_output = str(action.output)
                    break

            run.status = RunStatus.COMPLETED
            run.final_output = final_output
            await repo.update_run_status(
                run_id,
                RunStatus.COMPLETED,
                final_output=final_output,
                actions=actions,
                lease_owner=None,
            )
            await self._publisher.run_completed(run)

            # Phase N: extract and store session facts from final output
            if final_output and run.org_id:
                try:
                    from app.context.session_compact import extract_and_store_facts

                    await extract_and_store_facts(
                        org_id=run.org_id,
                        session_id=run.session_id,
                        summary_text=final_output,
                        llm_client=self._llm,
                    )
                except Exception as fact_exc:
                    logger.warning(
                        "fact_extraction_failed",
                        extra={"run_id": run_id, "error": str(fact_exc)},
                    )

            # Trajectory — plan-execute path
            _llm_data_final = self._llm.last_response_data or {}
            self._trajectory.record_nowait(
                run,
                tokens_in=_llm_data_final.get("tokens_in", 0) or 0,
                tokens_out=_llm_data_final.get("tokens_out", 0) or 0,
                started_at=_run_started_at,
            )

            # Letta TrajectorySync — fire-and-forget
            if run.org_id:
                try:
                    from app.config import settings as _s
                    if _s.letta_enabled:
                        from app.letta.org_memory import OrgMemory
                        from app.letta.trajectory_sync import TrajectorySync
                        from app.trajectory.patterns import normalize_goal as _ng
                        asyncio.ensure_future(
                            TrajectorySync(OrgMemory(), self._llm).maybe_sync(
                                run.org_id, _ng(run.goal)
                            )
                        )
                except Exception:
                    pass

            return RunResponse(
                run_id=run_id,
                status=RunStatus.COMPLETED,
                final_output=final_output,
                actions=actions,
                checkpoint_index=run.checkpoint_index,
            )

        except Exception as exc:
            logger.error("run_execution_failed", extra={"run_id": run_id, "error": str(exc)})
            run.status = RunStatus.FAILED
            run.error = str(exc)
            await repo.update_run_status(
                run_id, RunStatus.FAILED, error=str(exc), lease_owner=None
            )
            await self._publisher.run_failed(run)
            # Trajectory — failure path
            self._trajectory.record_nowait(run, started_at=_run_started_at)
            return RunResponse(run_id=run_id, status=RunStatus.FAILED)

        finally:
            await release_lease(run_id, self._worker_id)
            await invalidate_run_cache(run_id)

    async def resume_run(self, run_id: str, user_id: str) -> RunResponse:
        """Resume a run with full session recovery and interruption detection."""
        from app.session_recovery import (
            InterruptionType,
            deserialize_with_interruption_detection,
            restore_cost_state,
        )

        run = await repo.get_run(run_id)
        if run is None:
            raise ValueError(f"Run {run_id} not found")

        # Allow resumption from awaiting_approval OR failed (for recovery)
        if run.status not in (RunStatus.AWAITING_APPROVAL, RunStatus.FAILED, RunStatus.RUNNING):
            raise ValueError(f"Run {run_id} not resumable (status={run.status})")

        # Deserialize previous messages with interruption detection
        prev_messages = getattr(run, 'message_history', []) or []
        if prev_messages:
            messages, interruption = deserialize_with_interruption_detection(prev_messages)
            logger.info(
                "session_recovery",
                extra={
                    "run_id": run_id,
                    "interruption_type": interruption.value,
                    "messages_restored": len(messages),
                },
            )
        else:
            interruption = InterruptionType.NONE

        # Restore cost state from metadata
        metadata = getattr(run, 'metadata', {}) or {}
        cost_state = metadata.get("cost_summary")
        if cost_state:
            restore_cost_state(cost_state)

        # Re-execute from checkpoint
        return await self.execute_run(run_id)

    async def cancel_run(self, run_id: str) -> None:
        """Cancel a running or queued run."""
        await repo.update_run_status(run_id, RunStatus.CANCELLED, lease_owner=None)
        await release_lease(run_id, self._worker_id)
        logger.info("run_cancelled", extra={"run_id": run_id})

    # ----------------------------------------------------------------
    # Temporal durable execution (Phase 1.4)
    # ----------------------------------------------------------------

    async def _execute_via_temporal(self, run: RunRecord) -> RunResponse:
        """Delegate run execution to a Temporal workflow.

        The workflow owns the full plan→execute→checkpoint→finalize
        lifecycle, so we release the local lease and return immediately.
        """
        from app.config import settings
        from app.workflows.agent_workflow import RunWorkflowInput
        from app.workflows.worker import get_temporal_client

        try:
            client = await get_temporal_client()
            workflow_input = RunWorkflowInput(
                run_id=run.id,
                goal=run.goal,
                agent_type=run.agent_type.value,
                org_id=run.org_id or "",
                session_id=run.session_id,
                user_id=run.user_id or "",
                loaded_tool_names=run.loaded_tool_names or [],
                max_actions=run.policy.max_actions if run.policy else 20,
                approval_mode=(
                    run.policy.approval_mode.value
                    if run.policy and run.policy.approval_mode
                    else "none"
                ),
            )

            handle = await client.start_workflow(
                "AgentRunWorkflow",
                workflow_input,
                id=f"agent-run-{run.id}",
                task_queue=settings.temporal_task_queue,
            )
            logger.info(
                "temporal_workflow_started",
                extra={"run_id": run.id, "workflow_id": handle.id},
            )

            # Release local lease — Temporal owns execution now
            await release_lease(run.id, self._worker_id)
            return RunResponse(run_id=run.id, status=RunStatus.RUNNING)

        except Exception:
            logger.exception(
                "temporal_dispatch_failed, falling back to local execution",
                extra={"run_id": run.id},
            )
            # Fall through — caller continues with the non-Temporal path
            raise

    # ----------------------------------------------------------------
    # Planning
    # ----------------------------------------------------------------

    async def _plan_actions(self, run: RunRecord) -> list[AgentAction]:
        """Use ai-core LLM to plan actions for a run."""
        system_prompt = self._build_planner_prompt(run)

        # Phase B (Letta): inject org long-term memory context
        if run.org_id:
            try:
                from app.config import settings as _s
                if _s.letta_enabled:
                    from app.letta.org_memory import OrgMemory
                    _org_mem = OrgMemory()
                    letta_snippets = await _org_mem.search_context(run.org_id, run.goal)
                    if letta_snippets:
                        letta_block = "\n".join(f"- {s}" for s in letta_snippets)
                        system_prompt = (
                            f"<org_memory>\n{letta_block}\n</org_memory>\n\n{system_prompt}"
                        )
            except Exception as _letta_exc:
                logger.debug("letta_context_inject_failed", extra={"error": str(_letta_exc)})

        # Phase H: match and inject relevant skills
        if run.org_id:
            matched_skills = await match_skills(
                org_id=run.org_id,
                goal=run.goal,
            )
            if matched_skills:
                skills_block = "\n\n".join(
                    f"<skill name=\"{s.name}\">\n{s.content}\n</skill>"
                    for s in matched_skills[:5]  # max 5 skills
                )
                system_prompt = f"<skills>\n{skills_block}\n</skills>\n\n{system_prompt}"

        # Phase F: inject org/session memory into system prompt
        memory_snippets = await load_memory_files(
            org_id=run.org_id,
            session_id=run.session_id,
        )
        system_prompt = compose_system_prompt(
            base_prompt=system_prompt,
            memory_snippets=memory_snippets,
            session_id=run.session_id or "",
        )

        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": run.goal},
        ]

        # Phase F: compact history if approaching token budget
        messages = compact_history(messages)

        raw = await self._llm.planner_complete(messages)

        # Report LLM usage to billing-core
        if self._usage_reporter is not None and run.org_id:
            llm_data = self._llm.last_response_data or {}
            await self._usage_reporter.report_llm_usage(
                org_id=run.org_id,
                model=llm_data.get("model", ""),
                input_tokens=llm_data.get("tokens_in", 0) or 0,
                output_tokens=llm_data.get("tokens_out", 0) or 0,
                run_id=run.id,
                session_id=run.session_id,
            )
        # v1 compat: usage.recorded
        if run.org_id:
            llm_data = self._llm.last_response_data or {}
            total_tokens = (llm_data.get("tokens_in", 0) or 0) + (llm_data.get("tokens_out", 0) or 0)
            await self._publisher.usage_recorded(
                run, "llm.tokens", total_tokens,
                metadata={"model": llm_data.get("model", "")},
            )

        # Parse LLM output into actions
        actions = self._parse_plan(raw, run)

        # v1 compat: decision.made (one event per planned action)
        for act in actions:
            await self._publisher.decision_made(
                run,
                decision_type=act.kind.value,
                decision_value=act.name,
            )

        return actions

    async def _plan_actions_pydantic(self, run: RunRecord) -> list[AgentAction]:
        """Plan actions using PydanticAI structured output (Phase 1.2).

        Replaces the raw JSON parsing in ``_plan_actions`` with PydanticAI's
        typed ``Agent.run()`` + automatic retry on schema violations.
        """
        from app.orchestration import AgentDeps, create_velion_agent

        agent = create_velion_agent(
            self._llm,
            org_id=run.org_id or "system",
        )

        deps = AgentDeps(
            llm=self._llm,
            capability=self._capability,
            documents=self._documents,
            publisher=self._publisher,
            cost_client=self._cost_client,
            run=run,
        )

        result = await agent.run(run.goal, deps=deps)

        # Convert AgentOutput.actions → domain AgentAction list
        actions: list[AgentAction] = []
        for pa in result.output.actions:
            try:
                kind = ActionKind(pa.kind)
            except ValueError:
                kind = ActionKind.REASONING
            actions.append(AgentAction(
                kind=kind,
                target=ActionTarget.INTERNAL,
                name=pa.name,
                description=pa.description,
                input=pa.input,
            ))

        # If PydanticAI produced no explicit actions but gave an answer,
        # wrap the answer as a final_response action.
        if not actions and result.output.answer:
            actions.append(AgentAction(
                kind=ActionKind.FINAL_RESPONSE,
                target=ActionTarget.INTERNAL,
                name="final_response",
                description="Direct answer from PydanticAI agent",
                input={"content": result.output.answer},
            ))

        return actions

    def _build_planner_prompt(self, run: RunRecord) -> str:
        tool_names = ", ".join(run.loaded_tool_names) if run.loaded_tool_names else "none loaded"
        return (
            f"You are an agent planner. Agent type: {run.agent_type.value}.\n"
            f"Available tools: [{tool_names}].\n"
            f"Max actions: {run.policy.max_actions}.\n"
            f"Approval mode: {run.policy.approval_mode.value}.\n\n"
            "Given the user's goal, produce a JSON array of actions.\n"
            "Each action: {{\"kind\": \"reasoning|tool_call|control|final_response\", "
            "\"target\": \"internal|ai_core|agent_core\", "
            "\"name\": \"<action_name>\", \"description\": \"<why>\", \"input\": {{...}}}}\n"
            "Respond with ONLY the JSON array."
        )

    def _parse_plan(self, raw: str, run: RunRecord) -> list[AgentAction]:
        """Parse LLM planner output into validated AgentAction list."""
        import json

        # Try to extract JSON array from response
        raw = raw.strip()
        if raw.startswith("```"):
            lines = raw.split("\n")
            raw = "\n".join(lines[1:-1])

        try:
            items = json.loads(raw)
        except json.JSONDecodeError:
            # Fallback: single reasoning action
            return [
                AgentAction(
                    kind=ActionKind.REASONING,
                    target=ActionTarget.INTERNAL,
                    name="reason",
                    description="Process the goal directly",
                    input={"query": run.goal},
                )
            ]

        if not isinstance(items, list):
            items = [items]

        actions: list[AgentAction] = []
        for item in items[: run.policy.max_actions]:
            try:
                actions.append(AgentAction(
                    kind=ActionKind(item.get("kind", "reasoning")),
                    target=ActionTarget(item.get("target", "internal")),
                    name=item.get("name", "unknown"),
                    description=item.get("description"),
                    input=item.get("input", {}),
                ))
            except (ValueError, KeyError):
                continue

        if not actions:
            actions = [
                AgentAction(
                    kind=ActionKind.REASONING,
                    target=ActionTarget.INTERNAL,
                    name="reason",
                    description="Fallback reasoning",
                    input={"query": run.goal},
                )
            ]

        return actions

    # ----------------------------------------------------------------
    # Action execution
    # ----------------------------------------------------------------

    async def _execute_action(self, run: RunRecord, action: AgentAction) -> AgentAction:
        """Execute a single action via the appropriate adapter."""
        action.status = ActionStatus.RUNNING
        await self._publisher.action_started(
            run.id,
            run.session_id,
            action.id,
            action.name,
            query_depth=get_query_depth(run.metadata),
        )

        # Create progress stream (Phase L)
        from app.progress import create_progress_stream

        progress = create_progress_stream(
            publisher=self._publisher,
            run_id=run.id,
            session_id=run.session_id,
            action_id=action.id,
            action_name=action.name,
            query_depth=get_query_depth(run.metadata),
        )

        try:
            # --- Permission evaluation (Phase K) ---
            if action.kind == ActionKind.TOOL_CALL:
                perm_result = await evaluate_permission(
                    session_id=run.session_id,
                    tool_name=action.name,
                    permission_mode=run.policy.permission_mode,
                    allowed_tools=run.policy.allowed_tools or None,
                )
                if perm_result.decision == PermissionDecision.DENY:
                    action.status = ActionStatus.SKIPPED
                    action.error = f"Permission denied: {perm_result.reason}"
                    await self._publisher.action_completed(
                        run.id,
                        run.session_id,
                        action.id,
                        None,
                        action.error,
                        query_depth=get_query_depth(run.metadata),
                    )
                    return action
                elif perm_result.decision == PermissionDecision.ASK_USER:
                    # Publish permission request event for UI
                    from app.domain import AgentEvent

                    await self._publisher.publish(AgentEvent(
                        event_type="permission.requested",
                        run_id=run.id,
                        session_id=run.session_id,
                        payload={
                            "action_id": action.id,
                            "tool_name": action.name,
                            "risk_level": perm_result.risk_level.value,
                            "reason": perm_result.reason,
                            "query_depth": get_query_depth(run.metadata),
                        },
                    ))

            # --- Pre-tool-use hooks (Phase D) ---
            if action.kind == ActionKind.TOOL_CALL:
                from app.hooks.executor import run_pre_hooks

                pre_result = await run_pre_hooks(
                    org_id=run.org_id,
                    tool_name=action.name,
                    tool_input=action.input,
                )
                if pre_result.modified_input is not None:
                    action.input = pre_result.modified_input

            if action.target in (ActionTarget.AI_CORE, ActionTarget.CAPABILITY_CORE) and action.kind == ActionKind.TOOL_CALL:
                result = await self._capability.execute_tool(
                    tool_name=action.name,
                    parameters=action.input,
                    user_id=run.user_id,
                    org_id=run.org_id,
                    session_id=run.session_id,
                    run_id=run.id,
                    action_id=action.id,
                )
                action.output = result
                action.status = ActionStatus.COMPLETED

            elif action.target == ActionTarget.INTERNAL and action.kind == ActionKind.REASONING:
                # Delegate reasoning to llm-worker
                messages = [
                    {"role": "system", "content": "You are a helpful reasoning assistant."},
                    {"role": "user", "content": action.input.get("query", run.goal)},
                ]
                answer = await self._llm.planner_complete(messages)

                # Report usage for inline reasoning turns
                if self._usage_reporter is not None and run.org_id:
                    llm_data = self._llm.last_response_data or {}
                    await self._usage_reporter.report_llm_usage(
                        org_id=run.org_id,
                        model=llm_data.get("model", ""),
                        input_tokens=llm_data.get("tokens_in", 0) or 0,
                        output_tokens=llm_data.get("tokens_out", 0) or 0,
                        run_id=run.id,
                        session_id=run.session_id,
                    )
                # v1 compat: usage.recorded for inline reasoning turns
                if run.org_id:
                    llm_data = self._llm.last_response_data or {}
                    total_tokens = (llm_data.get("tokens_in", 0) or 0) + (llm_data.get("tokens_out", 0) or 0)
                    await self._publisher.usage_recorded(
                        run, "llm.tokens", total_tokens,
                        metadata={"model": llm_data.get("model", "")},
                    )

                action.output = {"answer": answer}
                action.status = ActionStatus.COMPLETED

            elif action.kind == ActionKind.FINAL_RESPONSE:
                # Terminal action — output is the response
                action.output = action.input.get("content", "")
                action.status = ActionStatus.COMPLETED

            elif action.name.startswith("mcp:"):
                # MCP tool dispatch (Phase E)
                from app.mcp.tool_proxy import parse_mcp_tool_name

                parsed = parse_mcp_tool_name(action.name)
                if parsed is None:
                    raise ValueError(f"Invalid MCP tool name: {action.name}")

                server_name, tool_name = parsed
                mcp_manager = getattr(self, '_mcp_manager', None)
                if mcp_manager is None:
                    raise RuntimeError("MCP manager not available")

                client = await mcp_manager.get_client(
                    run.org_id or "", server_name
                )
                if client is None:
                    raise ValueError(
                        f"MCP server '{server_name}' not connected for org"
                    )

                from app.mcp.tool_proxy import build_mcp_tool_action

                mcp_fn = build_mcp_tool_action(server_name, tool_name, client)
                result = await mcp_fn(parameters=action.input)
                action.output = result
                action.status = ActionStatus.COMPLETED

            elif action.kind == ActionKind.CONTROL:
                # Control actions handled by coordinator
                from app.coordinator import handle_control_action

                result = await handle_control_action(run, action, self)
                action.output = result
                action.status = ActionStatus.COMPLETED

            else:
                # Framework adapter dispatch
                from app.adapters import dispatch_to_adapter

                result = await dispatch_to_adapter(action, run)
                action.output = result
                action.status = ActionStatus.COMPLETED

        except Exception as exc:
            # Distinguish hook blocks from generic failures
            from app.hooks.domain import HookBlockedError

            if isinstance(exc, HookBlockedError):
                action.status = ActionStatus.SKIPPED
                action.error = str(exc)
                logger.info(
                    "action_blocked_by_hook",
                    extra={
                        "action_id": action.id,
                        "hook_id": exc.hook_id,
                        "tool": exc.tool_name,
                    },
                )
            else:
                action.status = ActionStatus.FAILED
                action.error = str(exc)
                logger.error(
                    "action_execution_failed",
                    extra={"action_id": action.id, "error": str(exc)},
                )

        # --- Post-tool-use hooks (Phase D) ---
        if action.status == ActionStatus.COMPLETED and action.kind == ActionKind.TOOL_CALL:
            from app.hooks.executor import run_post_hooks

            post_result = await run_post_hooks(
                org_id=run.org_id,
                tool_name=action.name,
                tool_output=action.output,
            )
            if post_result.modified_output is not None:
                action.output = post_result.modified_output

        await self._publisher.action_completed(
            run.id,
            run.session_id,
            action.id,
            action.output,
            action.error,
            query_depth=get_query_depth(run.metadata),
        )
        return action
