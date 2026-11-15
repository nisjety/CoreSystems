# Google Antigravity 2.0 (agent hub): agent-first UX and Gemini integration

Deep dive for Audit 3 (Model Plane and Verevon chat/agent pages). Facet: `agent-ux-and-gemini`. Companion file: `antigravity.agent-ux-and-gemini.features.json` (143 records, ids `antigravity-agent-ux-and-gemini-001..143`). Read-only audit. Nothing was launched, no project code was run, no per-user data store was read.

## 0. Summary

Antigravity 2.0 (build 2.11.0) is not the VS Code fork. It is a standalone desktop "agent hub": an Electron shell around a 153 MB Go language server that runs the agent loop, serves a React web UI over loopback HTTPS, and can be driven remotely from a browser. The editor is a separate app that is not installed on this machine. [SRC] `inv` section 0; [DOC] antigravity.google/docs/overview.

What makes it worth studying for Verevon is not the model list. It is a small number of design decisions that turn an autonomous agent into something a human can supervise:

1. **A typed step trajectory is the single data model** for chat, tools, plans, subagents, checkpoints, costs, hooks and debugging. The UI is fed by a paged, indexed delta stream of that model, not by raw events. [SRC] features 001, 027.
2. **Plans, task lists and walkthroughs are artifacts with a review state**, and review comments (on artifact selections, file diffs, files) are typed inputs to the next agent turn. The agent can block on review, report a confidence score, and auto-proceed under a policy. [SRC] 013, 029-032.
3. **Permissions are one grammar** (`action(target)`, deny over ask over allow, five scopes) with presets, per-project overrides, a model-based guardian and a visible reason for every prompt. [SRC] 097, 103-105.
4. **Everything the harness does is inspectable**: per-step timing, the exact prompt sections, token breakdown by category, steps visible to the user but hidden from the model and the reverse, hook activity, cost by token category. [SRC] 093-096, 088.
5. **Cost-tiered multi-model orchestration**: planner, subagents, checkpointing, guardian, browser agent and terminal input detection each have their own model slot or tier, and models declare capabilities that adapt prompts and tool formats. [SRC] 083-084.
6. **Parallel attempts with preference capture** (Best of N in isolated clones) and **a Remote Control tunnel** that lets the same UI drive a host machine from any browser. [SRC] 047, 067.

Where it is weak: it is a local, single-user, Google-account product with a large Google-internal residue; planning mode was partly retired in this build; Windows is a second-class platform for permissions and sandboxing; telemetry and sharing upload full trajectories; and BYOK is not supported. Sections 10 and 11 give the Verevon-specific reading.

## 1. Scope, method and provenance

### 1.1 Targets
- Install: `C:/Users/ImaFernandesDaCosta/AppData/Local/Programs/antigravity` (read-only): `Antigravity.exe` (Electron 41), `resources/app.asar` (shell, 4.5 MB), `resources/bin/language_server.exe` (Go, 153 MB), `resources/bin/webm_encoder.exe`, `resources/app.asar.unpacked/node_modules/chrome-devtools-mcp`.
- Pre-extracted shell: `scratchpad/extract/antigravity/dist` (main process code).
- **New in this pass:** the web UI bundle is embedded as a zip inside `language_server.exe`; it was recovered by a local-file-header scan into `scratchpad/tmp/ag/ui` (index.html, main.js 8.9 MB Closure-compiled, compiled Tailwind CSS, Prism, an AudioWorklet, a diff worker, two notification sounds). The scout inventory listed this as a blocker; it is resolved.
- **New in this pass:** the Go language server embeds its protobuf descriptors. A scratch script (`scratchpad/tmp/ag2/descs.cjs`) decoded 469 of 481 descriptor blobs into readable proto text under `scratchpad/tmp/ag2/protos`. This gives exact message, enum and RPC definitions, replacing noisy string greps.
- Public docs: antigravity.google/docs pages fetched on 2026-10-13 through WebFetch (a summarising fetcher, so exact wording is not reproduced and details are medium confidence unless corroborated in the binary).

### 1.2 Provenance tags used in this file and in the JSON
- **REA**: observed through REA on a shipped artifact. Used for the UI-bundle analysis (partial) and the scout's shell analysis.
- **SRC**: read directly from shipped files: the decoded protobuf descriptors, the extracted UI bundle and its string table, Go-template prompt text inside the binary, the Electron shell source.
- **DOC**: public documentation (WebFetch summaries).
- **INF**: my inference, with the basis stated.

Path aliases: `SP` = `C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad`. `PROTO` = `SP/tmp/ag2/protos`. Evidence strings in the JSON are `scratchpad/<relative path>:<line>`.

