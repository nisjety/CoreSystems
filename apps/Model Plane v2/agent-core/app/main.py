"""FastAPI application for agent-core v2.

Startup:  connect Postgres → run migrations → connect NATS → connect Redis
          → wire up AgentService → start NATS command loop
Shutdown: stop command loop → close NATS → close Redis → close Postgres
"""

from __future__ import annotations

import logging
import asyncio
from contextlib import asynccontextmanager
from typing import AsyncGenerator

import uvicorn
from fastapi import FastAPI

from app.capability_client import CapabilityClient
from app.cost_client import CostClient
from app.documents_client import DocumentsClient
from app.llm_client import LLMClient
from app.agent_service import AgentService
from app.api.agents import router as agents_router
from app.api.health import router as health_router
from app.api.orchestration import router as orchestration_router
from app.api.streaming import router as streaming_router
from app.approvals.api import router as approvals_router
from app.context.api import router as memory_router
from app.hooks.api import router as hooks_router
from app.mcp.api import router as mcp_router
from app.mcp.server_manager import MCPServerManager
from app.skills.api import router as skills_router
from app.config import settings
from app.cron.api import router as cron_router
from app.cron.scheduler import start_scheduler, stop_scheduler
from app.database import close_pool, get_pool, run_migrations
from app.messaging.api import router as messaging_router
from app.middleware import InternalAuthMiddleware
from app.nats_client import NatsManager
from app.nats_loop import CommandLoop
from app.nats_publisher import EventPublisher
from app.redis_client import close_redis, get_redis
from app.tasks.api import router as tasks_router
from app.usage_reporter import UsageReporter
from app.policy.org_sync import OrgPlanSyncHandler
from app.control_plane_subscriber import ControlPlaneSubscriber
from app.adapters import langchain_tool_adapter
from app.analytics.publisher import analytics_publisher

# --- New subsystems (Waves 1-4) ---
from app.tools.registry import ToolRegistry
from app.tools.builtins import register_builtins
from app.tools.dispatch import ToolDispatcher
from app.commands.registry import CommandRegistry
from app.commands.builtins.commands import register_all_commands
from app.rate_limits.service import RateLimitService
from app.prompt_cache import PromptCacheManager
from app.coordinator.mode import CoordinatorMode
from app.plugins.registry import PluginRegistry
from app.plugins.loader import PluginLoader
from app.plugins.marketplace import MarketplaceClient
from app.plugins.reconciler import PluginReconciler

# Structured JSON logging (replaces basicConfig)
from app.logging_config import configure_logging
configure_logging()

logger = logging.getLogger(__name__)

