# Cursor 3.17.21: how it harnesses models (AUDIT 3, facet harness-architecture)

Source: Cursor IDE 3.17.21 (Anysphere), installed at C:/Users/ImaFernandesDaCosta/AppData/Local/Programs/cursor. Role: deep-dive analyst. Read-only audit; nothing was run, installed or modified outside the scratchpad. Records: `cursor.harness-architecture.features.json` (141 records, ids cursor-harness-architecture-001 to -141). Record numbers below are written R001 to R141 (the last three digits of the id).

Provenance tags used throughout: REA = observed through a REA tool run on a shipped artifact; SRC = read directly in shipped program files (minified bundles, beautified into scratch copies; manifests; schemas); DOC = documentation (none exists in the install; the Verevon audit baseline is cited as DOC where used); INF = inference with its basis stated.

---

## 0. Method, evidence convention and honesty notes

### 0.1 What was analysed

Cursor ships its agent stack as separate VS Code extensions under `resources/app/extensions/cursor-*`, plus protobuf/Connect service descriptors inside `out/vs/workbench/api/node/extensionHostProcess.js` and `workbench.desktop.main.js`, two native Rust helpers in `resources/app/resources/helpers/` (crepectl.exe, cursorsandbox.exe), a bundled Node runtime, and N-API modules. The model prompts that run on the Cursor backend, tool routing logic and model weights are NOT in the install (INF from the install shape): what is visible is the client harness, the typed wire contracts and many prompts that the client assembles itself.

Important: the client contains a full local copy of the agent loop (`@anysphere/agent`), prompts, tool definitions and the permission engine. A feature gate (`agent_host_local_loop`) selects between a backend loop (called NAL, "new agent loop") and a managed local loop (R039). So the prompts and loop described below are real code that runs for at least some configurations; which configuration is the default is a server-side gate and is unknown (INF: backend NAL is the fallback path in the code).

### 0.2 Evidence convention

Minified bundles were beautified with esbuild-wasm (taken from the install, read-only) into scratch copies; line numbers in the records refer to those copies. Format: `<install-relative bundle> [beaut/<file>:<line>]`, where `beaut/` is `scratchpad/tmp/cursor-h/beaut/`. Mapping:

| beaut file | shipped file | lines |
|---|---|---|
| exec_main.js | extensions/cursor-agent-exec/dist/main.js | 216,437 |
| host_main.js, host_477.js | extensions/cursor-agent-host/dist/main.js, 477.js | 124,193 / (3 MB) |
| retrieval.js | extensions/cursor-retrieval/dist/main.js | 27,916 |
| shadow.js | extensions/cursor-shadow-workspace/dist/extension.js | 15,175 |
| mcp.js | extensions/cursor-mcp/dist/main.js | (3 MB) |
| browserauto.js | extensions/cursor-browser-automation/dist/extension.js | |
| commits.js | extensions/cursor-commits/dist/main.js | 15,668 |
| agentworker.js, alwayslocal.js | extensions/cursor-agent-worker, cursor-always-local | |

Records also cite `out/...` files (renderer bundles, utility processes), extension `package.json` manifests, `scratchpad/tmp/cursor-h/aiserver_services_full.txt` (65 aiserver.v1 service method tables extracted by script), `metrics.txt` (219 OTel metric names), and the scout inventory `scratchpad/findings/t3/cursor.inventory.md`.

### 0.3 REA tools actually used

Earlier attempt of this job (notes at `scratchpad/tmp/cursor-h/notes.md`), re-checked here:
1. `analyze_javascript_application` (format directory, detail summary, record-and-continue) on a scratch copy of the 12 cursor-* extensions: 222 files, 183 parsed JavaScript files, 12.6M AST nodes, 2,722 findings, 30 module nodes in the summary; retained evidence id `ev_131baecbd4b87236080aeca6460d94c92670cfa033ad709586026fd80ca9d189`. Coverage partial (329 application unknowns, 11,471 semantic unknowns).
2. `trace_application_feature` seed kind module on `cursor-agent-exec/dist/578.js` (718 nodes, 784 edges).
3. `inspect_pe_resources` on `cursorsandbox.exe` (PE32+ x64, no resource table).
The scout (before this job) ran `analyze_javascript_application` on `out/` (57 relevant files, 48 parsed) and `inspect_pe_resources` on Cursor.exe.

This pass (new REA runs):
4. `inspect_pe_resources` on `resources/helpers/crepectl.exe`: PE32+ x64, 5,331,752 bytes, sha256 9b309ab53cfe071aa12c005fe45934abc53ebb8cb3c781d2ac3dbb187c6d6c02, zero resources (evidence `ev_e2aa61ec...`). Consistent with a stripped Rust CLI.
5. `inspect_analysis_view` (page view, collection modules) on the retained extension-analysis evidence: 261 modules/assets in total, including `cursor-agent-exec/dist/213.js`, `248.js`, `578.js`, `cursor-shadow-workspace/dist/extension.js`.
6. `trace_application_feature` (module seed `cursor-shadow-workspace/dist/extension.js`) on the same evidence: 718 nodes, 784 edges, 32 endpoint nodes and 30 package nodes. The package nodes confirm the extension set (cursor-agent-exec, cursor-agent-host, cursor-local-agent-runtime, cursor-retrieval, cursor-mcp, cursor-commits, cursor-always-local, cursor-agent-worker, cursor-resolver, cursor-deeplink, cursor-browser-automation, cursor-shadow-workspace) and the runtime dependencies (piscina, better-sqlite3, tree-sitter, tree-sitter-bash, whichlang-node, @anysphere/tree-chunk-napi). The graph is the whole connected component, so it adds little beyond package and endpoint inventory.
Not used: open_binary, binary_session, search_strings/list_strings (they require the shared active target; avoided).

### 0.4 Verification performed in this pass

About 70 of the anchors used by the earlier notes were re-read in the beautified source (loop skeleton, streaming executor, arg repair, hooks wrapper, permission engine, AST analysis, summarization, blob store, redaction matrix, subagent Task schema, prompt sections, context builders, retrieval handshake, shadow workspace, commits tracker, conversation search schema, MCP process, sandbox support check, browser tool descriptions). An automated check confirms all 404 line anchors in the records exist; a manual sample of ~45 showed the cited line matches the claim. A few claims were kept from earlier notes at medium confidence and are labelled in each record's notes: bestOfNRunner and other flag-gated subagent kinds (R034, R012), the exact reach of tree-chunk-napi (R066), consumption of file_states_v2 (R127), the conflict barrier trigger line (R140).