### 1.3 Corrections to the scout inventory
The scout was honest about its limits; these are the verified differences.
1. **Step types:** the decoded `CortexStepType` enum has 118 values including UNSPECIFIED, not 122. [SRC] `PROTO/..cortex.proto.txt:49`.
2. **Trajectory types:** DEEP_THINK, AUTO_CASCADE and BACKGROUND_RESEARCH are not `CortexTrajectoryType` values. The real list is USER_MAINLINE, USER_GRANULAR, SUPERCOMPLETE, CASCADE, CHECKPOINT, APPLIER, TOOL_CALL_PROPOSAL, TRAJECTORY_CHOICE, LLM_JUDGE, INTERACTIVE_CASCADE, BRAIN_UPDATE, BROWSER, KNOWLEDGE_GENERATION, SIDE_QUESTION, COMMIT_MESSAGE_GENERATION. BACKGROUND_RESEARCH is an experiment key (a config-override flag). [SRC] `cortex.proto.txt:31`, `codeium_common.proto.txt:12`.
3. **Prompt injection:** the scout found no prompt-injection phrase. The UI warns about it explicitly in the settings and agent-onboarding copy. [SRC] `SP/tmp/ag/ui_strings.txt` (search "prompt injection").
4. **RPC surface:** the language server service has 305 RPCs (not a noisy 632-entry list); ApiServer 115, extension bridge 53, seat management 96, index 23. [SRC] `PROTO` service blocks.
5. **Model ids:** the `Model` enum in the descriptor is a short, older list (Gemini 2.5 family, Claude 4 and 4.5, GPT-OSS 120B, internal tab models). Newer model ids are strings served by the model catalogue; the lineup in section 6.1 comes from the public Models page.
6. **Battle mode** is confirmed as parallel forks with a winner (`BattleModeInfo`, `BattleEndType`), not a guess from names.

### 1.4 REA tool usage (actually run in this pass)
- `analyze_javascript_application` on `scratchpad/tmp/ag/ui` (directory, summary, record-and-continue, 2 GB heap): 5 relevant files, 4 JS files parsed, 1.58M AST nodes, 496 findings. Coverage partial: the semantic pass exhausted its heap on `main.js`. Evidence `ev_3e8e2730f6568f9e50e7c0e8946949767ded9e1a4e4ad8fdbdbb0c0866713221` (summary view), parent `ev_a6e69469665b8e550d6105cbc76529c09c66b4f36db92aacaf73143102e665f9`.
- `inspect_analysis_view` (modules page): confirmed main.js, prism_bundle.js, audio_processor.js, diff_worker.js as modules/assets. Evidence `ev_0c81d3815b59c2e80a2e979d5a6538852611e1ca5aab6fc603880391726766b2`.
- `trace_application_feature` (string seeds "implementation_plan.md", "Concierge is listening", "Revert and redo turn, best of N"): no match, with a coverage warning. This reflects the partial graph, not absence; features were verified by direct reads instead. Evidence `ev_96352cba...`, `ev_f26f4aff...`, `ev_9fcb08a8...`.
- Tools not used: `open_binary`, `search_strings`, `list_strings`, `binary_overview` (to avoid the shared REA active target while other agents run); the 153 MB binary was read with local scripts instead.
- Scout REA evidence reused where cited: parent shell analysis `ev_07f3b30797b1ab05de8887e6262b25e5409b2d05896c0e698bd33b9ef65a7ddc` (session-local).

## 2. Architecture

```
                         Google cloud
   Gemini API / Cloud Code endpoints, Vertex partner models, quota,
   web-search grounding, image gen, transcription, share links, admin controls
                                 ^
                                 | Connect RPC + model calls
 +-------------------------------+----------------------------------------------+
 | language_server.exe (Go, 153 MB)                                             |
 |  - agent loop ("Cascade"): planner, executor, tools, hooks, checkpoints      |
 |  - LanguageServerService (305 RPCs) + plugins, remoting, index, ext-bridge   |
 |  - reactive streams (StreamAgentStateUpdates, unified state sync topics)      |
 |  - serves embedded UI zip over https://127.0.0.1:<dynamic port>/              |
 |  - sidecar manager, MCP client, terminal sandbox, browser (CDP) driver        |
 |  - Remote Control client: outbound ConnectInstanceV2 stream to a broker       |
 +--------------+---------------------------+----------------------------+------+
                | stdio, flags, env         | loopback host bridge        | children
                v                           v (bearer token)              v
 +----------------------+        +-----------------------+     chrome-devtools-mcp,
 | Electron 41 shell    |<------>| HostBridgeService     |     webm_encoder.exe,
 | window, tray, menu,  |        | GetUpdateStatus /     |     sidecars, MCP servers,
 | updater, deep links  |        | ApplyUpdate           |     sandboxed commands
 +----------+-----------+        +-----------------------+
            | loads                       Browser (any device) --- Remote Control ---> broker --> LS
            v
   React + Redux Toolkit SPA ("Jetski Web"): Lexical composer, xterm.js, KaTeX, ELK, virtualised lists
```
[SRC/REA] shell flags and bridge from `inv` section 2b; descriptor services from `PROTO/*language_server*`, `PROTO/devtools__jetski__boq__api__proto__api.proto.txt`; UI from `SP/tmp/ag/ui`.

Key properties:
- **Same engine, many fronts.** `CortexTrajectorySource` includes the hub client, CLI, SDK, sub-agents, agent API, replay and eval; `AgentMessageOrigin` has IDE, SDK_EXECUTABLE and SUBAGENT. [SRC] `cortex.proto.txt:30`, `language_server.proto.txt` (AgentMessageOrigin). The Python SDK, `agy` CLI, IDE extensions and the hub all use it. [DOC] docs sdk/overview, cli/features.
- **Single UI bundle.** The same web app runs in Electron and in a browser through Remote Control; there is a PWA/notification path for remote use. [SRC] `ui_copy.txt` (install-as-app copy).
- **Feature-provider composition with internal and external app types in one bundle**, so internal-only features (agent team command, concierge, file tree, favicon badge, CitC/Piper/Fig/Cog/ABFS workspaces) ship in the public build behind flags. [SRC] `main.js` bootstrap (`appType`).
- **Data directory.** Per-conversation `brain` directory with artifacts and a JSON-lines transcript; sidecar data; config files under `~/.gemini/...` (names only; not read). [SRC/DOC] snippets, hooks docs.

