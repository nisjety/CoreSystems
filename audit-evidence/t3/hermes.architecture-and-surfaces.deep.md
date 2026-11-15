# Hermes Agent: architecture and surfaces (Audit 3, facet "architecture-and-surfaces")

Source: Hermes Agent by Nous Research (MIT, Python plus TypeScript), repo `C:/dev/hermes-agent`, branch main, HEAD `b23600860f82` (2026-10-12), latest tag v0.21.7. Companion file: `hermes.architecture-and-surfaces.features.json` (176 records, 514 evidence references; every path and line range was checked to exist and fall inside the file). Scout inventory used as a starting point and verified where relied on: `hermes.inventory.md`.

## 0. How to read this document

Provenance tags: SRC = read in source or config at HEAD; DOC = in-repo documentation (AGENTS guides, website/docs, SECURITY.md); INF = my inference with the basis stated. REA was not used: no shipped binary was analysed, and nothing was built, installed or run. Paths are relative to `C:/dev/hermes-agent` unless stated.

What was read: all area AGENTS.md guides (root, agent, tools, plugins, gateway, hermes_cli, tui_gateway, web, apps/desktop and its src guide, skills, pm, hermes_platform), SECURITY.md, the developer-guide pages on architecture, gateway, programmatic integration, provider runtime, egress, browser supervisor, relay, plugin LLM access and terminal-environment plugins, plus module headers and the key function bodies for the registry, tool search, MCP client, approvals, secret scope, environments, computer use, Bot Desktop, browser control broker, provider profiles, credential pool, fallback, reasoning effort, ACP, API server, event replay, state schema and the Tauri installer.

What was not read end to end: `run_agent.py`, `cli.py`, `gateway/run.py`, most adapters under `plugins/platforms/`, the TypeScript renderer bodies, the A2A adapter code beyond its manifest, `hermes update` internals beyond the AGENTS and docs description, and most of `website/docs/user-guide`. Claims resting only on docs are tagged DOC and the matching records have confidence medium or notes saying so.

## 1. What Hermes is, in architectural terms

One synchronous Python agent core (`AIAgent`, facade `run_agent.py`, loop in `agent/conversation_loop.py` plus about 33 `turn_*.py` phase modules) sits behind nine entry surfaces: a prompt_toolkit CLI, an Ink TUI, an Electron desktop app, a web dashboard, a 25-platform messaging gateway, an OpenAI-compatible HTTP API, an ACP editor server, cron/kanban workers, and a batch runner (SRC `website/docs/developer-guide/architecture.md`, `agent/AGENTS.md`). The core is extended at the edges: 41 model-provider plugins, 21 bundled platform plugins, about 100 tools, 58 bundled and 155 optional skills, a 570-entry plugin catalog and a 65-entry curated MCP catalog (SRC counts from find and ls).

Scale and discipline: about 2,500 non-test Python files (about 932k lines) and 4,463 TS/TSX files; the repo's own docs claim about 25,000 tests in about 1,250 files while the scout counted 5,695 `test_*.py` files, so test figures should be treated as unverified (DOC vs SRC mismatch). The engineering process is unusually explicit: a per-unit complexity ratchet (CC 20, 300 lines per function, 2,000 per file, units already over may only shrink) with fix text written for the coding agent that trips it, size-capped AGENTS.md guides, and rules that cite incident numbers (SRC `scripts/code_health/config.py`, `AGENTS.md`).

Positioning against the reference set (INF from the records): Hermes is a personal, single-tenant, terminal-and-messaging-first agent with a desktop app. It is strongest on harness engineering (loop resilience, context management, tool disclosure, MCP client robustness, approvals, multi-profile isolation, protocols, packaging) and weakest on what a web chat product emphasises (citations UI, deep research mode, hosted share links, first-class charts, enterprise SSO or admin; the scout found none of these in the tree).

## 2. Architecture map