### 0.5 Licensing and scope caution

Cursor is proprietary software. This audit extracts design shapes, contracts and behaviours from shipped artifacts for a read-only comparison. Per the Verevon rules (`docs/external-ideas-harvest.md`, cited in the audit baseline), proprietary material may contribute shapes only: reimplement the ideas, do not copy code, prompt text or schemas verbatim. Prompt excerpts here are paraphrased.

---

## 1. Executive summary: what makes Cursor's harness work

1. **The product is a thin UI over one reusable agent runtime** (`@anysphere/agent`, agent-client, agent-kv, chat-inference, agent-exec, shell-exec, mcp-core ...). The same packages power the IDE, a CLI, cloud/background agents and Grok Bot (R051). The runtime talks to the world through a small set of typed protobuf contracts, not through UI code.
2. **A single multiplexed streaming contract** (`agent.v1.AgentService.Run`, bidi, with SSE and polling twins) carries user actions, model output, tool-execution requests, KV blobs, interaction queries, checkpoints and heartbeats (R036). Tool execution is a typed **symmetric exec RPC**: for each tool there is an `xArgs` request and an `xResult` response, so the loop can live in the cloud while files and shells live on the user's machine, in a VM or in a private worker (R037).
3. **Context is assembled by programs, not strings.** The system prompt is a JSX-like component tree with named sections gated by model family, agent type, mode and feature flags (R121); a persisted `<user_info>` snapshot keeps the prefix cache-stable and is re-rendered only on explicit triggers (R122); each turn adds XML-tagged blocks (R123); a typed RequestContext with `*_info_complete` flags lets the client hydrate progressively (R124).
4. **Tool contracts are versioned per model family.** The Read tool's name, description and schema change with `promptVersion`; edit tools differ by vendor (Write+StrReplace for Anthropic/Gemini/xAI, ApplyPatch for GPT-5 family) (R058, R060, R104). Tools are declarative objects with zod schemas, stable analytics identifiers and owner-team labels (R072, R101).
5. **Edits are deterministic, not model-applied.** StrReplace does exact then whitespace-tolerant matching with fuzzy did-you-mean errors (R057); ApplyPatch parses a patch language client-side; the old fast-apply model RPC has shrunk to telemetry (R068).
6. **The loop is engineered for failure.** Streaming tool execution overlapped with generation (R003), unique-repair of malformed JSON (R004), streaming repetition detection (R005), typed recovery for max-token and empty responses (R006), a summarization ladder with self-summary, provider-native compaction, speculative background summarization and a model-free fallback (R007-R010), a typed failure taxonomy with split user/model messages (R136).
7. **Permissions are a real engine.** Shell commands are parsed with tree-sitter-bash into executable commands and redirects (R090) and then pass a layered decision pipeline (allowlists, team blocklists, delete protection, hooks, sandbox escalation, LLM classifier "smart mode") that returns reasons and suggested allowlist patterns (R089, R091, R092).
8. **State is content-addressed.** Conversation state is a graph of SHA-256 blobs behind layered stores (cache, retry, encrypt) (R025, R044), and privacy is enforced by types: classified fields, purpose-checked `unwrap`, five privacy modes (R045, R094).
9. **Native Rust is used exactly where Verevon will use it**: a sparse n-gram regex index (crepectl, R063), a sandbox with a built-in egress proxy (cursorsandbox, R082), file service and tree-sitter chunking N-API modules (R052).
10. **Filesystem as context**: terminals, large outputs, MCP descriptors, transcripts and plans are plain files under `.cursor/` that the agent can grep and read (R084, R075, R050). This is the main reason the product feels transparent and why it needs no bespoke memory or discovery APIs.

---

## 2. Architecture map

### 2.1 Process and extension topology (SRC)

```
Electron main (out/main.js)  --  auth, update, deep links, Statsig/Sentry, privacy-mode gates
 |
 +-- Renderers: workbench.desktop.main.js (IDE), workbench.glass.main.js (agent-first window)
 |      composer UI, controlled exec manager, composerHeaders table (state.vscdb)
 +-- Utility processes: conversationSearch (SQLite FTS5), mcpProcess (MCP supervisor),
 |      sharedProcess, watcher, extensionMonitor, processExplorer
 +-- Extension hosts
        AgentExec host:  cursor-agent-host   (session API: AgentHostService)      R038
                         cursor-agent-exec   (loop code, tools, exec registry, permissions, hooks)
                         cursor-local-agent-runtime (same modules, ui kind; "Private Inference")
        Workspace host:  cursor-retrieval (index, grep, git state)  cursor-shadow-workspace
                         cursor-mcp  cursor-commits (AI attribution)  cursor-browser-automation
        UI host:         cursor-always-local (validation, redaction, glass role)  cursor-agent-worker
 +-- Native helpers: crepectl.exe, cursorsandbox.exe, node.exe, *.node (file-service, tree-chunk, whichlang)
 +-- Backend (not in install): api2/api3/api4/agent.api5.cursor.sh, repo42; aiserver.v1 (65 services) + agent.v1
```

Evidence: R035 (extension manifests, proposed API list `control`, `cursor`, `cursorAgentHost`, `cursorTracing`, `cursorPseudoterminal`), R047/R048 (utility processes), R052 (native helpers), R054 (always-local, resolver).

### 2.2 Key design decision: the agent runtime is a library behind a host service

`AgentHostService` (16 RPCs) is the stable surface between UI and runtime: CreateSession, ForkSession, ListSessions, WatchSessions, SendMessage, SendAction, InterruptTurn, CancelQueuedTurn, RespondToInteraction, WatchSessionInteractions, WatchSessionBackgroundWork, WatchSessionTurnLifecycle, AbortBackgroundWork, AbortAllBackgroundWork, DeleteSession, AttachSession (R038). The UI never calls the model directly. The host also implements the "two loops" switch (R039). Verevon parallel: the gateway-to-Model-Plane link should look like this (watch streams plus unary controls).

