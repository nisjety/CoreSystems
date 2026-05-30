"""Postgres repository for capability-core.

All reads/writes funnel through here so the rest of the app
is storage-agnostic.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime
from typing import Any
from uuid import uuid4

import asyncpg

from app.database import get_pool
from app.domain import (
    MCPScope,
    MCPServerConfig,
    MCPTransport,
    MemoryAdapterEntry,
    MemoryAdapterType,
    ModelConfig,
    PluginManifest,
    RoutingPolicy,
    ToolDescriptor,
    ToolSource,
    ExecutionTarget,
    LatencyClass,
    FallbackStrategy,
)

logger = logging.getLogger(__name__)


# ── Tools ────────────────────────────────────────────────────────

async def list_tools(
    *,
    category: str | None = None,
    source: str | None = None,
    limit: int = 200,
    offset: int = 0,
) -> list[ToolDescriptor]:
    pool = await get_pool()
    clauses: list[str] = []
    args: list[Any] = []
    idx = 1

    if category:
        clauses.append(f"category = ${idx}")
        args.append(category)
        idx += 1
    if source:
        clauses.append(f"source = ${idx}")
        args.append(source)
        idx += 1

    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    args.extend([limit, offset])
    sql = f"""
        SELECT * FROM tools {where}
        ORDER BY name
        LIMIT ${idx} OFFSET ${idx + 1}
    """
    rows = await pool.fetch(sql, *args)
    return [_row_to_tool(r) for r in rows]


async def get_tool(name: str) -> ToolDescriptor | None:
    pool = await get_pool()
    row = await pool.fetchrow("SELECT * FROM tools WHERE name = $1", name)
    return _row_to_tool(row) if row else None


async def upsert_tool(tool: ToolDescriptor) -> ToolDescriptor:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO tools (id, name, version, description, category, source,
            execution_target, input_schema, output_schema, should_defer,
            always_load, tags, search_hint, avg_latency_ms, max_result_size_chars,
            read_only, requires_approval, timeout_ms, mcp_server_id,
            resolved_tool_name, plugin_id, skill_id, created_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)
        ON CONFLICT (name) DO UPDATE SET
            version=EXCLUDED.version, description=EXCLUDED.description,
            category=EXCLUDED.category, source=EXCLUDED.source,
            execution_target=EXCLUDED.execution_target,
            input_schema=EXCLUDED.input_schema, output_schema=EXCLUDED.output_schema,
            should_defer=EXCLUDED.should_defer, always_load=EXCLUDED.always_load,
            tags=EXCLUDED.tags, search_hint=EXCLUDED.search_hint,
            avg_latency_ms=EXCLUDED.avg_latency_ms,
            max_result_size_chars=EXCLUDED.max_result_size_chars,
            read_only=EXCLUDED.read_only, requires_approval=EXCLUDED.requires_approval,
            timeout_ms=EXCLUDED.timeout_ms, mcp_server_id=EXCLUDED.mcp_server_id,
            resolved_tool_name=EXCLUDED.resolved_tool_name,
            plugin_id=EXCLUDED.plugin_id, skill_id=EXCLUDED.skill_id
        """,
        tool.id, tool.name, tool.version, tool.description, tool.category,
        tool.source.value, tool.execution_target.value,
        json.dumps(tool.input_schema), json.dumps(tool.output_schema),
        tool.should_defer, tool.always_load,
        tool.tags, tool.search_hint, tool.avg_latency_ms,
        tool.max_result_size_chars, tool.read_only, tool.requires_approval,
        tool.timeout_ms, tool.mcp_server_id, tool.resolved_tool_name,
        tool.plugin_id, tool.skill_id, tool.created_at,
    )
    return tool


async def delete_tool(name: str) -> bool:
    pool = await get_pool()
    tag = await pool.execute("DELETE FROM tools WHERE name = $1", name)
    return tag == "DELETE 1"


async def search_tools(query: str, *, limit: int = 20) -> list[ToolDescriptor]:
    pool = await get_pool()
    rows = await pool.fetch(
        """
        SELECT *, similarity(name || ' ' || description || ' ' || search_hint, $1) AS score
        FROM tools
        WHERE name || ' ' || description || ' ' || search_hint %> $1
           OR $1 = ANY(tags)
        ORDER BY score DESC
        LIMIT $2
        """,
        query, limit,
    )
    return [_row_to_tool(r) for r in rows]