```
 CLI (prompt_toolkit)  TUI (Ink)  Desktop (Electron)  Dashboard (React + xterm PTY)
        |                  \            |                      |
        |                   \-- tui_gateway JSON-RPC (stdio / WebSocket, Pydantic contracts -> TS + OpenRPC)
 Messaging gateway (25+ adapters, relay)   OpenAI-compatible API server (/v1, /api, /p/{profile})
 ACP server (editors)   cron + kanban workers   batch_runner          A2A / MCP server (outbound)
                                   |
                         AIAgent core (phase pipeline)
        prompt builder (3 tiers, cache-stable)   provider resolver + transports   tool dispatch
                |                    |                                  |
   compression (2 layers)    provider profiles (41)           tools registry (+ tool search, MCP client, plugins)
                                   |                                   |
                         state.db (SQLite WAL, FTS5)         terminal backends (local, docker, ssh, modal, daytona,
                                                            singularity, vercel) + browser stack + computer use
```

Process model (SRC `apps/desktop/src/AGENTS.md`, `gateway/AGENTS.md`, `tui_gateway/AGENTS.md`): Electron supervises a pooled headless `hermes serve --port 0` per (connection, profile); the messaging gateway is spawned detached and survives the app; the TUI is a Node process talking newline-delimited JSON-RPC over stdio to Python; the dashboard hosts the real TUI through a PTY WebSocket. One process may serve many profiles, which is the central design constraint behind most of the isolation machinery in section 4.6.

## 3. Findings by area

### 3.1 Agent runtime (43 records; 40 were carried over from the interrupted first pass and re-verified, 3 added)

The first pass of this audit (recovered from `tmp/hermes-arch-parts/partA.json`, 136 evidence references re-checked) covered category 4 in depth. Highlights worth restating because they are not in the earlier Verevon docs:

- Phase pipeline: the outer loop obeys break/continue/return from small phase modules (preflight, request assembly, API call, error handling, tool round, overflow, truncation, finaliser). The AGENTS.md says the default iteration cap is 500; the code default is `sys.maxsize` with `agent.max_turns: null` (unlimited, because caps caused silent truncation), so the doc is stale (SRC `run_agent.py:241`, `hermes_cli/config_defaults.py:76-83`). A grace call and an optional wall-clock budget exist.
- Resilience ladders: a provider error taxonomy with recovery hints (plugin-overridable), a post-exhaustion wait-and-retry ladder, failed-turn closure that keeps the transcript well formed, stop gates that turn a text answer into another iteration (for example code edited without verification evidence), tool-loop guardrails (repeat, cycle, no-progress) and a path-scoped parallel tool-batch planner with a never-parallel list for interactive tools.
- Context: two compaction layers (gateway hygiene at 85 percent, agent compressor at 50 percent), prune-first then structured summary, failure cooldown ladder, optional micro-compaction that never touches user messages, native provider compaction first with the local compressor as fallback. A per-conversation prompt-cache invariant governs everything: mid-conversation changes ride user messages or tool results, never the system prompt, and compression is the only sanctioned cache break.
- Long-running work: background process registry with notify/watch/heartbeat events, subagent delegation with leaf and orchestrator roles and output schemas, a durable async-delegation ledger with honest unknown outcomes, filesystem checkpoints in one shadow git store, a post-turn background review fork that writes memory and skills, an idle-triggered skill curator that archives and never deletes, a cron scheduler with at-most-once slots, a kanban multi-agent board, persistent goals, loops and heartbeats driven as plain user messages, Mixture of Agents as a virtual provider, a global emergency stop that pauses only new work, and a scripted prelude that plays tool calls before the first model call.
- Added in this pass: Bot Mode (profiles as persistent named agents with a canonical forever-chat, group rooms of 2 to 6 bots with serial rounds capped at 3 and 10 messages, hold/release stop directives), session branching and fork as first-class RPCs, and automation blueprints (one slot schema rendered as dashboard form, slash command and agent seed prompt).

### 3.2 Tools, plugins and extensibility (32 records)

The registry is the cleanest part of the design (SRC `tools/registry.py`): modules self-register at import, discovery uses an AST scan with an mtime-keyed disk cache, handlers return JSON strings (plus a multimodal envelope), dispatch wraps and bounds errors, availability probes (`check_fn`) are TTL-cached per profile, and registrations can live in per-profile overlays with an explicit plugin-override policy. The governing rule is the footprint ladder in the root AGENTS.md: because every tool schema is sent on every call, new capability should arrive as a CLI command plus skill, a service-gated tool, a plugin or a catalog MCP server before it becomes a core tool.