### 2.3 Cloud, VM and self-hosted execution

`ControlService` (26 RPCs: Exec, files, diffs, workspace hash, artifacts persistence, MCP refresh tokens, env updates, reload skills/plugins, DownloadCursorServer, LoadMcpServers), `ExecService`, `PtyHostService`, `TmuxSessionService`, `LifecycleService` (claim/release worker, desktop session), and the private worker bridge (bidi Connect and DesktopStream) form the remote-environment plane (R040, R041, R087). The cloud sandbox is described to the model as a Docker container inside a Firecracker microVM (R083). Environment setup is declarative (`.cursor/environment.json` with install, start, terminals, ports, container build, snapshot) and the user watches a shared tmux session (R083, R030).

---

## 3. Wire contracts (the part most worth copying)

### 3.1 agent.v1 (9 services, SRC from extensionHostProcess.js)

| Service | Methods (count) | Role |
|---|---|---|
| AgentService | Run (bidi), RunSSE, RunPoll, NameAgent, UpdateConversationMetadata, CreateTranscriptOverview, GetUsableModels, GetDefaultModelForCli, GetAllowedModelIntents, UploadConversationBlobs, UploadLocalAgentRunToPromptQuality, GetSignedUrlForAttachedMedia, NotifyConversationClone, GetNewChatNudge*, GetPromptContextUsage (16) | conversation stream, models, blobs |
| AgentHostService (16) | session API | UI to runtime |
| ControlService (26) | remote control | cloud/VM/worker |
| ExecService (1), PtyHostService (6), TmuxSessionService (4), LifecycleService (5), PrivateWorkerApiService (2), PrivateWorkerBridgeExternalService (2) | | |

Message unions (R036, R037):
- AgentClientMessage: run_request, exec_client_message, exec_client_control_message, kv_client_message, conversation_action, interaction_response, client_heartbeat, prewarm_request.
- AgentServerMessage: interaction_update, exec_server_message, exec_server_control_message, conversation_checkpoint_update, kv_server_message, interaction_query, ttft_breakdown.
- InteractionUpdate: text_delta, partial_tool_call, tool_call_delta, tool_call_started/completed, thinking_delta/completed, user_message_appended, token_delta, summary(+started/completed), shell_output_delta, heartbeat, turn_ended, step_started/completed, prompt_suggestion, post_request_prompt, active_branch_change, feedback_request, response_comparison, context_injection_state, routed_model.
- InteractionQuery (server asks the user): web_search, ask_question, switch_mode, create_plan, setup_vm_environment, web_fetch, pr_management, mcp_auth, generate_image, replace_env, connect_scm.
- ExecServerMessage (server asks the client to execute): shell, shell_stream, background_shell_spawn, write, delete, grep, read, redacted_read, ls, diagnostics, request_context, mcp, list/read_mcp_resource, mcp_state, fetch, record_screen, computer_use, write_shell_stdin, execute_hook, subagent (+await, force_background), smart_mode_classifier, canvas_diagnostics, shell/mcp/web_fetch allowlist_precheck, git_diff, pi_*, mini_swe_agent_bash, conversation_search, agent_store_conflict, adopt, plus span_context and accept_hook_additional_contexts.

Design points: (a) KV blobs travel on the same stream, so state upload is incremental; (b) a precheck request type exists for allowlists so the UI can show approval state before a call; (c) `span_context` is on every exec request, giving distributed tracing across the loop/executor boundary; (d) `client_supports_*` booleans in AgentRunRequest implement capability negotiation (inline images, preview cards, routed-model updates, prompt-context usage RPC, send_to_user).

### 3.2 RequestContext and AgentRunRequest (R124)

RequestContext fields (SRC, extracted): rules, env, repository_info, tools, conversation/shared notes listings, git_repos, project_layouts, mcp_instructions, debug_mode_config, cloud_rule, web_search_enabled/web_fetch_enabled, skill_options, file_contents, user_intent_summary, custom_subagents, mcp_file_system_options, hooks_additional_context, hooks_config, agent_skills, precomputed_human_changes, recently_added_plugin, supports_mcp_auth, mcp_meta_tool_options, read_lints_enabled, non_file_rules, permission sets from user/project/admin, disabled_team_rules, search_conversations_enabled, send_message_enabled, admin_command_denylist, system_prompt_override, plus `*_info_complete` booleans for rules, env, repository, git status, MCP, skills, custom subagents. RequestContextEnv has os_version, workspace_paths, shell, sandbox_enabled/supported/network info, secret_redaction_enabled, computer_use_supported, time_zone, terminal/transcript/artifact folders, mounted_agent_stores.

AgentRunRequest adds conversation_state, action, model_details, requested_model, mcp_tools, skill_options, custom_system_prompt, system_prompt_spec, subagent_type_name, harness, selected_subagent_models, pre_fetched_blobs, run_id, agent_session_id and client_llm_gateway_credential.

### 3.3 ConversationStateStructure (R025)

root_prompt_messages (ids), turns, todos, pending_tool_calls, token_details, summary and summary_archives, plans, mode, file_states_v2, turn_timings, subagent_states/threads/runs/state_refs, self_summary_count, read_paths, active_branch_name, tracked_git_repo_branches, agent_type, communicate_update_* history, goal_state, completed_ask_question_tool_call_ids, durable_skill_blocks, durable_custom_mode_id, conversation_started_timestamp. Each entry is a blob id; Turn = user_message + steps (assistant_message | tool_call | thinking_message) + request_id.

### 3.4 aiserver.v1 (65 services in the desktop bundle; R042)

Largest: DashboardService 593, BackgroundComposerService 212, AiService 192, AutomationsService 42, EvalTrackingService 38, SandBoxService 36, GrokBotService 36, FullSelfDrivingService 24, VmDaemonService 22, RepositoryService 19, RelayService 18, ChatService 14, ShadowWorkspaceService 13, EnterpriseAdminService 12. Others include AuthService, SCMService, ReviewService, BugbotService, CppService (tab), CmdKService, FastApplyService (2), InferenceService, UploadService, CodebaseSnapshotService, CodebaseProtectionService, GitIndexService, GitGraphService, FileSyncService, TeamCreditsService, UsageSimulationService, MCPRegistryService, MarketplaceService, SchedulerService, RequestReplayService, ReplayChatService, ToolCallEventService, ChatRequestEventV2Service, OnlineMetricsService. Full method names: `scratchpad/tmp/cursor-h/aiserver_services_full.txt`.