## 3. The data model that carries everything

### 3.1 Trajectory and steps
`gemini_coder.Trajectory` holds `Step` records. The step enum covers planner responses, run command, file edits, searches, 25 browser steps, task boundary, notify user, ask question, invoke subagent, checkpoint, conversation history, ephemeral and system messages, MCP tool, tool search, memory, brain update, knowledge generation, deploy and cloud steps. [SRC] `cortex.proto.txt:49`.
- **Status machine:** GENERATING, QUEUED, PENDING, RUNNING, WAITING, DONE, INVALID, CLEARED, CANCELED, ERROR, INTERRUPTED. **Run status:** IDLE, RUNNING, CANCELING, BUSY. **Source:** MODEL, USER_IMPLICIT, USER_EXPLICIT, SYSTEM, SYSTEM_SDK. [SRC] `cortex.proto.txt:35-39`.
- **`CortexStepMetadata`** per step: created/viewable/started/completed times, tool call and modified tool call, model usage, retry infos, model cost, requested vs generator model, flow/prompt credits, tool-call choices, snapshot metadata, internal status transitions, pre-tool hook results, data-access marker, remote-control details. [SRC] `cortex.proto.txt:706`.
- **Generator metadata** per model call: system prompt, message prompts, prompt sections with token type and source, tools, time to first token, streaming duration, retries, cache breakpoints, credits. [SRC] `cortex.proto.txt:637`.
- **Executor metadata** per execution: termination reason, invocation counts, before/after snapshots, cascade config used, experiment ids. [SRC] `cortex.proto.txt:2547`.

### 3.2 State stream
`StreamAgentStateUpdates` (server stream) sends `AgentStateUpdate` messages. Every collection (steps, generator metadata, executor metadata, queued steps, artifact snapshots, file diffs, background commands/tasks, pending agent messages, accumulated sections, battle-mode infos) is a delta of `indices + total_length + page_bounds`. The request carries a verbosity (DEBUG, PROD_UI, FULL), initial page bounds and a rehydration switch; paging is requested with `RequestAgentStatePageUpdate`. The UI shows load-older controls. [SRC] `jetski_cortex.proto.txt:8,33-77,127,257`. Quality 5: this is the cleanest long-run streaming contract in the peer set.

### 3.3 Artifacts
`ArtifactSnapshot {name, uri, last_edited, review_state, ArtifactMetadata, content | url payload}`; `ArtifactMetadata {type IMPLEMENTATION_PLAN | WALKTHROUGH | TASK | OTHER, summary, version, request_feedback, user_facing}`; `ArtifactReviewState {PENDING_REVIEW | REVIEWED | MODIFIED, last_reviewed_content}`. Files are written by normal file tools; creating an artifact requires metadata. [SRC] `cortex.proto.txt:1884`, `codeium_common.proto.txt` (ArtifactMetadata), snippets (create-file tool).

### 3.4 Permissions and settings
`PermissionResource {action, target}`, `PermissionGrantsConfig {allow, deny, ask}`, `PermissionScope {ONCE, CONVERSATION, WORKSPACE, PROJECT, GLOBAL}`. User settings live in a typed object (`jetbox_state.UserSettings`, 40+ fields) synchronised with unified state sync; project settings override. [SRC] `cortex.proto.txt:1704`, `jetbox_state.proto.txt`, `project.proto.txt`.

### 3.5 Cost
`CostDisplaySection {input, cache_read, output, thinking, response, tools, tool_breakdown, estimated_cost_usd, pricing}`; `TurnCostSummary {turn, subagent, cumulative_total}`; `ConversationCostSummary` per turn and per subagent; consumed credits per step range. [SRC] `jetski_cortex.proto.txt:81-103`.

### 3.6 Conversation annotations
Title, tags, archived, starred, unread, group, pinned, active profile; mirrored by a thin public-shaped `ConversationService` (`jetski.product.v1`). [SRC] `jetski_cortex.proto.txt` (ConversationAnnotations), `product_api conversation_service`.

## 4. Protocols worth knowing

| Protocol | Shape | Note |
|---|---|---|
| Connect RPC over protobuf | `LanguageServerService` 305 RPCs, server streams for state, terminal output, audio | Loopback HTTPS with a CSRF token. [SRC/REA] |
| Remote Control | LS opens `ApiService.ConnectInstanceV2` (client-initiated bidi stream); broker sends `ProxyRequestV2 {request_id, rpc_path, headers, payload}`; LS answers `ProxyResponseV2` with `is_final`; instances have CONNECTED/DISCONNECTED/IDLE; mesh sessions use STUN/TURN signalling and web-crypto public keys; web push is registered | Outbound-only tunnel; the web UI calls the same RPC paths. [SRC] `devtools__jetski__boq__api__proto__api.proto.txt`. INF (medium): the language server is the host-side peer, because it serves the proxied RPC paths and exposes GetRemoteControlInfo; the code path that opens the stream was not traced. |
| Unified state sync | topic/row store with `e_tag` per row and applied-update acks | Settings, custom models, browser allowlist, planning mode. [SRC] `unified_state_sync.proto.txt` |
| Hooks JSON | stdin JSON in, stdout JSON out; events SessionStart, PreToolUse, PostToolUse, PreInvocation, PostInvocation, Stop; decisions allow, auto_approve, deny, ask, force_ask, deny_unless_prior_grant; inject steps; prompt hooks with their own model | [SRC] `hooks.proto.txt`; [DOC] docs hooks |
| Sidecar manifest | `sidecar.json {command | builtin(schedule), args, env, restart_policy, display_name}`; `agentapi new-conversation / send-message`; UI entry FULL_PANE or AUX_PANE through `plugin://` | [SRC] `cortex.proto.txt:451`; [DOC] docs sidecars |
| MCP | stdio, Streamable HTTP/SSE, OAuth, Google credentials, per-tool enable lists, lazy load, background mode, elicitation | [SRC] `cortex.proto.txt:2878-3036` |