Progressive disclosure is the standout (SRC `tools/tool_search.py`, `tool_search_catalog.py`): deferrable tools are replaced by three bridge tools; the listing degrades full to names to bare under a token budget (min of 4,000 tokens and 5 percent of context); the catalog is stateless because a session-keyed one drifted; search is BM25 with Snowball stemming but admission is gated on the query's rarest token plus a half-coverage floor for long queries, both introduced after measured failures (a model re-searched 216 times). Bridge calls validate arguments against the real tool schema before dispatch.

MCP client (about 30 modules, SRC `tools/mcp_tool*.py`): stdio, Streamable HTTP and SSE on one loop; per-call recovery ladder (stdio respawn, OAuth refresh, session-expired reconnect limited to readOnlyHint tools), circuit breaker with half-open probing that distinguishes "server rejected" from "unreachable", list_changed refresh that preserves the cached tools prefix, keepalive and idle recycling, OSV malware preflight and a managed Node, bounded sampling and elicitation handlers, invisible Unicode tag stripping and reserved `_meta` filtering, and a per-profile connection ledger. A `trust: untrusted` tier asks before any tool lacking `readOnlyHint` runs, fail-closed.

Plugins (SRC `plugins/AGENTS.md`, `hermes_cli/plugins.py`, `hermes_cli/middleware.py`): kinds with ABCs (memory, context engine, model provider, image/video/web/browser backends, terminal environments, cron providers, dashboard auth, platform adapters); a "plugins never touch core" rule; 44 hook names (the scout counted 33; counting quoted names in `VALID_HOOKS` at HEAD gives 44) separated into observers, transforms and vetoes; policy hooks (`pre_tool_call`) fail closed on timeout while other bounded hooks fail open; a four-kind middleware contract (tool_request, tool_execution, llm_request, llm_execution) with schema versions; shell hooks honouring the Claude Code exit-code-2 block convention with first-use consent; an opt-in out-of-process plugin host that proxies tools and hooks over a framed protocol; a behaviour-based compatibility contract (additive payload kwargs, never remove ctx methods, signature-inspected callbacks); and a catalog with mandatory 40-hex SHA pins, a kill list enforced at install, update, enable and load, and provenance recorded by the installer rather than the plugin tree.

Skills follow the agentskills.io layout with test-enforced authoring rules (description at most 60 characters, native tool names in prose, scripts instead of inline parsers) and a trust-tiered Skills Guard whose "dangerous" verdict cannot be forced past for plugins. Interop: an OpenAI-compatible API server (with `/v1/runs`, approvals, steer, stop, idempotency, `/v1/capabilities`, and `/p/{profile}` mirrors of every route), an A2A client and server, an MCP server exposing messaging, ACP for editors, and a Codex bridge in both directions.

### 3.3 Computer and environment control (16 records)

- Terminal backends (SRC `tools/environments/`): local, Docker, Singularity, SSH, Modal (direct and Nous-managed), Daytona and Vercel Sandbox behind one spawn-per-call contract (a session snapshot of env vars and functions re-sourced before each command, cwd carried by in-band markers), a `ProcessHandle` protocol so SDK-only backends are adapted by a threaded handle, a file-sync manager for remote backends, and third-party backends registered as plugins with capability flags that replace hardcoded name checks. The README says seven backends; the code has eight modules because `managed_modal` is a managed variant.
- Browser stack: accessibility-tree snapshots with `@eN` refs over local Chromium, cloud providers, Camofox, Lightpanda (with Chrome fallback) or the user's CDP endpoint; a persistent CDP supervisor per task that surfaces native dialogs (with an XHR-based dialog bridge for Browserbase, which auto-dismisses dialogs in about 10 ms) and cross-origin iframes; a ticketed browser-extension controller protocol for driving the user's own browser (single-use 30 s tickets, exact capability allowlist, developer-only raw CDP gated behind an explicit mode, routing that never falls back to another browser); and an opt-in real-profile mode that copies the user's Chromium profile (a high-risk pattern, see section 5).
- Computer use: one `computer_use` tool with SOM (numbered overlay plus AX tree), vision and AX-only capture modes, hard-blocked key combinations, per-action approval and a byte-frozen schema for cache parity, driven through an MCP stdio `cua-driver` on macOS, Windows and Linux. Bot Desktop gives each profile a headless Xfce desktop over RFB with a human-takeover lease stored on disk (epoch-stamped), a byte-level RFB input gate for non-holders, sandbox-aware placement that refuses to run a desktop outside the user's chosen sandbox, and a memory preflight.
- Editing safety: write-guard stack (sensitive paths, protected instruction files that always ask, read-baseline requirement before whole-file overwrite), read dedup that survives compression, a fuzzy edit matcher with Unicode normalisation and conservative escape-drift correction, and LSP diagnostics delta (only errors introduced by the edit) appended to edit results.