Observation (INF): the backend API surface mirrors product areas rather than capabilities, which is why Dashboard (593) and BackgroundComposer (212) became mega-services. Verevon should keep Model Plane services capability-scoped.

---

## 4. The harness in detail

### 4.1 Prompt and context assembly (categories 4 and 13)

**Prompt as component tree (R121).** A tiny JSX renderer (`EO`/`TO` element constructors; `xO.renderToMessages`) renders System/User/Assistant/Tool elements into role messages; `section` elements become XML-like tags. Sections include task_management, tool_calling, making_code_changes, plan_mode_guardrails, tone_and_style, citing_code, maximize_context_understanding, semantic_search_instructions, browser_tools, inline_line_numbers, terminal_files_information, mode_selection, debug_mode_logging, continual_learning, background_agent, git_and_submission, testing, markdown_spec, status_update_spec, summary_spec, grep_spec, todo_spec, linter_errors. Components receive typed props (modelInfo flags, toolInfo.allTools, mode, agentType, featureFlags, requestContext), so a section appears only if the tools it mentions exist (for example semantic_search_instructions requires both SemanticSearch and Grep). Even the identity line varies by model.

**Per-step system prompt plus persisted user_info (R122).** Each step regenerates the system message, but the first user message (`<user_info>`: OS, shell, workspace paths, git state, date, terminals folder, mounted stores, rules catalog, skills catalog, subagent models and types, MCP servers or dynamic tool namespaces) is persisted and re-rendered only if the agent type changed, the subagent model/type descriptions changed, or the MCP meta-tool options changed. Why it matters: with Anthropic caching (R105) the prefix stays byte-stable between steps; the volatile information rides in later messages.

**Per-turn blocks (R123).** `<user_query>`, `<attached_files>` (files, selections, terminal output and selections with path and line attributes), `<attached_folders>`, `<documentation_context>`, `<open_and_recently_viewed_files>` (explicitly labelled as possibly irrelevant), `<agent_notes>`, `<cursor_rules_context>` (rules framed as extra documentation to use only if relevant), manually attached skills, `<system_reminder>`, `<timestamp>`.

**System reminders as the steering channel.** About sixty code sites (61 occurrences of the tag in the beautified bundle) append user-role `<system_reminder>` messages: mode changes, looping, max-tokens recovery, reflect nudges, UI intents (multitask, build in parallel), smart-mode blocks, "earlier turns were produced by a different model" (R107). Injecting steering as user-role reminders instead of editing the system prompt keeps the cached prefix intact. Hook-provided text is sanitized so it cannot forge reminders (R076).

**UI intents become prompts on the server (R015).** A `simulatedMsgReason` enum (commit, push, create PR, fix merge conflicts, babysit PR, split into PRs, apply locally, multitask, build in parallel) selects server-side templates. Pages send an enum plus context.

**Modes and base prompts (R125, R126).** AgentMode: AGENT, ASK, PLAN, DEBUG, TRIAGE, PROJECT, MULTITASK, CUSTOM. PROJECT uses a dedicated prompt: the main agent may not edit files, must delegate to subagents, follows a phase gate (clarify, research, plan, user review, implement, review panel, iterate, finalize), never runs write-capable subagents with overlapping scope, and externalizes state into `progress.md` in a per-conversation notes folder because context may be condensed at any time.

### 4.2 Tool system (categories 4, 5, 7)

**Registry per request (R071).** Tool sets are computed from model vendor/family, agent type (IDE, CLI, BACKGROUND), mode (Ask filters edit tools), flags, readonly shell, MCP state and subagent type. Model-facing catalog (SRC, names as in code): Read, Write, StrReplace (search_replace for dsv3-1018), ApplyPatch, Delete, Shell, Await, Glob, Grep, SemanticSearch, ReadLints, TodoWrite, Task (also Subagent/mcp_task), AskQuestion, SwitchMode, GenerateImage, WebSearch, WebFetch, GetMcpTools/CallMcpTool or GetDynamicTools/CallDynamicTool, FetchMcpResource, ListMcpResources, RecordScreen, ReplaceEnv, SetupVmEnvironment, Reflect, SendMessage, SendToUser, ConnectScm, SetActiveBranch, Adopt, CreateGoal, UpdateGoal, StartGrindPlanning, StartGrindExecution, SearchConversations, UpdatePrCodeTour, GetPrCodeTour, record_ci_investigation_findings, bash (mini-swe-agent compatibility), pi_* (bash, read, write, edit, find, grep, ls), and the provider-defined computer tool. Plan creation is an interaction query rather than a normal tool.

**Declarative definition (R072).** `pL(IDENTIFIER, {name, descriptionGenerator, parameters (zod), contextType, execute})`. The identifier (SHELL, READ, STR_REPLACE, TASK...) is a stable analytics key; the visible name can change per model; every identifier has an owner team label (async, core, bugbot, data-infra, agent-quality, extensibility) used in metrics (R101).

**Versioned contracts (R060, R104).** `promptVersion` (latest, dsv3-1018, dsv3-1205, cursor-0226, haiku, gpt5-codex, codex-cloud) changes tool names, descriptions and schemas (Read: target_file/offset/limit versus path/line_range versus negative offsets and include_line_numbers). `mde(slug)` derives vendor and family flags by substring matching (isGpt55, isOpus46, isComposer2, isComposerMatterhorn ...).

**Streaming execution (R003).** Tool calls are dispatched when their arguments complete, not when the response ends; partial arguments stream to the UI; duplicate provider tool-call ids are dropped or merged; reasoning, redacted-reasoning and signatures are preserved; on mid-stream errors the partial assistant message is kept.

**Robustness utilities.** Unique-repair of malformed JSON (R004), typed failure taxonomy with `clientVisibleErrorMessage` and `modelVisibleErrorMessage` (R136), large-output offload to files (R084, R139 on MCP spill), parallel-call encouragement plus metrics (R137), tool hooks (R024).