# Module-level singletons (populated in lifespan)
nats_mgr: NatsManager | None = None
agent_service: AgentService | None = None  # type: ignore[assignment]
command_loop: CommandLoop | None = None
org_plan_sync: OrgPlanSyncHandler | None = None
cp_subscriber: ControlPlaneSubscriber | None = None


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncGenerator[None, None]:
    """Application lifecycle: startup -> yield -> shutdown."""
    global nats_mgr, agent_service, command_loop, org_plan_sync, cp_subscriber

    logger.info("starting agent-core v2", extra={"port": settings.port})

    # 0. Configure shared reasoning_runtime (direct LLM execution)
    from reasoning_runtime import configure as configure_runtime, RuntimeConfig

    configure_runtime(RuntimeConfig(
        openai_api_key=settings.openai_api_key or "",
        anthropic_api_key=settings.anthropic_api_key or "",
        google_api_key=settings.google_api_key or "",
        redis_url=str(settings.redis_url) if hasattr(settings, "redis_url") else "",
    ))

    # 0b. Initialize OpenTelemetry (no-op if deps not installed)
    from app.telemetry import init_telemetry
    init_telemetry()

    # 1. Postgres
    await get_pool()
    await run_migrations()

    # 1b. Apply new Phase 0/3 schemas (run_events + run_snapshots)
    from app.messages.store import apply_schema as apply_events_schema
    from app.snapshot import apply_schema as apply_snapshots_schema
    await apply_events_schema()
    await apply_snapshots_schema()

    # 2. Redis
    await get_redis()

    # 3. NATS
    nats_mgr = NatsManager()
    await nats_mgr.connect()

    # 4. Wire up AgentService
    capability = CapabilityClient()
    await capability.open()
    llm = LLMClient()
    await llm.open()
    documents = DocumentsClient()
    await documents.open()
    # U2-18 (velion ui-ux-velion-gap.md §10): CostClient reads cost_core_url
    # from settings directly in `open()` — the constructor takes no args.
    # The previous `CostClient(base_url=...)` call crashed the lifespan with
    # `TypeError: unexpected keyword argument 'base_url'`.
    cost_client = CostClient()
    await cost_client.open()
    analytics_publisher.set_cost_client(cost_client)

    publisher = EventPublisher(nats_mgr)

    # 4a. Usage reporter (billing-core integration)
    usage_reporter = UsageReporter(nats_mgr)

    # 4a2. Wire langchain tool adapter to capability-core (replaces NotImplementedError)
    langchain_tool_adapter.wire(capability)

    # 4b. MCP server manager (Phase E)
    mcp_manager = MCPServerManager()

    # 4c. Tool registry + builtins (Phase A2-A3)
    tool_registry = ToolRegistry()
    register_builtins(tool_registry)
    tool_dispatcher = ToolDispatcher(tool_registry)

    # 4d. Slash command registry (Phase C3)
    command_registry = CommandRegistry()
    register_all_commands(command_registry)

    # 4e. Rate limit service (Phase C1)
    rate_limit_service = RateLimitService(
        max_retries=settings.rate_limit_max_retries
    )

    # 4f. Prompt cache manager (Phase C2)
    prompt_cache = PromptCacheManager(
        enabled=not settings.disable_prompt_caching
    )

    # 4g. Coordinator mode (Phase D1)
    coordinator = CoordinatorMode()

    # 4h. Plugin marketplace (Phase C6)
    plugin_registry = PluginRegistry()
    plugin_loader = PluginLoader(plugin_registry)
    marketplace_client = MarketplaceClient(
        catalog_url=settings.plugin_catalog_url
    )
    plugin_reconciler = PluginReconciler(plugin_registry, marketplace_client)

    agent_service = AgentService(
        capability=capability,
        llm=llm,
        documents=documents,
        publisher=publisher,
        usage_reporter=usage_reporter,
        cost_client=cost_client,
    )
    agent_service._mcp_manager = mcp_manager  # inject for MCP dispatch

    # 4i. Register agent service for Temporal activities (Phase 1.3)
    from app.workflows._state import set_agent_service
    set_agent_service(agent_service)

    # 5. Start NATS command loop
    command_loop = CommandLoop(nats_mgr, agent_service.handle_command)
    await command_loop.start()

    # 5b. Start org plan sync (velion-nats → local Postgres)
    org_plan_sync = OrgPlanSyncHandler(nats_mgr)
    await org_plan_sync.start()

    # 5c. Start Control Plane subscriber (velion-nats events: org plan, billing quota)
    cp_subscriber = ControlPlaneSubscriber(
        nats_url=settings.nats_url,
        nats_token=settings.nats_token,
        service_name="mp-v2",
    )
    await cp_subscriber.initialize()

    # 6. Start cron scheduler (CC ScheduleCronTool pattern)
    await start_scheduler(nats_mgr)

    # 6b. Start Temporal worker (Phase 1.3 — durable workflows)
    from app.workflows.worker import start_temporal_worker
    await start_temporal_worker()

    # 7. Run recovery sweep for crashed/stale runs (Phase M)
    try:
        from app.recovery import run_recovery_sweep

        recovery_summary = await run_recovery_sweep(
            worker_id=agent_service._worker_id
        )
        if any(v > 0 for v in recovery_summary.values()):
            logger.info("startup_recovery", extra=recovery_summary)
    except Exception as recovery_exc:
        logger.warning("startup_recovery_failed", extra={"error": str(recovery_exc)})

    # 8. Self-improvement background tasks (Phases A-D)
    _self_improve_tasks: list[asyncio.Task] = []

    if settings.org_insights_interval_minutes > 0:
        from app.analytics.org_insights import OrgInsightsPublisher

        _insights_publisher = OrgInsightsPublisher(nats_mgr)
        interval_secs = settings.org_insights_interval_minutes * 60

        async def _org_insights_loop() -> None:
            while True:
                try:
                    await _insights_publisher.publish_all_active_orgs()
                except Exception as _exc:
                    logger.warning("org_insights_loop_error", extra={"error": str(_exc)})
                await asyncio.sleep(interval_secs)

        _self_improve_tasks.append(
            asyncio.create_task(_org_insights_loop(), name="org-insights")
        )

    if settings.atropos_export_enabled and settings.object_storage_endpoint:
        from app.trajectory.atropos_exporter import export_all_active_orgs

        async def _atropos_midnight_loop() -> None:
            from datetime import datetime, timezone, timedelta

            while True:
                # Sleep until next midnight UTC
                now = datetime.now(timezone.utc)
                tomorrow = (now + timedelta(days=1)).replace(
                    hour=0, minute=0, second=0, microsecond=0
                )
                secs_until_midnight = (tomorrow - now).total_seconds()
                await asyncio.sleep(secs_until_midnight)
                try:
                    await export_all_active_orgs()
                except Exception as _exc:
                    logger.warning("atropos_export_error", extra={"error": str(_exc)})

        _self_improve_tasks.append(
            asyncio.create_task(_atropos_midnight_loop(), name="atropos-exporter")
        )

    # Expose on app.state for request handlers
    app.state.nats_mgr = nats_mgr
    app.state.mcp_manager = mcp_manager
    app.state.tool_registry = tool_registry
    app.state.tool_dispatcher = tool_dispatcher
    app.state.command_registry = command_registry
    app.state.rate_limit_service = rate_limit_service
    app.state.prompt_cache = prompt_cache
    app.state.coordinator = coordinator
    app.state.plugin_registry = plugin_registry
    app.state.plugin_loader = plugin_loader
    app.state.plugin_reconciler = plugin_reconciler
    app.state.cp_subscriber = cp_subscriber  # exposed for quota checks

    # Phase 9: Initialize Letta memory bridge (lazy — connects on first use)
    if settings.letta_enabled:
        from app.letta.memory_bridge import get_memory_bridge
        _bridge = get_memory_bridge()
        app.state.memory_bridge = _bridge
        logger.info("letta_memory_bridge_registered")

    # Phase 5: Register built-in agents for sub-agent dispatch
    from app.builtin_agents import BUILT_IN_AGENTS
    app.state.builtin_agents = BUILT_IN_AGENTS

    logger.info("agent-core v2 ready")
    yield

    # ---- Shutdown ----
    logger.info("shutting down agent-core v2")

    # Cancel self-improvement background tasks
    for _t in _self_improve_tasks:
        _t.cancel()

    # Stop Temporal worker first (Phase 1.3)
    from app.workflows.worker import stop_temporal_worker
    await stop_temporal_worker()

    await stop_scheduler()
    if cp_subscriber:
        await cp_subscriber.close()
    if org_plan_sync:
        await org_plan_sync.stop()
    if command_loop:
        await command_loop.stop()
    if mcp_manager:
        await mcp_manager.disconnect_all()
    if nats_mgr:
        await nats_mgr.close()
    await cost_client.close()
    await documents.close()
    await llm.close()
    await capability.close()
    await close_redis()
    await close_pool()

    from app.middleware.auth import close_auth_client
    await close_auth_client()

    from app.permissions.org_gate import close_entitlement_client
    await close_entitlement_client()

    from app.tools.builtins.knowledge_search import close_knowledge_client
    await close_knowledge_client()

    logger.info("agent-core v2 stopped")


def create_app() -> FastAPI:
    """Factory for the FastAPI application."""
    app = FastAPI(
        title="Agent Core v2",
        version=settings.service_version,
        lifespan=lifespan,
    )
    app.add_middleware(InternalAuthMiddleware)
    app.include_router(health_router)
    app.include_router(agents_router, prefix="/v1")
    app.include_router(streaming_router, prefix="/v1")
    app.include_router(orchestration_router, prefix="/v1")
    app.include_router(tasks_router, prefix="/v1")
    app.include_router(messaging_router, prefix="/v1")
    app.include_router(cron_router, prefix="/v1")
    app.include_router(approvals_router, prefix="/v1")
    app.include_router(hooks_router, prefix="/v1")
    app.include_router(mcp_router, prefix="/v1")
    app.include_router(memory_router, prefix="/v1")
    app.include_router(skills_router, prefix="/v1")
    return app


app = create_app()


if __name__ == "__main__":
    uvicorn.run(
        "app.main:app",
        host=settings.host,
        port=settings.port,
        log_level="debug" if settings.debug else "info",
    )