### 3.4 Models and routing (22 records)

Providers are declarative profiles (SRC `providers/base.py`): auth type, endpoints, vision quirks, prompt-cache-key opt-in, native reasoning carrier type, external-process launch (ACP) and optional hooks for auth, credential refresh and error classification. Four transports normalise responses (chat completions, Codex Responses, Anthropic Messages, Bedrock) plus native Gemini, Vertex, Azure identity and Copilot adapters. Notable mechanisms: one runtime resolver in which the saved selection beats stale environment variables; credential pools (round-robin, least-used, fill-first, random) with model-scoped cooldowns; a cross-provider fallback chain that benches the primary until the provider-declared reset and can skip switching when the reset is near; a canonical effort ladder clamped to each provider's wire vocabulary (nearest weaker level); provider-native reasoning replay carriers and durable suppression of signature-rejected Anthropic thinking; per-provider tool-schema sanitisers (Gemini, Moonshot); per-task auxiliary model routing with attempt-level hooks; a structured-output capability memo; models.dev with stale-while-revalidate; cost and "trains on your data" confirmation guards; bounded fast-mode windows; native-versus-describe image routing with an image history budget; a managed llama.cpp runtime with a measured context-window ladder; Nous Portal with a Tool Gateway and anonymous free tier; the Codex app-server runtime with Hermes tools exposed back over MCP; and a model-only subscription proxy. Prompt-cache scope is derived from the compression lineage root so rotation keeps cache hits.

### 3.5 Safety, permissions and governance (26 records)

The governing stance is explicit and unusual (SRC `SECURITY.md`): the only boundary against an adversarial model is the operating system; approval gate, redaction and scanners are heuristics; supported postures are a terminal-backend sandbox (which does not confine MCP subprocesses, plugins, hooks or code execution) and whole-process wrapping (Docker or NVIDIA OpenShell). Network surfaces must refuse work until an allowlist exists, session ids are routing handles not authorisation, and callers inside the allowed set are equally trusted.

Controls (SRC): an approval stack with an always-on hardline floor and user deny globs evaluated before yolo, a 1,572-line detector with de-obfuscation, a smart guardian LLM with comment stripping and a consecutive-denial breaker, deny-by-default headless modes (cron, single query, unattended webhook/API surfaces), yolo frozen at import, protected instruction files that always ask, staged write approval for memory and skill writes, credential scoping with a provider-key blocklist and one environment builder, a fail-closed profile-scoped secret scope (no `os.environ` fallthrough), exact-value vault redaction, model-blind browser autofill bound to the page origin, a credential-injecting egress proxy for Docker sandboxes, SSRF protection that closes DNS rebinding at connect time and re-validates redirects, tiered prompt-injection patterns, self-protection floors (own interpreter, own checkout, own supervisor), DM pairing with lockout, single-use WebSocket tickets, administrator-pinned managed scope, and opt-in telemetry with separate collect and send consents and consent windows.

### 3.6 Platform architecture (27 records)