**MCP (R073-R075).** Runs in its own utility process with leases (R048). Tools are not all inlined: meta-tools GetMcpTools/CallMcpTool (or namespaced dynamic variants) discover and call tools; the prompt tells the model to read the tool's JSON schema (materialized under `.cursor/projects/<p>/mcps/<server>/tools/*.json` via atomic ~staging/~prev swap, with a STATUS.md) before calling; OAuth refresh races across runtimes are handled by a refresh lease patch to the MCP SDK; stdio servers can run under the sandbox helper with admin network modes; sandbox caches (npm, pnpm, go, cargo, pip, uv) are redirected to a temp dir.

### 4.3 The loop (category 4)

Step state machine (R001), queue-and-inject (R002), recovery (R005, R006), continuation injector (R006), summarization ladder (R007-R010), subagents (R012-R014, R141 model policy), hooks (R024), plan/goal/grind (R018, R020), debug mode (R021), multitask (R015, R016), ask-question durability (R022), conflict barrier (R140), A/B comparison (R027). Key properties:

- **Bounded and typed exits.** `maxSteps` finite; natural end when a step has no tool call and nothing was injected; plan creation breaks; otherwise error "possible looping".
- **Injection points.** Between steps: mode-change nudge, async ask-question completion, queued user messages, forced CLI reflect follow-up every 10 tool-calling messages, pre-final store-conflict barrier.
- **Failure recovery in layers.** Provider retry with abortable exponential backoff (1 s base, 60 s cap, 20% jitter) (R011); output-limit reminder once per turn (R006); empty-response retry budget 3 (R006); stream stall telemetry (R011); repetition detection with a 500 ms budget that fails open (R005); summarization retry with error classes (R007).
- **Subagents** (R012): Task tool args: description, prompt, model, subagent_type, resume (id or "self"), file_attachments, environment local|cloud, cloud base branch and environment build id, machine (new cloud VM, self-hosted worker or pool), interrupt, run_in_background. Types: explore, shell, bash, mediaReview, browserUse, computerUse, vmSetupHelper, debug, cursorGuide, watchVideo, custom, plus flag-gated types. Continuation config (idle threshold, max loops, nudge, escape token, collect background children) lets a coordinator wake on completion instead of polling (R014). subagentStart/Stop hooks can deny (R024).
- **Hooks (R024, R076).** 21 events; one envelope with allow/deny/ask, user_message, additional_context (size-capped), updated_input; fail-closed option; Claude Code settings are translated.

### 4.4 Context management (category 4)

Summarization ladder: (1) background self-summary started early, persisted only if the context really crossed a threshold (used tokens > max + min(25%, 50k) or image-count threshold), else discarded and classified (R009); (2) blocking self-summary through the same executor with identical system prompt and tools so the provider cache applies (R007); (3) provider-native compaction for Anthropic and OpenAI (R008); (4) external summarizer model when the self-summary hits the input limit (R007); (5) deterministic truncated transcript with max-min fair character allocation (R010). The summary is stored as a message with `isSummary` provider option, skill blocks are re-injected, and the last real user message is preserved as tail. Anthropic prompt caching uses rolling ephemeral markers on the last message with at most 4 cache blocks (R105).

Model switch: reasoning blocks carry `providerOptions.cursor.modelName`; if the model changes mid-conversation a reminder warns that earlier turns came from a different model and tools may differ (R107).

### 4.5 Apply, edit and shadow-workspace pipeline (category 7)

- **StrReplace (R057):** exact match; else whitespace-insensitive line match; else a failure with a fuzzy did-you-mean block (+/- 2 lines of context); ambiguity requires `replace_all` or more context; metrics for whitespace-insensitive usage; diff via the `diff` library with context 3 and a Piscina worker pool above 262,144 characters (R059).
- **ApplyPatch (R058):** Begin Patch language with Add/Update/Delete File headers parsed client-side; selected for GPT-5-family and Cursor-trained models.
- **Locks and lint feedback:** per-file operation lock manager for parallel tool calls (R059); ReadLints with a 10 s timeout; diagnostics exec request; ShadowWorkspaceService (GetLintsForChange, SwWriteTextFileWithLints ...) over a Unix socket Connect server in a hidden second window that mirrors the workspace folders; gated by a setting, disabled for remote authorities (R046, R067).
- **No apply model:** FastApplyService has only ReportEditFate and WarmApply (R068). CmdK and Tab (CppService) are separate low-latency services (R132).
- **AI attribution (R069):** cursor-commits snapshots files at request start and end, stores diffs with line-id maps, scores commits after they land (developer.aiCodeTracking.scoreCommitInternal) and triggers local Bugbot review for fresh unpushed commits.

### 4.6 Indexing pipeline (category 7)

Two independent indexes:
1. **Lexical (crepe, R063):** Rust `crepectl build --worktree --commit --cache-path --filter --memory-limit` writes index.bin, metadata.json and postings.bin with u32 n-gram tables and a "github-sparse-freq" frequency table, memory-bounded spilling, gitoxide to read git objects, rkyv zero-copy serialization, regex-automata and aho-corasick. The TypeScript GrepClient serves exact search, registers as VS Code TextSearchProvider2, and falls back to ripgrep for reasons no_index, unsupported_shape, index_no_snapshot, index_busy, index_unavailable, exec_failure, timeout, no_overlap, alarming if the fallback rate in a window is high.
2. **Semantic (R064, R065):** a Rust MerkleClient builds a tree of file hashes; handshake sends the root, a simhash and an encrypted-path scheme; SyncMerkleSubtree walks only mismatched subtrees; FastUpdateFile uploads changed files; the server may attach a query-only repository or copy an existing similar index (GetCopyStatus polling up to 3600 s). Limits: 50 concurrent uploads, 100,000 absolute file cap, auto-index only up to 10,000 files, 2 MiB batches, 600 s period. Embedding models include VOYAGE_CODE_2, TEXT_EMBEDDINGS_LARGE_3 and a custom QWEN 1.5B variant (FP8). The prompt makes SemanticSearch the default search tool (~10 ms on a warm index) and Grep the exact-text fallback (R062).
3. Extras: GitIndexService (PR history index) and GitGraphService (commit-graph related files) (R070), tree-sitter chunking N-API (R066), codebase snapshot packfile uploads and codebase protection policy (R064 notes).