def _row_to_tool(row: asyncpg.Record) -> ToolDescriptor:
    return ToolDescriptor(
        id=str(row["id"]),
        name=row["name"],
        version=row["version"],
        description=row["description"] or "",
        category=row["category"] or "general",
        source=ToolSource(row["source"]),
        execution_target=ExecutionTarget(row["execution_target"]),
        input_schema=json.loads(row["input_schema"]) if row["input_schema"] else {},
        output_schema=json.loads(row["output_schema"]) if row["output_schema"] else {},
        should_defer=row["should_defer"],
        always_load=row["always_load"],
        tags=row["tags"] or [],
        search_hint=row["search_hint"] or "",
        avg_latency_ms=row["avg_latency_ms"],
        max_result_size_chars=row["max_result_size_chars"],
        read_only=row["read_only"],
        requires_approval=row["requires_approval"],
        timeout_ms=row["timeout_ms"],
        mcp_server_id=row["mcp_server_id"],
        resolved_tool_name=row["resolved_tool_name"],
        plugin_id=row["plugin_id"],
        skill_id=row["skill_id"],
        created_at=row["created_at"],
    )


# ── MCP servers ──────────────────────────────────────────────────

async def list_mcp_servers(
    *, scope: str | None = None, limit: int = 100
) -> list[MCPServerConfig]:
    pool = await get_pool()
    if scope:
        rows = await pool.fetch(
            "SELECT * FROM mcp_server_configs WHERE scope = $1 ORDER BY server_id LIMIT $2",
            scope, limit,
        )
    else:
        rows = await pool.fetch(
            "SELECT * FROM mcp_server_configs ORDER BY server_id LIMIT $1", limit
        )
    return [_row_to_mcp(r) for r in rows]


async def get_mcp_server(server_id: str) -> MCPServerConfig | None:
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM mcp_server_configs WHERE server_id = $1", server_id
    )
    return _row_to_mcp(row) if row else None


async def upsert_mcp_server(cfg: MCPServerConfig) -> MCPServerConfig:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO mcp_server_configs (id, server_id, scope, transport, command, url,
            auth_json, auto_connect, globally_enabled, discovered_tools, created_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        ON CONFLICT (server_id) DO UPDATE SET
            scope=EXCLUDED.scope, transport=EXCLUDED.transport,
            command=EXCLUDED.command, url=EXCLUDED.url,
            auth_json=EXCLUDED.auth_json, auto_connect=EXCLUDED.auto_connect,
            globally_enabled=EXCLUDED.globally_enabled,
            discovered_tools=EXCLUDED.discovered_tools
        """,
        cfg.id, cfg.server_id, cfg.scope.value, cfg.transport.value,
        cfg.command, cfg.url, json.dumps(cfg.auth_json),
        cfg.auto_connect, cfg.globally_enabled,
        cfg.discovered_tools, cfg.created_at,
    )
    return cfg


async def delete_mcp_server(server_id: str) -> bool:
    pool = await get_pool()
    # Clean up overrides first
    await pool.execute(
        "DELETE FROM mcp_session_overrides WHERE server_id = $1", server_id
    )
    tag = await pool.execute(
        "DELETE FROM mcp_server_configs WHERE server_id = $1", server_id
    )
    return tag == "DELETE 1"


async def is_mcp_enabled(
    server_id: str,
    *,
    session_id: str | None = None,
    org_id: str | None = None,
    user_id: str | None = None,
) -> bool:
    """Check if an MCP server is active for the given context.

    Logic: globally_enabled → True.  Otherwise check session overrides.
    """
    cfg = await get_mcp_server(server_id)
    if not cfg:
        return False
    if cfg.globally_enabled:
        return True

    pool = await get_pool()
    row = await pool.fetchrow(
        """
        SELECT enabled FROM mcp_session_overrides
        WHERE server_id = $1
          AND (session_id = $2 OR org_id = $3 OR user_id = $4)
        ORDER BY
            CASE WHEN session_id IS NOT NULL THEN 1
                 WHEN user_id IS NOT NULL THEN 2
                 WHEN org_id IS NOT NULL THEN 3
            END
        LIMIT 1
        """,
        server_id, session_id, org_id, user_id,
    )
    return bool(row and row["enabled"])


async def set_mcp_override(
    server_id: str,
    *,
    session_id: str | None = None,
    org_id: str | None = None,
    user_id: str | None = None,
    enabled: bool = True,
) -> None:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO mcp_session_overrides (server_id, session_id, org_id, user_id, enabled)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (server_id, COALESCE(session_id,''), COALESCE(org_id,''), COALESCE(user_id,''))
        DO UPDATE SET enabled = EXCLUDED.enabled
        """,
        server_id, session_id, org_id, user_id, enabled,
    )