- Contracts and transport: every JSON-RPC method, server-to-client request and event is one Pydantic model (extra=forbid), registered at import, generated into TypeScript and OpenRPC with a stale-file test; the dispatcher rejects unknown params and raises on contract violations in test isolation. Server-to-client requests (approval, clarify, sudo, secret, vault, connection cards, desktop read/act bridges) block the agent thread until answered, with `request.cancel`, replay of open requests on resume and a `client.capabilities` handshake. Events carry a per-session sequence number with an epoch and a truncation watermark in a bounded replay ring; resume returns the in-flight turn; rewind/edit/regenerate is a confirmed truncating `prompt.submit` with durable row ids, a busy refusal and survivor ids.
- Multiplexing: one process serves many profiles with home, secret scope and terminal scope bound together per activity (contextvars), an immutable routing identity per inbound event, blocking CI rules for the call-site shapes that leaked profile state, and a host-wide singleton rendezvous with a control socket for proven liveness.
- State: SQLite per profile (schema version 31) with WAL read pool, FTS5 including a loadable CJK tokenizer, compression lineage, lock-holder diagnostics, repair and portability, and a test guard that refuses to open a live database.
- Delivery: transactional update pipeline with fleet-wide drain, a frozen compatibility surface for old updaters, release channels with a validated wire protocol, a Tauri bootstrap installer (the only Rust in the repo, a port of the Electron bootstrap runner with stage retries), MSIX/App Installer, Termux APT repo, Docker with pinned SQLite and digest-pinned bases, and a Nix flake.

### 3.7 UI surfaces (6 records, plus the TUI/desktop/dashboard material above)

The desktop app is explicitly its own chat surface (React, nanostores, assistant-ui primitives, a plugin SDK where core registers through the same API), governed by a 34 KB design contract (flat surfaces, one primitive per concern, no background focus stealing, distinct empty/loading/reconnecting/degraded states, one cancel gesture does one thing). State is placed by authority (backend for anything another surface can change, Electron for machine facts, renderer for presentation) and persisted renderer state declares its scope in its key. The dashboard deliberately embeds the real TUI over a PTY rather than re-implementing chat. Slash-command desktop disposition is authored once on the command definition. i18n resolves plugin packs, profile overlays, bundled locales, English, then the key.

## 4. What is best in class (relative to the reference set)

1. Tool disclosure design (budgeted listing tiers, rarest-token gate, argument validation, eval harness).
2. MCP client robustness and trust tiering (recovery ladder, breaker, readOnlyHint gating, supply-chain preflight).
3. Contract-first RPC (Pydantic to TypeScript/OpenRPC, replayable events with epoch, blocking replayable server-to-client requests).
4. Edit/rewind protocol with durable ids and explicit confirmation flags.
5. Multi-profile isolation machinery (fail-closed secret scope, activity-bound scope, routing identity, lint rules) and the honest SECURITY.md.
6. Approval layering (hardline floor before yolo, deny globs, headless defaults, staged write approval, self-protection floors).
7. Human-takeover lease for a headless desktop with a byte-level input gate.
8. Prompt-cache discipline as a first-class invariant, including cache-scope derivation across compression rotation and byte-frozen schemas.
9. Plugin governance (SHA-pinned catalog, kill list, installer-owned provenance, disclosure text, host isolation).
10. Engineering process encoded for AI contributors (ratchet with agent-readable fixes, size-capped guides, advisory-driven invariants).

Peers likely do some of these better in places (INF from general knowledge, not verified in this audit): Claude desktop and Cursor have deeper first-party UX polish, Codex's sandbox is OS-native by default, Manus is a hosted VM rather than a user-run desktop. Hermes's advantage is that nearly all of it is inspectable.

## 5. Weaknesses, limitations and risks