### 4.7 Execution environments and safety (categories 6, 11)

- **Permission engine (R089):** parse (R090) -> parse-miss prompts as possible bypass -> hard deny (admin denylist) -> allowlists (user/project file `permissions.json` with `approvalMode` allowlist|unrestricted|manual, mcpAllowlist, terminalAllowlist; dashboard/team allowlists; smart-allowlist suggestions) -> team blocklist, soft denylist, delete protection -> readonly block -> hook-forced approval -> auto-approval by mode, with allowlisted commands escalating out of the sandbox -> else approval prompt with reasons and suggested patterns. Sandbox policies from the request and user config are merged (read boundary, extra paths, network allow lists).
- **Smart mode (R091):** LLM classifier for shell/MCP/web fetch/computer actions with natural-language `allow_instructions` and `block_instructions`; shadow mode; 10 s timeout, 2 attempts; block produces a reminder telling the agent it may request approval with a reason.
- **Sandbox helper (R082):** policy JSON {sandbox: type, readBoundary, hardcodedReadPaths, additionalReadonlyPaths, additionalReadwritePaths, ignoreMapping, networkAccess, disableTmpWrite, blockGitWrites; networkPolicy: default, allow, deny, logging; networkPolicyStrict}; bubblewrap on Linux (with `--preflight-only`), sandbox-exec on macOS, proxy-only on Windows; built-in HTTP CONNECT + SOCKS5 proxy for allow/deny with decision log.
- **Browser (R085):** accessibility snapshot with opaque refs (options: interactive only, depth, compact, selector scope, diff versus previous, screenshot-after); click by ref not by CDP; CDP escape hatch denies cookie/storage/permission/download/target/filesystem/system commands; per-tab `ownerAgentId`; origin allowlist hooks on page and navigation; iframes inaccessible.
- **Computer use (R086):** provider-native tools behind one internal action proto; restricted subagent.
- **Privacy (R094, R045):** five privacy modes; data classes SAFE, CODE, CREDENTIALS, PATH, PROVIDER_INFO, UNSPECIFIED; purpose-checked `unwrap`; training-related features (A/B comparison, prompt-quality upload) run only in mode 4.
- **Prompt injection (R095):** prompt-level caution plus hook-text sanitization; no tool-output injection classifier found (INF from absence).

### 4.8 State, persistence and search (category 13)

- Content-addressed blobs through layers (R044); AES-GCM with a key derived as SHA-256 of a key string and 12-byte IVs, fallback key for rotation.
- Local SQLite: `composerHeaders` rows with derived columns and a JSON value (R128); conversation FTS5 index in a utility process (R047); agent-store sync ledger with etags and pending deletes (R049).
- Project files under `.cursor/` (R050): rules, skills, agents, commands, plans, worktrees, plugins, mcp.json, hooks.json, permissions.json, sandbox.json, sandbox-policies, environment.json, projects/<p>/{agent-transcripts, agent-tools, terminals, canvases, mcps, agent-notes}.
- Persistent named agents: durable home store with SELF.md and SOUL.md plus event subscriptions (R032, R112); automations with memories (R114).

### 4.9 Telemetry and evals (category 10)

219 OTel metrics (R055), TTFT breakdown in-stream (R098), prompt token breakdown (R099), tool-call metrics with owner team (R101), hook tracking (R102), typed AgentEventTracker (R029), eval overrides and external-harness tool sets (R028, R080), A/B response comparison (R027), post-turn labelling and follow-up classification RPCs (R129), RPC trace logging with truncation (R100). Privacy mode suppresses Sentry and gates data-collecting features.

---

## 5. What is best-in-class (relative to the peers in this audit)

1. Typed exec RPC and single multiplexed stream (R036, R037). No other reference exposes tool execution as a typed pair with prechecks and span context.
2. Streaming tool dispatch, arg repair, repetition guard, typed failures (R003-R005, R136): the most complete loop-hardening set seen.
3. Summarization ladder with background speculation (R007-R010).
4. Shell permission engine on AST facts with reasons and sandbox escalation (R089, R090); smart mode with shadow rollout (R091).
5. Privacy by types (R045, R094) and content-addressed state (R025, R044).
6. Prompt as component tree with model/mode gating and stable-prefix design (R121, R122).
7. Context-as-files (R075, R084, R050): terminals, outputs, MCP catalogs, plans, transcripts.
8. Rust index/sandbox helpers (R063, R082).
9. Hooks with a uniform envelope and Claude Code compatibility (R024, R076).
10. Project Agent Mode and UI-intent templates (R126, R015) as concrete orchestration patterns.

## 6. Weaknesses and limits

- **Opaque server half.** Prompts, routing and weights are server-side; flags (641 client gates) change behaviour remotely; reproducing exact behaviour from the client is impossible (INF).
- **Monolithic implementation.** One large `runTurnLoop` and ~216k-line beautified bundle with heavy flag branching; names are mangled; maintainability looks poor (INF from structure). R001 notes.
- **Slug string matching** for model families (R104) is brittle.
- **Windows has no filesystem sandbox** (proxy only) (R082); Linux needs bubblewrap and kernel support; sandbox escalation for allowlisted commands means the allowlist, not the sandbox, is the trust boundary for those.
- **Prompt-injection defence is mostly prompt text** (R095).
- **Several features are partial or flag-gated**: grind/goals (R020), best-of-N (R034), private inference runtime, shadow workspace (R046), effort-level and image-resizing middleware stubs (R106).
- **API surface sprawl** (DashboardService 593 methods) (R042).
- **Privacy-mode 4 data collection** (A/B comparison, prompt-quality uploads) is built into the loop (R027); correct gating matters.
- **Memory is weak:** transcripts and rule files only; no automatic user model (R111).
- **Extension-host coupling:** the runtime depends on a VS Code fork API (`cursorAgentHost` proposal), so reuse outside Cursor needs the Grok Bot style host.

## 7. What Verevon should copy, adapt or avoid