## 5. Agent-first UX, in depth

### 5.1 Surfaces and layout
Sidebar of projects, workspaces and conversations (group by workspace or project; sort by last updated, last prompt, date added; filters for pinned, unread, last 7 days, scheduled, extensions); a conversation view; an auxiliary pane for artifacts, files, diffs, browser subagent viewer, terminals, web preview and sidecar UIs; horizontal and vertical splits; a command palette with typed pickers; a shortcut overlay with 58 bindings and macOS overrides. [SRC] features 006, 111-118; `SP/tmp/ag2/shortcuts.txt`.

The hub is explicitly a manager: standalone quick conversations run in an isolated scratch folder; project conversations run Local or in a new git worktree; many agents run in parallel. [DOC] docs features, projects.

### 5.2 The plan, review, execute, walkthrough loop (the central UX idea)

```
 user prompt
    |
    v
 agent (planning prompt: research without source edits)
    |  decides whether the request warrants a plan (architectural change, heavy research,
    |  real ambiguity, deviation from approved plan) or is investigatory/trivial
    v
 writes implementation_plan.md  (ArtifactMetadata: type, request_feedback=true, user_facing=true)
    |  sections: goal, user review required (alerts), open questions,
    |  proposed changes grouped by component with NEW/MODIFY/DELETE file markers, verification plan
    v
 notify_user(review_uris, blocking, confidence_score + justification, should_auto_proceed)
    |
    +--> review policy ALWAYS: stop, user comments on selected text / diffs, Proceed or Review
    +--> review policy AUTO / TURBO: proceed (autonomous mode also removes questions and stops)
    v
 comments + approval status arrive as typed fields on the next user step
    v
 execute (task_boundary groups work: mode EXECUTION) ... verify (mode VERIFICATION)
    v
 walkthrough.md (changes, tests, validation; screenshots and browser recordings embedded)
```
[SRC] planning templates in the binary (`SP/tmp/ag2/tmpl/t007.txt`, `t008.txt`), `CortexStepNotifyUser`, `ArtifactComment`, `AgentMode`; [DOC] docs implementation-plan, walkthrough, artifact-review.

Observations:
- **The gate is a data contract, not a chat convention.** The request that carries a user's comments has typed fields for artifact comments (selection or whole file, approval status APPROVED / REQUESTED_CHANGES), file-diff comments and file comments. That lets the UI batch comments and lets the agent treat them as instructions on specific text. [SRC] `cortex.proto.txt:2342-2360`; `codeium_common.proto.txt` (ArtifactComment). Reverting a turn that carries such comments is disallowed in the UI.
- **Plan only when warranted** is stated as a rubric in the prompt, with explicit non-plan cases (explanations, trivial edits, minor follow-ups). That avoids plan fatigue. [SRC] `t008.txt`.
- **Open questions live in the plan**, not in an ask-question call, so the user answers by commenting. [SRC] `t008.txt`.
- **The agent reports confidence.** `notify_user` carries a confidence score with a justification and a should_auto_proceed flag; the artifact review mode decides what to do with it. This is a self-report and weakly calibrated, but it is a useful extra input to a gate. [SRC] `cortex.proto.txt:3297`.
- **In this build planning mode is partly retired.** The UI copy says planning mode is no longer supported and `/plan` replaces it, while the artifact templates and `PlanningMode` setting still ship; the docs still describe Planning mode as the main source of artifacts. Treat the flow as `/plan`-initiated plus a model-chosen plan in agentic mode. [SRC] `ui_copy.txt` (planning-mode notice), `cortex.proto.txt:1365-1368` (`use_slash_plan`); [DOC] artifacts page.

### 5.3 Task groups and progress
The model declares tasks with `task_boundary` (name, status, summary, delta summary, mode) and is nudged by reminder thresholds when it works without an active task. The UI groups work: overview of goal and changes, edited files, collapsed subtask progress updates, and a dedicated section at the end collecting steps that need approval so the user does not open every update. [SRC] `cortex.proto.txt:1243,3288`; [DOC] docs tools (Task groups). This is the best answer seen to "what is the agent doing and what needs me".

### 5.4 Subagents, teams and background work
- `SubagentSpec {type_name, role, initial_prompt, workspace (inherit | branch | workspace_uri | share), model, ModelTier, run_as_task, fork}`; tool config with fast and heavy models, a nesting limit, task mode and fork permission. Docs: nesting up to 10, a subagent starts with a clean context but inherits permission scopes, built-ins are research, browser and self. [SRC] `cortex.proto.txt:3366,1084`; [DOC] docs subagents.
- Agent-to-agent messaging with priority and delivery strategy; pending messages stream to the UI. [SRC] `cortex.proto.txt:3407`.
- Background tasks and commands are state: manage_task, SendStepsToBackground, a fully-idle flag on every update, MCP tools with background mode. [SRC] `cortex.proto.txt:1898`, `jetski_cortex.proto.txt:9`.
- `/boost` (orchestrator plus implementation, investigation and verification subagents) and `/teamwork-preview` (Sentinel, orchestrator with successor per milestone, explorers, workers, critic, challenger, auditor, success auditor; request, plan and progress artifacts) are documented long-horizon modes. The binary carries about 30 `teamwork_preview_*` role identifiers and swarm/blackboard conventions. [DOC] docs boost, teamwork; [SRC] `SP/tmp/ag2/ls_tool_and_role_names.txt`.