- Single-tenant security model. Plugins, skills, hooks and code execution run in the agent's own process by default; the OS-level boundary is the operator's job; one shared bearer key guards the API server; callers inside an allowlist are equally trusted (SRC `SECURITY.md`). Not suitable as-is for Verevon's multi-tenant plane.
- Heuristic guards. Approval patterns, deny globs and scanners are bypassable by design, and the docs say so (the deny-rule threat model note). Do not port them as boundaries.
- Risky defaults and features. MCP sampling is enabled by default; real-profile browsing copies cookies and login data into an agent-controlled browser; cloud browser modes advertise CAPTCHA solving; the local terminal backend runs unsandboxed unless configured; the iteration cap is unlimited by default.
- Complexity. 103 slash commands, 44 hooks, about 60 config sections, about 100 tools, 41 providers and a very large Python surface; documented drift already exists (hook count, iteration default, backend count, test counts).
- Python runtime constraints. A synchronous thread-based loop with blocking server-to-client requests, contextvar scope that relies on every thread hop using `spawn_context_thread`, AST discovery, and in-memory registries; the multi-profile guarantee depends on discipline enforced by lint rather than types.
- Gaps against a web chat product (scout finding, SRC grep, medium confidence): no dedicated citation UI, no deep-research mode, no hosted share link, no first-class chart tool, no enterprise SSO/SCIM, no ZDR propagation (the phrase appears only in the Nous provider plugin).
- Honcho and other external memory providers are catalog installs rather than bundled code, and the A2A and relay code is experimental or only manifest-verified here.

## 6. What Verevon should copy, adapt or avoid

Verevon context (from the baseline files `v-modelplane.baseline.md` and `v-chat-agents-ui.baseline.md`): the Model Plane has a 3,700-line plain-chat loop and a 13.5k-line governed Do loop, MCP tools that cannot be invoked by either loop, hooks not applied in Do mode, a stub plugin host, no server-side rewind/branch/search, no ACP/AG-UI/A2A, five hard-coded providers, and strong governance (capability policy, ZDR, tenant isolation).

Copy first (highest leverage, low risk):
1. MCP invocation path: per-server trust tier plus `readOnlyHint` classification to bind `mcp__*` tools to Allow/Ask capability decisions, with a circuit breaker, OAuth refresh and Unicode-tag stripping. This addresses the baseline blocker that every MCP call is denied for lack of a capability binding.
2. Tool disclosure: replace the 40-tool collapse with a budgeted, tiered listing plus bridge tools and schema validation; add the rarest-token gate if search is introduced.
3. Generated wire contracts (serde/schemars or utoipa) covering all request, response and event shapes shared by Model Plane, BFF and SPA, with a stale-file CI test; keep the existing SSE taxonomy doc check.
4. A blocking, replayable HITL channel for approvals and questions so the loop continues after approval (Model Plane currently re-executes only three effect tools after approval and does not resume the LLM loop), plus the event epoch and truncation watermark on resume.
5. Rewind/branch protocol: durable row ids, confirm flags, busy refusal, survivor ids, plus lineage fields in `sessions.proto`.
6. Declarative provider profiles with an error-classification hook, the effort-ladder clamp (fixes "OpenAI/Azure sends no reasoning effort"), credential pools with model-scoped cooldowns and a reset-aware fallback bench.
7. Task-local tenant context in Rust that fails closed (no process-global fallback), with CI rules for the leaking call-site shapes; adopt the "client can only ratchet stricter" rule inside Do mode and never read autonomy toggles from process state.
8. Staged write approval for learned memory and skills (Dreaming and skill learning currently write directly), and a trust-tier by verdict policy matrix for skill ingestion.
9. Quarry login handles: model proposes "fill from handle", Quarry fills with origin binding and exact-value redaction so the secret never enters model context.

Adapt (after design review):
- Facades: an OpenAI-compatible `/v1/chat/completions` and `/v1/responses` surface and a read-mostly ACP/A2A adapter in front of the existing capability policy; an outward MCP server for threads and runs.
- Tool-call ergonomics: path-scoped parallel batch planning with a never-parallel list, tool-loop guardrails and stop gates for the Do loop.
- Human takeover: a disk or DB-backed lease with epoch and a fail-closed input gate if Verevon reopens browser or desktop takeover.
- Browser reliability: native-dialog bridge and OOPIF session map in Quarry's executor; the extension-controller ticket protocol if the "user's own Chrome" decision is reconsidered.
- Observability: separate a content-free OTLP plane from content-bearing traces, with a fail-closed redactor.
- Process: a per-unit complexity ratchet and size-capped guides to decompose `sse.rs` and `agent.rs`.