def _row_to_mcp(row: asyncpg.Record) -> MCPServerConfig:
    return MCPServerConfig(
        id=str(row["id"]),
        server_id=row["server_id"],
        scope=MCPScope(row["scope"]),
        transport=MCPTransport(row["transport"]),
        command=row["command"] or "",
        url=row["url"] or "",
        auth_json=json.loads(row["auth_json"]) if row["auth_json"] else {},
        auto_connect=row["auto_connect"],
        globally_enabled=row["globally_enabled"],
        discovered_tools=row["discovered_tools"] or [],
        created_at=row["created_at"],
    )


# ── Plugins ──────────────────────────────────────────────────────

async def list_plugins(*, org_id: str | None = None) -> list[PluginManifest]:
    pool = await get_pool()
    if org_id:
        rows = await pool.fetch(
            "SELECT * FROM plugin_catalog WHERE installed_by_org_id = $1 ORDER BY plugin_id",
            org_id,
        )
    else:
        rows = await pool.fetch("SELECT * FROM plugin_catalog ORDER BY plugin_id")
    return [_row_to_plugin(r) for r in rows]


async def get_plugin(plugin_id: str) -> PluginManifest | None:
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM plugin_catalog WHERE plugin_id = $1", plugin_id
    )
    return _row_to_plugin(row) if row else None


async def upsert_plugin(p: PluginManifest) -> PluginManifest:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO plugin_catalog (id, plugin_id, version, author, description,
            tools, mcp_server_ids, enabled, installed_at, installed_by_org_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (plugin_id) DO UPDATE SET
            version=EXCLUDED.version, author=EXCLUDED.author,
            description=EXCLUDED.description, tools=EXCLUDED.tools,
            mcp_server_ids=EXCLUDED.mcp_server_ids, enabled=EXCLUDED.enabled
        """,
        p.id, p.plugin_id, p.version, p.author, p.description,
        p.tools, p.mcp_server_ids, p.enabled, p.installed_at,
        p.installed_by_org_id,
    )
    return p


async def delete_plugin(plugin_id: str) -> bool:
    pool = await get_pool()
    tag = await pool.execute(
        "DELETE FROM plugin_catalog WHERE plugin_id = $1", plugin_id
    )
    return tag == "DELETE 1"


async def set_plugin_enabled(plugin_id: str, enabled: bool) -> bool:
    pool = await get_pool()
    tag = await pool.execute(
        "UPDATE plugin_catalog SET enabled = $2 WHERE plugin_id = $1",
        plugin_id, enabled,
    )
    return tag == "UPDATE 1"


def _row_to_plugin(row: asyncpg.Record) -> PluginManifest:
    return PluginManifest(
        id=str(row["id"]),
        plugin_id=row["plugin_id"],
        version=row["version"],
        author=row["author"] or "",
        description=row["description"] or "",
        tools=row["tools"] or [],
        mcp_server_ids=row["mcp_server_ids"] or [],
        enabled=row["enabled"],
        installed_at=row["installed_at"],
        installed_by_org_id=row["installed_by_org_id"] or "",
    )


# ── Routing policies ────────────────────────────────────────────

async def get_routing_policy(org_id: str) -> RoutingPolicy | None:
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM routing_policies WHERE org_id = $1", org_id
    )
    return _row_to_policy(row) if row else None


async def upsert_routing_policy(p: RoutingPolicy) -> RoutingPolicy:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO routing_policies (id, org_id, tier, latency_class, fallback_strategy,
            provider_priority, max_cost_per_request_nok, daily_budget_nok,
            monthly_budget_nok, feature_requirements, created_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        ON CONFLICT (org_id) DO UPDATE SET
            tier=EXCLUDED.tier, latency_class=EXCLUDED.latency_class,
            fallback_strategy=EXCLUDED.fallback_strategy,
            provider_priority=EXCLUDED.provider_priority,
            max_cost_per_request_nok=EXCLUDED.max_cost_per_request_nok,
            daily_budget_nok=EXCLUDED.daily_budget_nok,
            monthly_budget_nok=EXCLUDED.monthly_budget_nok,
            feature_requirements=EXCLUDED.feature_requirements
        """,
        p.id, p.org_id, p.tier, p.latency_class.value, p.fallback_strategy.value,
        p.provider_priority, p.max_cost_per_request_nok, p.daily_budget_nok,
        p.monthly_budget_nok, json.dumps(p.feature_requirements), p.created_at,
    )
    return p


