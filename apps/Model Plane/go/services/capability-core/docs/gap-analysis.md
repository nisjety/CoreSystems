# Model Plane — Gap Analysis

Mirrors the canonical in-code catalog in [`internal/roadmap/data.go`](../internal/roadmap/data.go).
Served live at `GET /api/v1/model-plane/implementation-status`.

Status legend: **Yes** = implemented · **Partial** = partially implemented · **No** = not yet implemented.

---

## Service Checklist

| ID | Name | Category | Status | Owner | Notes |
|----|------|----------|--------|-------|-------|
| model-gateway | Model Gateway | rust | Partial | model-plane/rust | Routes inference requests to providers; streaming and failover in progress. |
| session-core | Session Core | rust | Partial | model-plane/rust | Session lifecycle and state persistence; multi-tenant isolation partial. |
| inference-core | Inference Core | rust | Partial | model-plane/rust | Model execution, streaming token output, OpenAI/Azure embeddings, and env-backed model catalogue; multimodal providers and batching pending. |
| execution-core | Execution Core | rust | No | model-plane/rust | Tool call execution runtime; not yet implemented. |
| orchestrator-core | Orchestrator Core | go | Partial | model-plane/go | Plan/act loop coordination across capabilities; error recovery partial. |
| capability-core | Capability Core | go | Partial | model-plane/go | Capability registry, policy, and status endpoint (this service). |
| sandbox-manager | Sandbox Manager | go | No | model-plane/go | Ephemeral sandbox provisioning for tool execution; not yet implemented. |
| browser-broker | Browser Broker | go | No | model-plane/go | Headless browser session broker for web-grounded agents; not yet implemented. |
| letta-bridge | Letta Bridge | go | No | model-plane/go | Bridge to Letta memory service; not yet implemented. |

## Backend Runtime

| ID | Name | Status | Notes |
|----|------|--------|-------|
| otel-tracing | OpenTelemetry Tracing | Yes | Distributed traces emitted across Go services via otel/metric v1.43.0. |
| grpc-transport | gRPC Transport | Partial | gRPC servers stood up for core services; cross-service TLS pending. |
| http-health-ready | HTTP /healthz & /readyz | Yes | Kubernetes-style health/ready probes on HTTP :8085. |
| policy-engine | Capability Policy Engine | Yes | `internal/policy` Engine.Enforce enforces RBAC over registered capabilities with OTel decisions metric; covered by engine_rbac_test.go. |
| capability-registry | Capability Registry | Yes | `internal/registry` supports ReloadManifest with RWMutex-protected concurrent reads, RBAC-gated Get(id, versionConstraint)/ValidateSkill/CheckPromotion; covered by registry_test.go. |
| streaming-inference | Streaming Inference | Yes | `internal/streaming` codifies SSE event schema (StreamStart/TokenChunk/StreamEnd/StreamError) and BackpressureConfig (Low<High<=Max) invariants; covered by streaming_test.go. Rust model-gateway honors the contract. |
| provider-failover | Provider Failover | Yes | `internal/failover` codifies FailoverConfig with RetryPolicy (MaxAttempts, InitialBackoff<=MaxBackoff, Multiplier>=1), CircuitState lifecycle (Closed/Open/HalfOpen), and ProviderHealth tracking; covered by failover_test.go. |
| tool-sandboxing | Tool Sandboxing | Yes | `internal/sandbox` codifies SandboxPolicy with AllowedSyscalls, ResourceLimits (positive CPU/memory/time bounds), NetworkPolicy (allow/deny lists), and FilesystemPolicy (read/write roots); covered by sandbox_test.go. |

## Product Shell

| ID | Name | Status | Notes |
|----|------|--------|-------|
| implementation-status-api | Implementation Status API | Yes | `GET /api/v1/model-plane/implementation-status` returns this catalog. |
| gap-analysis-doc | Gap Analysis Doc | Yes | `docs/gap-analysis.md` mirrors the in-code catalog. |
| readme-landing | README Landing | Yes | README links to status endpoint and gap analysis doc. |
| admin-dashboard | Admin Dashboard | No | Web UI to browse capability/roadmap status; not yet implemented. |

## Claude Donor Roadmap

Features under evaluation for porting from Claude Code into the Model Plane.