### 5.5 Browser subagent
A dedicated subagent with its own model, context policy (task only or task plus trajectory summary), suggested tool-call budget, reminder modes (verify screenshots, verify completeness, custom), scratchpad and recording. Tool set modes: all tools, pixel only, DOM input with pixel output. After each action it returns a compact state diff. DOM reads return only interactive elements in the viewport with indices. Actions need separate permission for page access and for acting; JavaScript execution has its own policy. A server-side denylist (fail closed) and a user allowlist (starts with localhost, extended with an Always allow prompt) gate navigation. Sessions are recorded to WebM by a helper process and attached as artifacts. [SRC] `cortex.proto.txt:1097-1200,2632`; [DOC] docs browser pages.

### 5.6 Review and feedback loops
- Artifact and diff comments (5.2); hunk, file and all-scope accept/reject with written feedback on code edits; turn-level undo with a diff preview; a Changes Overview bar. [SRC] features 013, 032, 073.
- Message feedback with fixed reason sets (positive and negative) and Best-of-N pairwise preference with up to two reasons; the model header can show an experiment label with feedback and opt-out. [SRC] features 080, 047, 090.

### 5.7 Attention model
OS notifications with two sounds (action required, done), a tray icon with running-agent count, quit confirmation when agents are active, a sidebar attention state, web push for remote sessions, and an update guard that lists active work. Settings choose when to notify. [SRC] features 114, 127.

### 5.8 Onboarding and tips
First-run wizard in the shell; in-app pages for auth options (Google, business account, Cloud project, SSO), theme, import (including Cursor rules and AI Studio projects), keybindings, migration scope, setup; post-onboarding steps (manager welcome, usage mode, agent configuration, add workspace); Build with Google bundles; server-driven tips with triggers, locations and priority. [SRC] features 057, 115.

### 5.9 Settings
Presets plus rules for permissions; artifact review policy; terminal sandbox and network; browser (tools on/off, JS policy, Chrome path, profile path, CDP port); queued message delivery; verbose chat; conversation width; telemetry; notifications; Remote Control; custom models; experimental features; customizations (skills, rules, MCP, hooks, agents, plugins) with a token-budget meter; per-project overrides with Inherit General and organisation locks. [SRC] features 103, 117.

## 6. Models and Gemini integration

### 6.1 What is observable
- **Lineup (public docs, medium confidence):** Gemini 3.8, 3.7 and 3.6 Flash with Fast/Low/Medium/High thinking levels; Gemini 3.1 Pro (Low, High); Claude Sonnet 5.5 and Opus 5.5 (Thinking) for paid tiers; older Claude 4.6 and GPT-OSS-120B scheduled for removal; an image model used by the image tool. Selection persists per conversation and a mid-run change applies after the current turn. [DOC] docs models.
- **Provider plumbing (binary):** `APIProvider` GOOGLE_GEMINI, GOOGLE_VERTEX, ANTHROPIC_VERTEX, OPENAI_VERTEX, GOOGLE_EVERGREEN, INTERNAL; `ModelProvider` Antigravity, OpenAI, Anthropic, Google, xAI, DeepSeek, GLM, Kimi. INF (medium): Claude and GPT-OSS are served through Vertex partner endpoints, consistent with the enterprise docs (third-party models need admin opt-in and run in global, us or eu locations). [SRC] `codeium_common.proto.txt`; [DOC] docs enterprise.
- **Endpoints:** the shell starts the LS with the Generative Language API as `--api_server_url` and a Cloud Code endpoint; `GetLoadCodeAssist`, `GetAvailableModels` and `RetrieveUserQuotaSummary` pull entitlement, model list and quota. [SRC] `inv` section 2c; `language_server.proto.txt`.
- **Agent lineage (INF, medium):** package names `exa.*`, `codeium_common_pb`, "Cascade" and Windsurf-era aliases (SWE-1, "windsurf" model strings) indicate the loop descends from Windsurf/Codeium's agent. This explains the model-agnostic harness (Llama/Hermes/XML tool formats, DeepSeek templates).

### 6.2 Design points that matter
1. **Server-pushed catalogue.** Each entry carries label, provider, description, beta/premium/recommended, allowed tiers, supported mime types, credit multiplier, quota fraction with reset time, and tags. Groups and sorts are server-defined; aliases (Auto, Recommended) resolve server-side with a version id. [SRC] features 081, 140.
2. **Capability flags drive the harness.** `ModelFeatures` (images, pdf, video, tool calls, thinking, adaptive thinking, raw thinking, thought circulation, deferred tool loading, mime types) plus tool-format and prompt-template enums. Behavioural prompt guidelines are annotated with the model and date that motivated them so they can be pruned. [SRC] features 083, 092; `t003.txt`.
3. **Role-to-model tiers.** Planner, subagents (INHERIT, FLASH_LITE, FLASH, PRO), checkpointing, knowledge generation, memory, guardian, fast-apply edits, terminal input detection and browser subagent each have a model slot. [SRC] feature 084.
4. **Thinking controls.** Per-model thinking level and budget, thought signatures kept on the step for replay, optional circulation of thoughts. [SRC] feature 092.
5. **Quota and credits.** Per model-family five-hour and weekly quotas, overages for paid tiers, per-step consumed credits, credit multipliers per model, estimated USD per token category with a comparable-rate fallback marked as estimated. [SRC/DOC] features 087-089.
6. **Custom models.** Only Gemini-API-scheme URLs with declared features and a compaction threshold; BYOK and bring-your-own-endpoint are not supported on consumer plans. Local and OpenAI-compatible models exist in the SDK only. [SRC/DOC] features 085-086.
7. **Experiments.** Remote config overrides (global, plan-based, default model, memory, view-file tool) and a "Next" label for rotating model configurations with opt-out; experiment ids are stamped on steps, executions and metrics. [SRC] feature 090.
8. **Gemini-specific touches.** Web-search grounding provider is a Gemini model; image generation uses a dedicated image model; an experimental computer-use model id exists but only browser pixel tools use it; transcription is a server RPC. [SRC] features 017-018, 022, 071.