Avoid:
- Treating in-process heuristics as security boundaries; copying real-profile cookie cloning, CAPTCHA solving, default-on MCP sampling, unsandboxed local backends, a single shared API key, or in-process plugin loading as the default.
- A single unified loop: Verevon's decision to keep read-only chat and governed Do separate is defensible given the authority boundary; borrow phase decomposition, not unification.
- Hermes's single-writer SQLite and thread-blocking designs; translate to Postgres and async tasks.

## 7. Corrections to the scout inventory and earlier Verevon analyses

- Hook vocabulary is 44 names, not 33 (SRC `hermes_cli/plugins.py:109-210`).
- The AGENTS.md says `max_iterations` defaults to 500; code default is unlimited (`sys.maxsize`, `agent.max_turns: null`).
- Desktop IPC: 167 `ipcMain.handle` registrations (non-test) rather than "about 200 channels", which counted event channels too.
- `COMMAND_REGISTRY` has 103 `CommandDef` entries (confirmed).
- "21 first-class plugin packages" and "8 pluggable sandbox or memory backends" in the earlier Verevon doc are loose: 21 platform plugins and 41 provider plugins exist; terminal backends are 8 modules; only two memory plugins are bundled (the rest are catalog installs and the in-tree provider directory is closed).
- Items the earlier Verevon docs lacked and this audit now records with evidence: tool search, trust-tiered MCP, MCP supply-chain preflight, computer use, Bot Desktop, browser extension control, CDP supervisor, LSP delta, egress proxy, secret scope, ACP, A2A, API server runs, event replay and contracts, rewind protocol, Bot Mode rooms, Tauri installer and update pipeline, managed scope, local runtime, Codex runtime bridge, subscription proxy, automation blueprints.

## 8. Open questions

1. How does the in-process `ctx.llm` trust gate interact with ZDR-style provider restrictions, and is there any Hermes concept of per-request retention policy beyond the data-policy guard? (Not found; likely none.)
2. What is the real-world failure rate of the cua-driver path on macOS given the reliance on private SPIs?
3. How complete is the A2A adapter beyond its manifest and DESIGN.md, and has it been interop-tested against ADK/LangChain peers?
4. Does the plugin host (`plugins.isolation: host`) have measurable overhead, and what fraction of the 570-entry catalog passes the static isolation audit (`evals/plugin_isolation`)?
5. For Verevon: should an MCP trust tier be per server, per org, or per tool, given that `readOnlyHint` is server-supplied and untrusted?
6. Which of Hermes's loop guardrails (repeat/cycle detection, verification stop gates) measurably reduce wasted tokens, and is there data in `evals/`?

## 9. Key evidence index

- Loop and phases: `agent/conversation_loop.py`, `agent/AGENTS.md`, `agent/turn_*.py`.
- Registry and disclosure: `tools/registry.py`, `tools/tool_search*.py`, `toolsets.py`.
- MCP: `tools/mcp_tool*.py`, `mcp_serve.py`, `optional-mcps/`.
- Plugins and hooks: `plugins/AGENTS.md`, `hermes_cli/plugins.py`, `hermes_cli/middleware.py`, `agent/shell_hooks.py`, `plugin-catalog/`.
- Environments and control: `tools/environments/`, `tools/computer_use/`, `tools/bot_desktop/`, `gateway/browser_control_broker.py`, `tools/browser_supervisor.py`, `agent/lsp/`.
- Models: `providers/base.py`, `agent/transports/`, `agent/credential_pool*.py`, `agent/reasoning_effort.py`, `hermes_cli/local_runtime/`.
- Safety: `SECURITY.md`, `tools/approval*.py`, `agent/secret_scope.py`, `agent/redact.py`, `tools/url_safety.py`, `tools/environments/docker_egress.py`.
- Platform: `tui_gateway/AGENTS.md`, `tui_gateway/event_replay.py`, `gateway/AGENTS.md`, `hermes_cli/AGENTS.md`, `acp_adapter/`, `hermes_state_*.py`, `apps/desktop/`, `apps/bootstrap-installer/`.