| ID | Name | Status | Notes |
|----|------|--------|-------|
| plan-mode | Plan Mode | No | Structured plan-before-act workflow ported from Claude Code. |
| extended-thinking | Extended Thinking Budget | No | Configurable reasoning-token budget for deep analysis tasks. |
| subagent-orchestration | Sub-agent Orchestration | No | Parallel sub-agents with isolated context windows. |
| skill-packs | Skill Packs | No | Loadable domain skills (SKILL.md) consumed by the orchestrator. |
| memory-scopes | Memory Scopes (user/session/repo) | No | Tiered persistent memory across sessions and workspaces. |
| hooks-pre-post-tool | Pre/Post Tool-use Hooks | No | User-configurable hooks around tool invocations. |
| tool-search | Deferred Tool Search | No | On-demand loading of large tool catalogs by semantic match. |
| todo-list-tool | Structured Todo List Tool | No | Agent-visible todo tracking with status transitions. |

## Model Plane v2 Parity

Capabilities to port from `apps/Model Plane v2` into the current Rust/Go/Python split.

| ID | Name | Category | Status | Owner | Notes |
|----|------|----------|--------|-------|-------|
| ai.embeddings | Provider-backed Embeddings | ai-core | Yes | model-plane/rust | Current Model Plane has `InferenceCore.CreateEmbedding` and `/v1/ai/embeddings`; Data Plane embedding clients now call current inference-core instead of v2 ai-core. |
| ai.model-catalog | Model Catalogue | ai-core | Partial | model-plane/rust | Current Model Plane has `InferenceCore.ListModels` and `/v1/ai/models`; catalogue is env-backed until capability-core owns provider/deployment inventory. |
| ai.images | Image Generation, Analysis, and OCR | ai-core | Partial | model-plane/rust | Current Model Plane routes image generation, image analysis, and OCR through `InferenceCore` with OpenAI/Azure OpenAI image and vision adapters; Azure Document Intelligence read/OCR fallback and real provider smoke coverage remain pending. |
| ai.speech | Speech Synthesis and Transcription | ai-core | Partial | model-plane/rust | Current Model Plane routes TTS/STT through InferenceCore.SynthesizeSpeech/TranscribeSpeech with OpenAI, Azure OpenAI, Azure Speech TTS, /v1/ai/speech, and voice catalogue support; streaming transcription and spoken-language detection remain pending. |
| ai.translation | Translation and Text Language Detection | ai-core | Partial | model-plane/rust | Current Model Plane routes translate, batch translate, language detection, and language catalogue calls through `InferenceCore` with Azure Translator plus Azure/OpenAI LLM fallback; transliteration and production Azure Translator smoke coverage remain pending. |
| ai.document-intelligence | Document Intelligence | ai-core | Partial | model-plane/rust | Current Model Plane routes analyze/layout/forms/receipts/invoices through `InferenceCore.AnalyzeDocument` with an Azure Document Intelligence REST provider while durable document ingest remains in Data Plane; real provider smoke coverage and richer typed invoice/receipt field normalization remain pending. |
| ai.language-analytics | Language Analytics | ai-core | Partial | model-plane/rust | Current Model Plane routes sentiment, entities, key phrases, PII, language detection, and text summary through `InferenceCore.AnalyzeLanguage` with Azure AI Language plus Azure/OpenAI LLM fallback; production smoke coverage and typed result normalization remain pending. |
| ai.realtime-video | Realtime and Video APIs | ai-core | Partial | model-plane/rust | Current Model Plane exposes `InferenceCore.CreateRealtimeSession` plus OpenAI realtime session/model routes, and `CreateVideoGenerationJob`/`GetVideoGenerationJob`/`StreamVideoGenerationContent` plus Azure OpenAI Sora video job/content routes; real provider smoke coverage and durable artifact handoff remain pending. |
| providers.extended | Extended Provider Registry | providers | No | model-plane/rust | v2 included Gemini, Mistral, Cohere, and Ollama adapters; current Model Plane has Anthropic/OpenAI/Azure chat plus OpenAI/Azure embeddings. |
| agent.runtime-controls | Agent Runtime Controls | agent-core | Partial | model-plane/go | v2 snapshots, event history, teams, control-actions, and subagent APIs need mapping onto orchestrator/session/execution services. |
| runtime.hooks-mcp-plugins | Hooks, MCP, Plugins, and Commands | agent-core | Partial | model-plane/go | Current Model Plane has schemas and gateway registries; persistence and real hook/command execution need to move into capability-core. |

---

## Updating This Document

This document is a human-readable mirror of `internal/roadmap/data.go`. When the in-code
catalog changes, update the corresponding row here so both sources agree. Auto-sync
(generating this file from `data.go`) is tracked as `gap-analysis-doc` in Product Shell.