Mapping uses the Model Plane constraints in the audit baseline (DOC: `scratchpad/findings/t3/v-modelplane.baseline.md`): two loops that must stay separate (read-only gateway chat loop versus governed Do loop), capability-before-dispatch fail closed, ZDR cannot be downgraded, Quarry executes browser actions the Model proposes, Integration Core owns tokens, no cross-plane DB access, Rust hot path with Go control.

### 7.1 Copy (adapt to Rust)

| Technique | Records | Where it fits in Verevon |
|---|---|---|
| Typed exec request/result pairs with allowlist precheck and span context | R037 | Model Plane to Quarry/sandbox/Integration Core contract: "Model proposes, X executes or rejects" as proto messages, with capability evidence in the request |
| Single multiplexed stream with SSE and poll fallbacks, capability negotiation flags | R036 | Gateway to page link (already SSE): add typed event union and `client_supports_*` negotiation |
| Session service with watch streams (interactions, background work, lifecycle) | R038 | Chat and agent pages reconnect to a run; works with session-core as single writer |
| Content-addressed turn blobs with layered stores | R025, R044 | session-core: fork, rewind (known gap), resume; ZDR mode maps to no-store layer |
| Typed privacy classification with purpose-checked unwrap | R045, R094 | `Redacted<T>` newtype in the shared contracts crate; used by logs, memory, caches, comparison features |
| Prompt component tree and per-step system prompt + persisted environment snapshot | R121, R122 | Seed Do mode with history and instructions (known gap: context-blind) in a cache-stable way |
| Streaming tool dispatch, arg repair, repetition guard, failure taxonomy | R003-R005, R136 | Both loops; extends existing round budgets and retry vocabulary |
| Summarization ladder incl. speculative background and deterministic fallback | R007-R010 | MG/compaction.rs and EC/compaction_budget.rs |
| Hook envelope applied in both loops, fail-closed option | R024 | Fix the empty hook_context in Do mode |
| Shell policy crate on tree-sitter-bash facts with reasons and sandbox merge rules | R089, R090 | Replace the substring risky-tool classifier before any shell tool is added |
| Sandbox policy schema with default-deny egress proxy and decision log | R082 | execution-core bwrap sandbox: add network policy and logging; later per-session microVM |
| MCP supervisor with refresh leases; stdio servers under the same sandbox | R048, R073 | Integration Core and execution-core; resolves the quarantined stdio path |
| Dynamic tool discovery with schema-on-demand and file-backed catalogs | R074, R075 | MCP staged disclosure exists; fix capability binding first (known gap 1) |
| Subagent typed kinds, resume/fork, continuation config, model policy | R012, R014, R141 | Extend depth-1 subagent.task |
| UI intents as server-rendered templates | R015 | Composer buttons send enums |
| Project mode: orchestrator + progress.md + write-scope locks | R126 | Long-running Space agents |
| Context-as-files for terminals, outputs, plans | R084, R050 | Sandbox workspace conventions in Spaces |
| Lexical n-gram index and Merkle sync | R063, R064 | Data Plane owns retrieval; consider crepe-like index for Space file grep |
| Conversation FTS in a separate index with reconciliation cursor | R047 | Server-side chat search (known gap) |
| Browser tool vocabulary (snapshot refs, diff, restricted CDP, tab ownership) | R085 | Quarry observation/action schema |

### 7.2 Adapt with care

- **Smart mode (R091):** use LLM classification only after deterministic policy; keep it out of ZDR paths unless the provider is attested.
- **A/B response comparison (R027):** only for consenting, non-ZDR tenants.
- **Agent identity files and stores (R112):** Verevon already has Letta memory and Dreaming; use files for inspectability, not as the primary store.
- **Canvas SDK (R115):** valuable for artifact quality; needs a sandboxed renderer and CSP in a web app.
- **Local loop versus backend loop (R039):** Verevon has no local agent; do not add a third loop. Keep two (chat, Do) with a shared executor contract.

### 7.3 Avoid

- A dedicated apply model (R068).
- Slug substring matching for model capabilities (R104); use a model profile table.
- Mega-services (R042) and one 3,000-line loop method (R001).
- Prompt-only injection defence (R095).
- Windows-style proxy-only sandbox as the isolation story (R082).
- Hidden server-side flags that change agent behaviour without a trace; Verevon should expose the effective profile in the run record (transparency).

### 7.4 Rust crate sketch (INF, R056)

`mp-agent` (loop state machine, recovery, summarization traits), `mp-agent-client` (stream transport, retry, stall), `mp-state` (content-addressed store with cache/retry/encrypt layers), `mp-exec-contract` (prost types for exec pairs and RequestContext), `mp-policy` (shell AST facts, decisions, merge rules, tree-sitter-bash), `mp-sandbox-policy` (policy JSON and proxy decision log), `mp-prompt` (component tree and model profiles), `mp-tools` (ToolSpec registry keyed by tool id and prompt version, zod-equivalent JSON schema via schemars), `mp-redact` (Redacted<T> with purpose-checked unwrap), `mp-metrics` (OTel metric names and owner labels). Several of these already exist in some form in `CR/mp-contracts`, `mp-slo`, `mp-toon`; reuse rather than duplicate.

## 8. Gap matrix versus the Verevon Model Plane (from the audit baseline, DOC)

| Verevon known gap (baseline) | Cursor technique that addresses it | Record |
|---|---|---|
| Do mode context-blind (no history, persona, org or Space instructions) | Persisted user_info snapshot plus typed RequestContext | R122, R124 |
| Hooks not applied in Do mode | Uniform hook envelope with fail-closed | R024 |
| MCP tools denied by capability gate (no binding for mcp__*) | Not a Cursor technique; Cursor binds MCP via allowlist patterns `server:tool` and precheck RPC | R093 |
| Scheduled runs are one no-tool call, no delivery | Automations run full agent sessions with memories, retry, run history | R033, R114 |
| No rewind, branching, share links | Content-addressed state; fork-from-turn; share RPC | R025, R116 |
| No server-side chat search | FTS5 index with reconciliation | R047, R113 |
| Plugins registry inert | Plugin manifest + loader (skills, commands, agents, hooks, MCP) | R077 |
| Sandbox is bwrap in execution-core container only; no per-session VM | Sandbox policy JSON + proxy; Firecracker VM with environment file | R082, R083, R030 |
| Post-approval: LLM loop does not resume | Durable ask-question completion injected as tool result | R022 |
| Risky-tool classifier is a substring list | AST-based command facts | R090, R089 |
| No reasoning-effort control, no Gemini/DeepSeek provider | Reasoning block preservation and model profiles (provider breadth not shown by Cursor harness itself) | R107, R104 |
| Subagent depth 1, results need consent | Typed subagents, resume/fork, continuation config | R012, R014 |
| No OS computer use (parked) | Provider-native computer tools + VM | R086 |
| Process-local state blocks multi-replica correctness | Host session API + checkpointed state DAG | R038, R025 |