### 6.3 What is not there
No Ollama or LM Studio option in the app, no desktop-wide computer use, no BYOK on consumer plans, no model comparison outside Best of N. Image and video understanding depend on the selected model's declared mime types.

## 7. Transparency and control

- **Trajectory debug view:** zoomable step timeline (collapse idle gaps, colour by kind or type), step inspector with generator metadata (TTFT, streaming, tool-call generation, queue wait, tool execution), system prompt sections with token type, message history, cascade config, parent and child conversations, battle-mode info. Invocation metadata is pruned except for the latest. [SRC] feature 093; `ui_copy.txt` debug strings.
- **Visibility flags:** the inspector states whether a step is shown to the user, sent to the model, both or neither. [SRC] feature 094.
- **Token breakdown:** system prompt, tools, rules, MCP tools, skills, subagents, messages, workflows, plugins, plus unused capacity; a customization budget meter. [SRC] features 038, 096.
- **Decision provenance:** auto-run decisions record user, system, model or default; permission prompts record why they appeared (effective grant, default grant, cross-project isolation, secure mode, default fallback, hook). [SRC] feature 097.
- **Hook activity** is visible live and stored per step. [SRC] feature 102.
- **Cost and timing** per turn, per subagent, cumulative. [SRC] feature 088.
- **Export and support:** Markdown export, shareable links, debug bundle, flight recorder and pprof RPCs, replay RPCs. [SRC] features 008, 098-099, 137.
- **Telemetry** (opt-out setting) records full trajectories, step feedback, metrics with a better-direction flag, monitor signals with provenance (rule-based, offline, background eval). [SRC] features 100-101.

## 8. Safety, permissions and governance

- **Presets:** Request Review, Default, Vetted (beta), Turbo; each maps to a bundle of sandbox, auto-execution, non-workspace file access and preset id. [SRC] feature 103.
- **Grammar and scopes:** `read_file`, `write_file`, `read_url`, `execute_url`, `command`, `unsandboxed`, `mcp`; deny wins; compound commands with substitution disable prefix matching; always-allow is refused for dangerous binaries and subcommands; scopes ONCE to GLOBAL. [SRC/DOC] feature 104.
- **Guardian:** a separate model with vet_data_flow, vet_content, vet_dependency, vet_shell_change, vet_deletion, vet_url and vet_denied_repeat tools, fail mode OPEN or CLOSED, a per-command risk table. [SRC] feature 105.
- **Sandbox:** standard vs bypass mode with explicit approval; OS isolation per platform (docs); network off by default; read_url domains join an outbound allowlist. [SRC/DOC] features 064-065.
- **Browser:** two-layer URL control, separate page-access vs action permissions, JS policy. [SRC] feature 061.
- **Org controls:** locked settings, disabled MCP/browser, non-removable domain rules, SSO/WIF, licences by project and region, data residency for partner models. [SRC/DOC] feature 109.
- **Injection posture:** warnings next to autonomy settings; Always Proceed is called out as the highest-risk review policy; the guardian looks at data flow. There is no dedicated injection classifier. [SRC] feature 108.
- **Shell hardening (scout):** contextIsolation on, loopback-only bridge with constant-time token compare. Weak points: CSRF and bridge tokens passed as command-line arguments, full shell environment inherited by agent processes, Electron fuses at defaults, GitHub token stored in a plaintext dotfile. [SRC] `inv` section 3.11; feature 058.

## 9. What is best in class, ranked for Verevon relevance

1. **Review comments as typed inputs plus an approval gate with confidence** (013, 031, 032). Highest value; fits Verevon's plan card.
2. **Typed step trajectory with paged delta stream** (001, 027). It underlies everything else and maps onto session-core events.
3. **Task boundary plus task-group UI with pending approvals hoisted** (029, 120).
4. **Permission grammar, presets, scopes, trigger provenance** (097, 103, 104).
5. **Transparency stack: debug timeline, visibility flags, token breakdown, cost by category** (088, 093, 094).
6. **Cost-tiered model roles and capability flags** (083, 084).
7. **Checkpoint schema and turn-level undo with preview** (037, 005).
8. **Browser subagent mechanics: state diffs, tool-set modes, two-layer access, recordings** (059-062).
9. **Hooks contract with step injection and prompt hooks** (040).
10. **Best of N with preference capture** (047, 091), optional but differentiating.

## 10. Weaknesses and limitations

