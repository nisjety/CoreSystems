"""Domain types for capability-core.

Covers: tool descriptors, plugin manifests, MCP server configs,
routing policies, model configs, memory adapter catalog, and budget tracking.
"""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from typing import Any
from uuid import uuid4

from pydantic import BaseModel, Field


# ── Tool descriptor ─────────────────────────────────────────────

class ToolSource(str, Enum):
    BUILT_IN = "built_in"
    MCP = "mcp"
    PLUGIN = "plugin"
    SKILL = "skill"
    WORKSPACE = "workspace"


class ExecutionTarget(str, Enum):
    LLM_WORKER = "llm_worker"
    WORKSPACE = "workspace"
    AGENT_CONTROL = "agent_control"
    MCP = "mcp"
    DOMAIN_WORKER = "domain_worker"


class ToolDescriptor(BaseModel):
    """Unified tool metadata used across the entire platform."""

    id: str = Field(default_factory=lambda: str(uuid4()))
    name: str
    version: str = "1.0.0"
    description: str = ""
    category: str = "general"
    source: ToolSource = ToolSource.BUILT_IN
    execution_target: ExecutionTarget = ExecutionTarget.LLM_WORKER

    input_schema: dict[str, Any] = Field(default_factory=dict)
    output_schema: dict[str, Any] = Field(default_factory=dict)

    should_defer: bool = False
    always_load: bool = False

    tags: list[str] = Field(default_factory=list)
    search_hint: str = ""

    avg_latency_ms: int = 100
    max_result_size_chars: int = 16_000
    read_only: bool = True
    requires_approval: bool = False
    timeout_ms: int = 30_000

    mcp_server_id: str | None = None
    resolved_tool_name: str | None = None
    plugin_id: str | None = None
    skill_id: str | None = None

    created_at: datetime = Field(default_factory=datetime.utcnow)

    def is_eager(self) -> bool:
        return self.always_load or not self.should_defer


# ── Tool pool ───────────────────────────────────────────────────

class ToolPool(BaseModel):
    session_id: str
    version: str = "1"
    eager: list[ToolDescriptor] = Field(default_factory=list)
    deferred: list[ToolDescriptor] = Field(default_factory=list)
    mcp: list[ToolDescriptor] = Field(default_factory=list)


# ── Permission ──────────────────────────────────────────────────

class PermissionMode(str, Enum):
    DEFAULT = "default"
    ACCEPT_EDITS = "acceptEdits"
    BYPASS = "bypassPermissions"
    DONT_ASK = "dontAsk"
    PLAN = "plan"
    AUTO = "auto"


class PermissionDecision(BaseModel):
    allowed: bool
    reason: str = ""
    requires_user_approval: bool = False
    auto_approved: bool = False


# ── MCP ─────────────────────────────────────────────────────────

class MCPScope(str, Enum):
    USER = "user"
    PROJECT = "project"
    SESSION = "session"
    PLUGIN = "plugin"
    ENTERPRISE = "enterprise"


class MCPTransport(str, Enum):
    STDIO = "stdio"
    STREAMABLE_HTTP = "streamable_http"


class MCPServerConfig(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    server_id: str
    scope: MCPScope = MCPScope.SESSION
    transport: MCPTransport = MCPTransport.STDIO
    command: str = ""
    url: str = ""
    auth_json: dict[str, Any] = Field(default_factory=dict)
    auto_connect: bool = False
    globally_enabled: bool = False
    discovered_tools: list[str] = Field(default_factory=list)
    created_at: datetime = Field(default_factory=datetime.utcnow)


class MCPSessionOverride(BaseModel):
    server_id: str
    session_id: str | None = None
    org_id: str | None = None
    user_id: str | None = None
    enabled: bool = True


# ── Plugin ──────────────────────────────────────────────────────

class PluginManifest(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    plugin_id: str
    version: str = "1.0.0"
    author: str = ""
    description: str = ""
    tools: list[str] = Field(default_factory=list)
    mcp_server_ids: list[str] = Field(default_factory=list)
    enabled: bool = True
    installed_at: datetime = Field(default_factory=datetime.utcnow)
    installed_by_org_id: str = ""


# ── Routing policy ──────────────────────────────────────────────

class LatencyClass(str, Enum):
    REALTIME = "realtime"      # < 500ms
    FAST = "fast"              # < 2s
    BALANCED = "balanced"      # < 5s
    BACKGROUND = "background"  # no limit


class FallbackStrategy(str, Enum):
    SEQUENTIAL = "sequential"
    CHEAPEST = "cheapest"
    LOWEST_LATENCY = "lowest_latency"


class RoutingPolicy(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    org_id: str
    tier: str = "basic"
    latency_class: LatencyClass = LatencyClass.BALANCED
    fallback_strategy: FallbackStrategy = FallbackStrategy.SEQUENTIAL
    provider_priority: list[str] = Field(default_factory=list)
    max_cost_per_request_nok: float = 1.0
    daily_budget_nok: float = 25.0
    monthly_budget_nok: float = 500.0
    feature_requirements: dict[str, bool] = Field(default_factory=dict)
    created_at: datetime = Field(default_factory=datetime.utcnow)


# ── Model config ────────────────────────────────────────────────

class ModelConfig(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    model_id: str
    provider: str
    api_endpoint: str = ""
    capabilities: dict[str, bool] = Field(default_factory=dict)
    cost_per_1k_input_nok: float = 0.0
    cost_per_1k_output_nok: float = 0.0
    context_window: int = 128_000
    enabled: bool = True
    created_at: datetime = Field(default_factory=datetime.utcnow)


# ── Model selection result ──────────────────────────────────────

class ModelSelection(BaseModel):
    model_id: str
    provider: str
    api_endpoint: str = ""
    priority: int = 0
    estimated_cost_nok: float = 0.0


# ── Budget ──────────────────────────────────────────────────────

class BudgetCheckResult(BaseModel):
    allowed: bool
    daily_used_nok: float = 0.0
    monthly_used_nok: float = 0.0
    daily_limit_nok: float = 0.0
    monthly_limit_nok: float = 0.0
    reason: str = ""


class UsageRecord(BaseModel):
    org_id: str
    session_id: str = ""
    cost_nok: float


# ── Memory adapter catalog ──────────────────────────────────────

class MemoryAdapterType(str, Enum):
    LETTA = "letta"
    RETRIEVAL = "retrieval"
    WORKSPACE = "workspace"


class MemoryAdapterEntry(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    adapter_type: MemoryAdapterType
    description: str = ""
    config_schema: dict[str, Any] = Field(default_factory=dict)
    default_policy: dict[str, Any] = Field(default_factory=dict)
    enabled: bool = True


# ── Audit ───────────────────────────────────────────────────────

class AuditEntry(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    event_type: str
    entity_id: str = ""
    actor_id: str = ""
    org_id: str = ""
    payload: dict[str, Any] = Field(default_factory=dict)
    ts: datetime = Field(default_factory=datetime.utcnow)