Where Verevon is ahead (per baseline): governance and capability policy, tenancy, evidence-bound ZDR, provenance and honesty contracts, memory with Dreaming, in-gateway deep research and citations, Temporal workflows. Cursor adds little on these and the audit should not "improve" them toward Cursor.

## 9. Open questions

1. Which loop (backend NAL versus local managed loop) is the default per client type, and how do prompts differ between them? (gate value is server-side)
2. How does the server use `ConversationStateStructure` blobs: does the backend loop fetch them through the KV channel (kv_server_message) on every step?
3. Is `file_states_v2` used for stale-read detection or only for attribution? (R127)
4. What is Private Inference (cursor-local-agent-runtime)? It reuses the exec module set with ui kind; models and download prompt flags suggest a local model runtime (INF, low).
5. Effort-level and image-resizing middleware are stubs here: where do reasoning effort and image limits get applied (server)?
6. How are cloud VM snapshots restored (`snapshot` field, agentCanUpdateSnapshot in environment.json) and what is the SandBoxService manifest versioning? Only client descriptors are visible.
7. Does codebase protection enforcement (CodebaseProtectionService) block agent reads or only indexing?
8. Grok Bot shares the harness: do its AgentStoreService and persistent-agent features flow back into Cursor (the sand_* flags in Cursor suggest yes)?

## 10. Appendix

### A. Hook events (R024)
beforeShellExecution, afterShellExecution, beforeMCPExecution, afterMCPExecution, beforeReadFile, afterFileEdit, beforeTabFileRead, afterTabFileEdit, beforeSubmitPrompt, afterAgentResponse, afterAgentThought, stop, sessionStart, sessionEnd, preCompact, subagentStart, subagentStop, preToolUse, postToolUse, postToolUseFailure, workspaceOpen.

### B. Tool failure types (R136)
noop, invalid_args, unexpected_environment, user_rejected, timeout, provider_error, bad_user_device_state, aborted, exec_backend_unavailable, hook_denied, mcp_auth_error, error.

### C. Privacy matrix (R094)
modes: 0 UNSPECIFIED, 1 NO_STORAGE, 2 NO_TRAINING, 3 USAGE_DATA_TRAINING_ALLOWED, 4 USAGE_CODEBASE_TRAINING_ALLOWED. Purposes: STORAGE_FOR_TRAINING, STORAGE_FOR_LOGGING, STORAGE_FOR_USAGE, UNSAFE_ALWAYS_ALLOWED. Classes: SAFE, CODE, CREDENTIALS, PATH, PROVIDER_INFO, UNSPECIFIED. SAFE always allowed; CREDENTIALS and UNSPECIFIED never storable except UNSAFE_ALWAYS_ALLOWED; modes 0 and 1 nothing; mode 2 logging allows PATH and PROVIDER_INFO, usage allows all, training none; modes 3 and 4 all.

### D. Metric families (R055)
agent.* (116: step, turn, tool_call, tools.read/str_replace/edit_common/generate_image, summarization, background_summarization, conversation_state.restore, goal, ttft.*, store, loop), agent_client.* (stream stall, request_context_blob), agent_kv.* (24 layers), mcp.* (22: meta, discovery), chat_inference.* (ttft_ms, ttf_toolcall_ms, e2e, input), anthropic_compaction.*, openai_compaction.*, self_summary.*, glob.*, bugbot.*.

### E. Notable constants (SRC)
Tool output offload threshold 40,000 bytes, cap 50 MB, GC 30 days; Piscina diff threshold 262,144 chars; read_lint_timeout 10 s; smart classifier 10 s, 2 attempts; max-token reminder once per turn, 5 iterations; empty response budget 3; summarization attempts 3; cache blocks 4; backoff 1 s to 60 s, 20% jitter; indexing: 50 uploads, 100,000 files, auto 10,000, batch 2 MiB, 600 s period, copy timeout 3,600 s; conversation search FTS prefix 2 3; hook defaults loop_limit null, failClosed false; ndjson ports 7242-7942; plugin cap 10 MiB.

### F. Records by category
Agent runtime 46 (R001-R034, R121-R127, R136-R138, R140, R141), Platform architecture 24, Coding capability 15, Tools/plugins 13, Computer/environment 8, Safety 10, Transparency 8, Models 7, Memory 4, Chat 3, Multimodal 1, Research 1, UI/UX 1. Quality 5: 27 records; 4: 70; 3: 43; 2: 1. Uniqueness: unique 48, uncommon 65, common 28. Provenance: SRC 137, REA 3, INF 1. Confidence: high 114, medium 27.

Chat, multimodal, research and UI categories are thin by design: this facet targets harness architecture, and Cursor is an IDE; the scout inventory covers breadth for those categories.

### G. Records not discussed in the narrative (details in the JSON)
R013 custom subagents from .cursor/agents; R017 background shell with Await and notify_on_output (covered in 4.3 only briefly); R019 todo accounting; R023 mode switching tool; R026 reflect and progress reminders; R031 PR babysit and autopilot prompts; R043 legacy OAL/NAL chat stack; R053 local/BYOK provider with apiKeyHelper and Bedrock chain; R061 Grep/Glob guards; R078 skills roots and budget; R079 rules tiers; R081 MCP registry and marketplace; R088 git-aware context; R096 read policy outside workspace; R097 secrets as placeholders; R103 NDJSON ingest; R108 usable-model and routed-model APIs; R109 perception subagents on specific models; R110 prewarm; R117 markdown output rules; R118 image generation guards; R119 web search and fetch governance; R120 Glass window; R130 telemetry stack; R131 ADMX policies; R133 worktrees; R134 commands in subagents; R135 distribution and updates.