- **Local, single-user, Google-account product.** No multi-tenant model, no org-scoped sessions; Remote Control only reaches your own machines and requires the host to stay online and awake. [DOC] docs remote-control, FAQ.
- **Large Google-internal residue** in a public build (CitC, Piper, Fig, Cog, ABFS, Blaze, internal search, internal agent binaries, internal feature flags); dead code and private identifiers ship in both the bundle and the language server. [SRC] feature 126.
- **Planning mode half-retired**, strict mode half-retired (docs page 404 while UI copy remains), legacy command lists replaced by permissions v2, Windows still on older settings and a preview sandbox. [SRC/DOC] features 030, 106, 108.
- **Opaque limits.** Five-hour and weekly quotas have no published numbers; overage billing detail is thin. [DOC] docs plans.
- **Privacy surface.** Share links and telemetry upload trajectories; `has_ever_used_non_google_model` is tracked per execution; no zero-data-retention concept was found. For Verevon ZDR this is the main thing not to copy. [SRC] features 008, 101.
- **Accessibility and i18n.** English-only copy; viewport disables pinch zoom; reduced-motion honoured in only a couple of components. [SRC] feature 121.
- **Sidecars and automations** run user code with no documented sandbox. [DOC] docs sidecars.
- **Model-self-reported confidence** is weakly calibrated. [INF]
- **Surface bloat.** 2,989 proto names, 467 messages in one file, 305 RPCs in one service: the opposite of Verevon's one-owner-per-capability rule. [SRC]

## 11. Gaps versus Verevon, and what to do

Verevon state is taken from `findings/t3/v-chat-agents-ui.baseline.md` and `v-modelplane.baseline.md` (code is ground truth there). Section references are to those files. Not re-verified in this pass.

| Capability | Antigravity | Verevon today (baseline) | Gap | Adopt / adapt / avoid |
|---|---|---|---|---|
| Typed step record with per-step status | Core data model | session-core append-only events; UI derives nodes from SSE (s4, s13) | Medium | Adapt: add typed step status and indices to run events; keep Postgres events |
| Paged delta stream | Slices per collection, verbosity tiers | SSE with Last-Event-ID resume, full-message reparse, no virtualisation (s1, s12) | Medium | Adopt slice contract for `/runs/{id}/events` and thread views |
| Plan gate and review | Plan artifact, comments, confidence, policy | Editable plan card with authority rung and approval; artifacts have versions and diff but no editing or comments (s1, s4) | Medium | Adapt: add artifact comment inputs and a confidence field; keep rungs |
| Task boundary and task groups | Model-declared, UI groups, pending approvals hoisted | Plans and todos from orchestration; Work tab (s4, s10) | Small to medium | Adopt model-emitted boundary events and grouped Work view |
| Checkpoint, rewind, fork | Per-turn snapshot, revert preview, fork at step | None; branch is browser-only (s4, gap 8) | Large | Adapt: thread-level checkpoints in session-core; effects are provider actions, so show receipts not diffs |
| Best of N | Parallel arms, winner, preference | No comparison (s9) | Large (optional) | Adapt later; use sandbox leases and router-policy data |
| Hooks | Six events, JSON, step injection, prompt hooks, UI status | Engine real, not applied in Do, in-memory registry (gap 4) | Medium | Adopt the contract; store hook results on steps |
| Subagents | Spec with workspace, tiers, run_as_task, depth cap 10 | Depth 1, isolated history, up to 8 children (s4) | Medium | Adapt: add model tier and run_as_task; keep depth small |
| Permission grammar and presets | `action(target)`, scopes, presets, guardian, trigger provenance | Autonomy rungs, capability policy, substring risk classifier (s11, gap 14) | Medium | Adopt grammar, presets, provenance; add a fail-closed guardian with deterministic tools |
| Browser subagent | Own model, recordings, state diffs, two-layer access | Quarry executes; agent runs fail closed on grants; no browser from chat (s6, gap 5) | Large, mostly reachability | Fix the grant first; then adopt state diffs and read-vs-act permission |
| MCP | Full | Registration and OAuth only; calls denied in both loops (gap 1) | Large, reachability | Finish execution; copy enable/disable lists and lazy load |
| Skills, rules, plugins | SKILL.md, four rule modes, marketplace, token budget | Skills CRUD; instructions only in Ask; plugins inert (s5, gap 2, 11) | Medium | Adapt: bring rules into Do with a budget; skip marketplace until plugins run |
| Scheduling | Cron agents with permission review | One no-tool call, no delivery (gap 6) | Large | Adopt agent-run schedules with explicit grants and a visible event log |
| Model catalogue | Server-pushed with quota, tiers, credit multiplier, status | Catalogue with cheap flag and privacy badges (s9) | Small to medium | Adopt quota fraction, multiplier, status banners |
| Gemini and model breadth | Native Gemini, Claude via Vertex, GPT-OSS | No Gemini provider (gap 9) | Large breadth | Add a Gemini adapter only with capability flags; keep tier and residency routing |
| Cost and transparency | Cost by token category and subagent, debug timeline, visibility flags | Per-answer cost; trace; context inspector; no live meter (s10) | Medium | Adopt per-step cost fields and visibility pairs; gate prompt views by role |
| Notifications | Two-tier attention model, tray, push | Notify-when-done watcher (s4) | Small to medium | Adopt needs-input vs finished with sound and badge; PWA push |
| Command palette and shortcuts | 58 shortcuts, typed pickers, overlay | None (s12) | Small | Adopt (UI only) |
| Share, export, import | Share link, Markdown export, Cursor import | None (s1) | Medium | Adapt as org-scoped signed read-only snapshots; ZDR-aware; import ChatGPT/Claude exports |
| Remote control | Outbound tunnel to a broker | Not needed for cloud chat | None | Use the pattern only for customer-hosted runners |
| Voice | Streaming dictation with cleanup | One-shot dictate and TTS (s2) | Small | Adopt chunked streaming if cheap; skip always-on listener |