def _row_to_policy(row: asyncpg.Record) -> RoutingPolicy:
    return RoutingPolicy(
        id=str(row["id"]),
        org_id=row["org_id"],
        tier=row["tier"],
        latency_class=LatencyClass(row["latency_class"]),
        fallback_strategy=FallbackStrategy(row["fallback_strategy"]),
        provider_priority=row["provider_priority"] or [],
        max_cost_per_request_nok=float(row["max_cost_per_request_nok"]),
        daily_budget_nok=float(row["daily_budget_nok"]),
        monthly_budget_nok=float(row["monthly_budget_nok"]),
        feature_requirements=(
            json.loads(row["feature_requirements"])
            if row["feature_requirements"]
            else {}
        ),
        created_at=row["created_at"],
    )


# ── Model configs ────────────────────────────────────────────────

async def list_models(
    *, provider: str | None = None, enabled: bool | None = None
) -> list[ModelConfig]:
    pool = await get_pool()
    clauses: list[str] = []
    args: list[Any] = []
    idx = 1
    if provider:
        clauses.append(f"provider = ${idx}")
        args.append(provider)
        idx += 1
    if enabled is not None:
        clauses.append(f"enabled = ${idx}")
        args.append(enabled)
        idx += 1
    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    rows = await pool.fetch(
        f"SELECT * FROM model_configs {where} ORDER BY model_id", *args
    )
    return [_row_to_model(r) for r in rows]


async def get_model(model_id: str) -> ModelConfig | None:
    pool = await get_pool()
    row = await pool.fetchrow(
        "SELECT * FROM model_configs WHERE model_id = $1", model_id
    )
    return _row_to_model(row) if row else None


async def upsert_model(m: ModelConfig) -> ModelConfig:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO model_configs (id, model_id, provider, api_endpoint,
            capabilities, cost_per_1k_input_nok, cost_per_1k_output_nok,
            context_window, enabled, created_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (model_id) DO UPDATE SET
            provider=EXCLUDED.provider, api_endpoint=EXCLUDED.api_endpoint,
            capabilities=EXCLUDED.capabilities,
            cost_per_1k_input_nok=EXCLUDED.cost_per_1k_input_nok,
            cost_per_1k_output_nok=EXCLUDED.cost_per_1k_output_nok,
            context_window=EXCLUDED.context_window, enabled=EXCLUDED.enabled
        """,
        m.id, m.model_id, m.provider, m.api_endpoint,
        json.dumps(m.capabilities), m.cost_per_1k_input_nok,
        m.cost_per_1k_output_nok, m.context_window, m.enabled, m.created_at,
    )
    return m


def _row_to_model(row: asyncpg.Record) -> ModelConfig:
    return ModelConfig(
        id=str(row["id"]),
        model_id=row["model_id"],
        provider=row["provider"],
        api_endpoint=row["api_endpoint"] or "",
        capabilities=json.loads(row["capabilities"]) if row["capabilities"] else {},
        cost_per_1k_input_nok=float(row["cost_per_1k_input_nok"]),
        cost_per_1k_output_nok=float(row["cost_per_1k_output_nok"]),
        context_window=row["context_window"],
        enabled=row["enabled"],
        created_at=row["created_at"],
    )


# ── Memory adapters ──────────────────────────────────────────────

async def list_memory_adapters() -> list[MemoryAdapterEntry]:
    pool = await get_pool()
    rows = await pool.fetch("SELECT * FROM memory_adapter_catalog ORDER BY adapter_type")
    return [_row_to_mem(r) for r in rows]


def _row_to_mem(row: asyncpg.Record) -> MemoryAdapterEntry:
    return MemoryAdapterEntry(
        id=str(row["id"]),
        adapter_type=MemoryAdapterType(row["adapter_type"]),
        description=row["description"] or "",
        config_schema=json.loads(row["config_schema"]) if row["config_schema"] else {},
        default_policy=json.loads(row["default_policy"]) if row["default_policy"] else {},
        enabled=row["enabled"],
    )


# ── Audit log ────────────────────────────────────────────────────

async def write_audit(
    event_type: str,
    *,
    entity_id: str = "",
    actor_id: str = "",
    org_id: str = "",
    payload: dict[str, Any] | None = None,
) -> None:
    pool = await get_pool()
    await pool.execute(
        """
        INSERT INTO capability_audit_log (id, event_type, entity_id, actor_id, org_id, payload)
        VALUES ($1, $2, $3, $4, $5, $6)
        """,
        str(uuid4()), event_type, entity_id, actor_id, org_id,
        json.dumps(payload or {}),
    )