Where Verevon is ahead (do not regress): ZDR propagation and privacy-tier routing, tenancy and per-audience tokens, durable approvals with proof bundles, deterministic confidence and verification passes, honest-cost rule (no fabricated cost), result receipts, KB grounding with citations. Antigravity has none of ZDR, tenant isolation or residency routing beyond enterprise licence regions. [baseline s11 and Model Plane verdict]

### 11.1 Suggested adoption order (Model Plane first)
1. **Typed step status and indices** in run events (session-core), then the slice contract on the gateway stream.
2. **Hooks into Do mode** with the six-event contract and step injection; store hook results on steps.
3. **Permission grammar and presets** in capability-core, with trigger provenance and scope ladder; add a guardian as a deterministic pre-check.
4. **Review comments** as typed input to a run (artifact selection, diff, file) plus `confidence_score` on plan review.
5. **Checkpoint and rewind** at thread level; show reversible effects only.
6. **Model roles and tiers** (planner, subagent, checkpoint, guardian, browser) with per-role cost fields.
7. **Transparency**: step timing split, visibility pairs, token breakdown including truncation reason.
8. Then browser subagent mechanics (state diffs, pixel-only mode), schedules as agent runs, and optional Best of N.

### 11.2 Avoid
- Shipping internal features inside customer builds behind flags.
- Storing tokens in plaintext dotfiles or passing them as command-line arguments.
- Default telemetry of full trajectories; any share feature that bypasses ZDR.
- A 305-RPC god-service; prefer plane-owned small APIs and the existing capability ownership matrix.
- Treating model self-reported confidence as a gate on its own.

## 12. Rust-portable design notes
- **Step model:** `enum StepPayload` with one variant per tool family, `StepMeta` with status transitions and timings, `Trajectory { steps: Vec<Step> }` persisted as rows keyed by (run id, index). Keep payloads as protobuf or serde enums; add `render_info` for plugin steps.
- **Delta stream:** `struct CollectionDelta<T> { indices: Vec<u32>, items: Vec<T>, total: u32, bounds: Option<Slice> }` per collection; server streams over axum SSE or tonic; client subscribes with `subscriber_id` and `verbosity`.
- **Permissions:** `struct PermissionResource { action: Action, target: String }`; `GrantSet { allow, deny, ask }` evaluated deny > ask > allow with prefix and regex matchers; compound-command detector; `Scope` ladder persisted per run, thread, Space, org.
- **Hooks:** subprocess with JSON on stdin/stdout, timeout default 30 s, a `HookDecision` enum, injected steps as `Vec<StepInjection>`; run hooks in execution-core's existing hook stage.
- **Guardian:** a small agent with a closed tool list (data flow, content, dependency, env change, deletion, URL, repeat-deny), `FailMode::{Open, Closed}` default Closed, result cached by command hash.
- **Checkpoint:** `Checkpoint { intent, session_summary, change_summary, artifact_snapshots, running_tasks, subagents, memory_summary }` produced by a role-model; replace history prefix, keep `compacted_at_step_indices`.
- **Browser:** keep Quarry as executor; add `StateDiff` in observations and `ToolSetMode`; separate `read_url` and `execute_url` grants.
- **Remote runners:** outbound-only bidi stream carrying `{request_id, rpc_path, headers, payload, is_final}`; instance registry with status and last-seen; optional WebRTC mesh later.
- **State sync:** topic/row with `e_tag`; compare-and-set on write; subscription per topic.
- **Embedded UI:** a Rust server can embed a built SPA zip (`include_bytes!` or rust-embed) and serve it with a CSRF token on loopback for desktop runners.

## 13. Open questions and what was not verified
- Exact current model ids and per-tier access (docs summary only; binary model enum is old).
- Whether Concierge, agent team, strict mode and the Vetted preset are available to external users (all are gated in the bundle).
- Telemetry defaults and what leaves the machine in the external build; trajectory share server-side retention.
- Sandbox implementation on Windows (helper present; mechanism unverified) and macOS/Linux details beyond docs.
- Sidecar trust model; plugin signing; marketplace curation (`trust_level` semantics).
- The behaviour of `THOUGHT_REWRITE` fork reference; semantics of `AgenticModeConfig` artifact reminder thresholds.
- How the UI binds comments to selections at runtime (selection target schema is known; UI code not traced).
- REA could not build a complete semantic graph of main.js (heap limit); a later pass with a larger heap or a split bundle could trace IPC-like RPC callers.

## 14. Files produced
- `SP/findings/t3/antigravity.agent-ux-and-gemini.features.json` (143 records) and this file.
- Scratch (reproducible): `SP/tmp/ag/ui` (extracted UI), `SP/tmp/ag2/protos` (decoded descriptors), `SP/tmp/ag2/tmpl` (Go-template prompt sections), `SP/tmp/ag2/shortcuts.txt`, `SP/tmp/ag2/ls_tool_and_role_names.txt`, `SP/tmp/ag2/jsonschema_desc.txt`, `SP/tmp/ag2/snippets.txt`, scripts `descs.cjs`, `tmpl.cjs`, `jsdesc.cjs`, `keys.cjs`, `build.cjs` and the `feat*.cjs` record sources.
- Secret-bearing files skipped: everything under `AppData/Roaming/Antigravity` and `~/.gemini`. No secret values were read, printed or recorded.
