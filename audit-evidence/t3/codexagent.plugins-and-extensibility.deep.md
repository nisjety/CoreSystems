# Codex desktop app: plugins, skills, MCP, automations and the agent runtime (deep dive)

Audit 3, facet "plugins-and-extensibility". Source: OpenAI Codex desktop app 26.1007.21434 (MSIX OpenAI.Codex_26.1007.2314.0_x64), Electron shell plus a Rust agent runtime (codex.exe, runtime string 0.162.0-alpha.1). Read-only analysis; nothing was run, installed or modified. Companion file: codexagent.plugins-and-extensibility.features.json (127 records, ids codexagent-plugins-and-extensibility-001 to 127).

Provenance tags used throughout: SRC = read directly in shipped source, manifests or printable strings of binaries; DOC = prose in SKILL.md or docs that ship with a plugin; REA = observed through the REA tool; INF = my inference (basis stated). Confidence in the JSON: high / medium / low.

## 0. Evidence key and method

- PLUG = resources/plugins/openai-bundled/plugins under C:/Program Files/WindowsApps/OpenAI.Codex_26.1007.2314.0_x64__2p2nqsd0c76g0/app. RES = the same app/resources folder.
- EXE-STR:Lnnn = line number in scratchpad/tmp/cxpe/codex_exe.strings.txt, a dump of printable ASCII runs from resources/codex.exe (334 MB Rust binary). The dump contains repeated copies of some literals, so a line number points at one of several occurrences. String order is concatenated, so field lists are reliable as sets but not as schemas.
- BUILD:file@N = character offset in extract/codex/.vite/build/file (the minified Electron main-process bundle).
- I18N:key = scratchpad/tmp/cxib/i18n_all.txt, an English renderer string catalogue extracted by the sibling browser audit (2,870 keys, not the full catalogue).
- The single most valuable source was unexpected. plugins/code-review/server.mjs (and codex-app-tools/server.mjs) are unminified esbuild bundles that still carry source-path comments. They embed the app's own `protocol` package (about 280 modules): Zod schemas for settings, automations, runtimes, plugins, MCP Apps, notifications and permissions. That gives near-source evidence for the desktop side. The Rust side is known from strings only.
- REA usage (real calls): analyze_javascript_application on plugins/code-review (directory, detail summary, record-and-continue, 2 GB heap; 5 files, 17 MB text, 3.3 M AST nodes, 858 modules, partial graph because the worker ran out of heap on server.mjs and pull-requests.js); trace_application_feature twice on that graph (seeds "resources/read" and "ui/open-link": no graph match, which REA states is not proof of absence); inspect_analysis_view (modules page). Parent evidence ev_f85b8f51d3a51eacd16f2fe2f7ef4b49b5261e61e4333809753f644f6fbb2786, summary evidence ev_4eb1f8d6d6c18c428fdb9468c0dbbf46c4533592280411440ed06583e6d92e54. The scout's earlier REA runs on .vite/build and PE resources are recorded in codexagent.inventory.md. REA added little beyond reading the files: its semantic graph does not reach inside these large bundles, and its string-seeded traces missed literals I could read directly.
- Not read, by rule: any per-user data under AppData, the user's ~/.codex, cookies, tokens, keychains. No secret-bearing file was opened; client identifiers in strings (Sentry, Statsig) were not recorded.

## 1. Architecture in one page

1. Three layers. (a) An Electron shell (main process plus a React-style renderer, 19,961 asset files) that is a thin client. (b) A Rust runtime, `codex app-server`, that owns threads, turns, tools, sandboxing, memory, skills, hooks, MCP and plugin loading. (c) Out-of-process helpers: a V8 code-mode host, a command runner, a Windows sandbox service, a Node REPL (cua_node) and MCP servers.
2. One protocol. The renderer, the terminal UI, the VS Code integration and the in-app browser all speak the same JSON-RPC "app-server" protocol (SRC; EXE-STR:L1296258, L1298001). Transports: stdio, a unix control socket with WebSocket upgrade, and WebSocket with capability-token or signed-JWT auth (see the transport record in the JSON). Clients negotiate capabilities at `initialize` (experimentalApi, optOutNotificationMethods, mcpServerOpenaiFormElicitation, extensions) and choose which of roughly 150 notifications they handle.
3. Features are "extensions" inside the runtime. The source paths show ext crates for skills, goal, guardian-v2, mcp, memories, queue, image-generation, web-search, git-attribution, connectors and agent-message-board, built on an extension-api with contributors, a registry and typed per-session state (SRC; rec "Rust extension architecture"). Core stays small; features contribute tools, prompt fragments, hooks and "world-state" updates.
4. Configuration is a typed layer stack (packaged defaults, system, user, project, MDM, session flags, plugin, cloud requirements, cloud managed config, legacy files) with a batch-write API and a debug view. Admin "requirements" constrain every security-relevant choice (rec on enterprise requirements).
5. State is local SQLite plus JSONL rollouts: threads, turns, items, goals, queued messages, typed thread artifacts, per-thread dynamic tools, spawn edges, lease-based jobs, a separate agent message-board database, and desktop-side SQLite for automations and inbox.
6. The desktop app is also the plugin host and the configuration orchestrator: it copies the bundled marketplace into CODEX_HOME, enables or disables bundled plugins by rewriting their .mcp.json atomically, injects the node_repl server into each thread's config, and passes plugin-specific config overrides to the app-server (BUILD:main-BklS_2Y2.js@101227, @178465).

## 2. The plugin system

### 2.1 What a plugin is

A plugin is a folder with `.codex-plugin/plugin.json` (SRC; nine bundled examples). The manifest carries marketing metadata (displayName, descriptions, brandColor, icons, screenshots, defaultPrompt suggestions, privacy and terms URLs, developer name, category), a capability label set (Interactive, Read, Write), a `skills` directory, an `mcpServers` file pointer and an inline `hooks` object. Beyond that the runtime knows about app connectors, app templates, scheduled-task templates and an onboarding skill (plugin summary fields hasSkills, hasMcps, hasApps, hasHooks, appTemplates, scheduledTasks, onboardingSkill; EXE-STR:L1289331). The Rust loader also accepts the open "Agent Plugins" format (root plugin.json with a `$schema` from agent-plugins.org and a namespaced `extensions` map), and skill discovery recognises `.claude-plugin` and `.cursor-plugin` manifests. That cross-vendor tolerance is the clearest strategic signal in the package: Codex positions its plugin loader as a superset reader of other agents' layouts, and ships an importer for Claude Code setups.

Bundled set: browser, chrome, computer-use, unified-computer-use (infrastructure), code-review (an MCP App), codex-app-tools (host bridge), deep-research and visualize (skill-only), latex (skills plus scripts and tests). Several of the nine are mostly skills and documents with little or no host code of their own (deep-research, computer-use, visualize, unified-computer-use), which shows how far a "skill + doc" plugin can go.

### 2.2 Marketplaces and sources

- Marketplace manifest `.agents/plugins/marketplace.json`: entries with a source (local path, URL, npm package, git subdirectory), an installation policy (AVAILABLE) and authentication policy (ON_INSTALL), and a category.
- Users add marketplaces (marketplace/add), upgrade them (marketplace/upgrade) and can be locked to an allow-list of sources by admin policy. OpenAI-owned names (openai-curated, openai-bundled, openai-primary-runtime and others) are reserved.
- Upgrades are transactional: stage, validate, move the old tree to a backup, activate, write an install record, restore on failure, and abort if the config changed mid-flight (SRC; EXE-STR:L1311380).
- npm plugins are fetched with `npm pack --ignore-scripts`, exactly one archive, a byte cap and a name check; git plugins use a trusted repository root inside CODEX_HOME with a scrubbed git environment (SRC; EXE-STR:L1311579, L1311571). This is a careful supply-chain posture that a typical "download a zip" marketplace lacks.
- Account-hosted plugins add a remote catalogue (/ps/plugins/list, search, installed, suggested, workspace/shared) with scopes GLOBAL, USER, WORKSPACE, workspace sharing to named principals, visibility tiers (directory, shared with me, private, unlisted) and sha256-checked bundle downloads.

### 2.3 Install, cache, signing and updates

- Installed plugins live at CODEX_HOME/plugins/cache/<marketplace>/<plugin>/<version>; remote ones carry a marker file with the remote plugin id so the cache can be reconciled.
- Bundled plugins ship twice: unpacked, and as signed archives (plugin.tar.gz with an integrity.json of per-file sha256, a detached RSA-SHA256 signature, a public key and a build receipt with CI run id and source sha). I found no code that verifies those signatures in the JavaScript bundles or in the Rust strings. Treat signing as present, enforcement unconfirmed (rec on signed archives).
- Updates for plugins and runtimes come from restricted CDN paths (https://chatgpt.com/codex/app/updates/plugins/ and /runtimes/) with sha256 pins and single-path-segment version validation. Heavy dependencies (Node, Python, the documents/presentations plugins) arrive as a separately versioned "primary runtime" bundle with per-platform manifests, release channels (latest, latest-alpha) and its own marketplace name.
- The host mutates plugin files at runtime (patching .mcp.json to enable a server only when prerequisites exist). It does so atomically, but a plugin directory that is not immutable is a weakness (section 9).

### 2.4 How the model learns about plugins

- When a plugin is available or explicitly selected with an @-mention, the runtime injects a "Capabilities from the X plugin" block: skills are name-prefixed, apps and MCP servers are listed, and for the request that selected the plugin the model is told to search its tools with tool_search before falling back to unrelated tools. The block is trimmed to fit context (EXE-STR:L1303806).
- If a user asks for a plugin that is not installed, the model has a governed install path: `list_available_plugins_to_install` and `request_plugin_install`. The tool description is restrictive: exact match to an explicit user request only, only after tool search failed, only for plugins in a `<recommended_plugins>` list, never in parallel, root thread only. The user accepts, declines or cancels in a card; declining is remembered (EXE-STR:L1305563).

### 2.5 Hooks and MCP inside plugins

Plugins declare hooks inline or in hooks/hooks.json. The hook engine supports twelve events (PreToolUse, PermissionRequest, PostToolUse, PreCompact, PostCompact, SessionStart with a source, SessionEnd, UserPromptSubmit, SubagentStart, SubagentStop, Stop, Interrupt) and handler kinds command, mcp_tool, prompt and agent (prompt and agent parse but may be unsupported). Hooks can add context, rewrite tool input and veto actions. The notable safety feature is the trust model: every hook has a source layer, a content hash and a status of managed, untrusted, trusted or modified, and admins can set "managed hooks only". Bundled plugins use the mcp_tool handler to call `turn_ended` on a persistent JavaScript REPL when a turn stops, is interrupted or a sub-agent stops, which is how browser and computer-use sessions get cleaned up.

Plugin MCP servers are loaded defensively: the MCP file must be a regular file inside the plugin root, stdio servers get a private .plugin-data directory, duplicates are ignored, and path escapes disable the MCP config.

### 2.6 What is missing

There is no per-plugin permission grant. The manifest's capabilities are labels; nothing declares filesystem, network or data scopes. Plugin MCP servers are ordinary child processes with an env_vars allow-list. Safety comes from per-tool approval modes, the sandbox policy, hook trust, admin requirements and, in a separate layer, the guardian reviewer. I flagged this as INF with medium confidence because I only see manifests and config schemas, not any cloud-side review.

## 3. Skills

### 3.1 Format and discovery

A skill is a folder with SKILL.md (front matter name and description, optional metadata.short-description) and an optional agents/openai.yaml carrying interface metadata (display name, icons, brand colour, default prompt), a policy (allow_implicit_invocation, products) and declared dependencies (tools and OAuth). Scripts, references, assets and tests sit alongside. Discovery scans the project, ~/.agents/skills, CODEX_HOME/skills and the plugin cache with ripgrep, capped at 512 results, 1 MiB per metadata file, bounded traversal and parallel parsing (BUILD:skills-8U-G2giZ.js@0).

### 3.2 A typed authority model

The Rust skills extension separates where a skill lives: host files, cloud packages, executor packages (inside a remote execution environment) and custom providers. The model gets two tools, `skills.list` (cursor-paged, per authority) and `skills.read` (a package plus an optional skill:// resource, with root aliases resolved from a "Skill roots" section). Reading a skill's own bundled files goes through the same tool, so skills in a sandbox work without mounting. Cloud skill reads abort if the cache changed mid-read.

### 3.3 Context budget and selection

The skill list the model sees is budgeted: descriptions are truncated, then dropped, and the model is told how many were omitted and that explicit mentions still resolve. Changes in availability are announced as "Host skills update" messages. Telemetry counts enabled, kept, truncated and truncated characters. Alongside this, a shadow-mode experiment evaluates retrieval over the catalogue with twelve method ids: LRU, fielded BM25, character n-gram, reciprocal-rank fusion, multi-query lexical, routing-card variants and a task-context fusion method, with script detection for CJK text. Only measurement was observed, so this is an experiment, not a shipped selector (EXE-STR:L1335967, L1335975).

### 3.4 Invocation policy and attribution

Implicit invocation is on by default and can be turned off per skill (the bundled review-agent is explicit-only). Every injection emits an event with skill, plugin, invoke type (explicit, implicit, unattributed), model and effort. Skill execution is a distinct approval category (`skill_approval`) in the granular policy.

### 3.5 Authoring doctrine and system skills

The embedded skill-creator is itself a good spec: disclose in three stages, keep descriptions discriminating, add scripts only for deterministic work, no README clutter, name rules, and the line "approval to complete a task does not expand its scope or execution permissions". Other embedded system skills: imagegen, openai-docs (answers questions about Codex itself), review-agent, skill-installer, documents, presentations, spreadsheets and pdf, installed into CODEX_HOME/skills/.system at startup. Skills can depend on other skills with a hard failure contract (deep-research stops if $documents is missing). Complex skills carry tests (latex, hatch-pet).

## 4. MCP, tools and code mode

- MCP client: rust-sdk based, stdio and streamable HTTP; protocol versions 2024-11-05 through 2025-11-25 plus a path that requires 2026-07-28 for input-required results; tasks, sampling, logging, subscriptions. OAuth covers dynamic client registration, portable client-id documents, resource metadata discovery, WWW-Authenticate scopes and credential invalidation when the issuer changes.
- Per-server and per-tool config: enabled, enabled_tools, disabled_tools, omit_tools_from (deferred, code_mode), startup and tool timeouts, env allow-list, output token limit, destructive_enabled and open_world_enabled gates, default and per-tool approval modes, required servers.
- Human-readable approval templates: 55 baked templates (schema_version 4) turn a connector tool call into a sentence with labelled parameters, for example "Allow {connector} to create a branch?" (EXE-STR:L1304554).
- MCP Apps: the app implements the ext-apps standard (HTML resource with profile mcp-app, ui/open-link, display modes inline, fullscreen and pip, app-only versus model-visible tools, CSP domain lists, theme variables, host context). The Code Review plugin is a full 13.9 MB UI delivered this way, rendered in a sandbox partition with a fixed list of host calls and telemetry for blocked navigation and downloads. Tool results may also declare "resource activities" (read, created, updated URLs).
- Elicitation: MCP servers can request forms, file pickers and image pickers through an OpenAI extension, with decline versus cancel semantics and auto-resolution timers, plus user-verification (passkey) elicitation.
- Dynamic tools: a client can register tools (functions or namespaces, optionally deferred) at thread start; they are persisted per thread and calls return to the client. codex-app-tools uses this to expose the desktop app (threads, projects, sidebar, automations, worktrees, panels) to the agent over a named pipe, with 4-byte-length-framed JSON-RPC and cancel support; the consequential tools (create_thread, send_message_to_thread, fork_thread, handoff_thread, move_thread_to_project, automation_update) prompt for approval.
- Agent-editable settings: a 96-entry typed settings manifest marks each setting hidden, read-only or read-write for the agent, with JSON Schemas exported to it (theme, fonts, composer behaviour, notification rules, detail level, usage display).
- Code mode: the model writes a JavaScript program (a single `exec` tool) that runs in a fresh V8 isolate with no filesystem, network or console, calling nested tools as async functions (MCP tools included), with store/load across calls, token-capped output and yield control. A separate V8 host process speaks a versioned protocol to the runtime. This is the biggest structural capability that Verevon lacks.
- Tool discovery: deferred tools are found with BM25-ranked `tool_search`, usable from code mode, so a plugin ecosystem does not blow up the tool list. Registry config handles collisions and schema size limits.

## 5. Commands, import and instructions

- About fifty slash commands in the terminal client (see the JSON record) and a desktop slash menu plus command palette with a keymap protocol. Notable ones: /goal, /side (ephemeral fork that cannot use tools), /import (from Claude Code), /memories, /skills, /plugins, /hooks, /mcp login, /debug-config, /review, /compact, /plan, /app (continue in the Desktop app).
- External agent import: detect and import AGENTS.md, config, skills, plugins, MCP servers, subagents, hooks, commands, memory and recent sessions from another coding agent, with optional continuous sync and a history table.
- Instructions: AGENTS.md with override variants and a 32 KiB cap, profiles, developer instructions, and the app-server returns instruction_sources so clients can show which files applied.

## 6. Agent runtime features relevant to extensibility

- Automations: TOML files plus SQLite run history; kinds cron (new thread each run) and heartbeat (wake an existing thread); RRULE schedules validated against a whitelist; per-automation model, effort, environment (worktree or local) and notification policy; deterministic per-install jitter; run contract with a per-automation memory.md and a mandatory single `::inbox-item{title,summary}` directive that feeds an Inbox. Plugins can ship scheduled-task templates.
- Multi-agent v2: spawn with fork_turns, send, wait, resume, close; depth and concurrency limits; spawn graph in SQLite; a separate message board (channels, posts, subscriptions with idempotent request ids).
- Goals: budgeted long-running objectives with automatic continuation and a strict blocked audit (three consecutive turns).
- Compaction, steering and queued follow-ups are configurable and surfaced as events and settings.
- Purpose-specific model slots: automations, ephemeral generation, ambient suggestions and a separate safety model for suggestions.
- Review as a first-class path: review-agent skill, review/start, and a Code Review MCP App with a structured findings schema (summary, impact, up to 50 findings with priority, path, line, side).

## 7. Safety and governance in the extension model

- Guardian v2: an asynchronous reviewer scores the trajectory and a "high" result turns later actions into blocking reviews. Trust rules: only user and developer messages, AGENTS.md and request_user_input answers are trusted evidence; tool output, skill instructions and plugin descriptions are untrusted; invoked user-owned skills are verified through a trusted message of canonical paths. An org policy slot is injected into the prompt. This is the most reusable safety idea in the package.
- Approval ladder: read-only, auto, granular, guardian-approvals, full-access, custom; granular splits sandbox, skill, permission-request and MCP-elicitation approvals; user answers allow once, allow this conversation, always allow or deny with a reason; approved prefixes become exec-policy amendments.
- Enterprise requirements file and a signed cloud config bundle (workspace-managed): allowed approval policies, sandbox modes, login methods, web-search modes, permission profiles, plugins, features, residency, managed-hooks-only, browser and computer-use switches.
- Passkey-based user verification (security key) as a step-up for sensitive approvals; MCP servers can request it.
- Documents as policy: the Computer Use confirmation taxonomy (four modes, numbered action classes) ships as readable Markdown that the skill forces the model to read first; the browser plugin gates documents by tool and capability through documents.json.

## 8. Artifacts and generative UI

- Artifact templates are skills named artifact-template-* with a JSON manifest (kind, reference file, preview) and a picker MCP server that shows up to ten thumbnails via elicitation, accepts an uploaded reference and can save it as a reusable template. The picker is paranoid about paths (realpath containment, 64 KiB metadata, PNG header and pixel caps, canonical Google URLs).
- Visualize is a skill-defined generative UI contract: HTML fragments only, 1 MB cap, CDN allow-list, theme tokens, a state bridge (widgetState up to 16 KiB with model-visible and private parts) and a confirmed follow-up bridge. The same host runs MCP Apps.
- Deep research and LaTeX are plugins made of skills, references and scripts. Hatch-pet shows a deterministic QA pipeline for generated assets.

## 9. Best in class, weaknesses, limits

Best in class (quality 5 in the JSON, 11 of 127): code mode with tool_search; hook trust by hash; human-readable approval templates; MCP Apps with a sandbox host; the app-server protocol and its transports; enterprise requirements plus a signed cloud bundle; the guardian reviewer; the approval ladder; defensive file handling in the template picker; the visualize output contract. Strong but rated 4 because only strings or documents were visible: MCP OAuth hardening, the agent-editable settings manifest, the automation run contract, external agent import, the skill-creator doctrine, the Code Review MCP App, the Computer Use confirmation taxonomy. Quality scale used: 5 = shipped, detailed, best-in-class design; 4 = solid and well evidenced; 3 = partial, experimental, unclear enforcement or weakly evidenced; 2 = weak execution.

Weaknesses and limits:
1. No per-plugin permission model; plugin directories are rewritten by the host at runtime.
2. Signature artifacts without observed enforcement.
3. Dynamic skill selection is experimental; the full catalogue is still budget-truncated.
4. Plugin and skill authoring docs are proprietary and spread across prompts embedded in a binary; no public manifest schema for the .codex-plugin format was found (the Agent Plugins schema is external).
5. Many features depend on the ChatGPT backend (remote plugin catalogue, cloud config bundle, connectors, cloud tasks), so they are not portable as-is.
6. Local SQLite and local files are the system of record; a multi-tenant web product needs server-side equivalents.
7. Internal codenames (daybreak, aeon, orbit, tortilla, cumberbatch, chronicle) mark features whose semantics I could not establish.
8. Batch CSV agent jobs existed (tables agent_jobs and agent_job_items) and were dropped by a later migration; the runtime moved to multi-agent v2.

## 10. What Verevon should copy, adapt or avoid

Mapped to the Verevon Model Plane and chat/agent baselines (code is ground truth): plugins registry inert; MCP tools cannot be invoked by the model; hooks absent in Do mode; slash commands minimal; skills injected by keyword (3 per turn); no code mode; scheduled runs are a single no-tool call; no importer; no command palette.

Copy (adapt to Verevon's rules):
1. Plugin package + marketplace pipeline: manifest with typed interface, multi-capability components, policy fields, signed archive with integrity manifest, staged upgrade with rollback, reserved namespaces, org source allow-list. Fit: Control Plane owns allow-lists; capability-core owns the registry; the "no host loads a manifest" gap closes by adding a loader in execution-core behind the capability gate.
2. Unblock MCP with governed bindings: per-tool approval modes, destructive and open-world annotation gates, human-readable approval templates, tool_search discovery, MCP Apps for UI. This directly addresses the "capability gate denies mcp__*" gap without weakening fail-closed behaviour.
3. Hook trust (hash plus source layer plus managed-only) and the full event set; feed hooks into Do mode.
4. Guardian-style asynchronous reviewer with an explicit trusted-evidence rule; use it in place of the substring risky-tool classifier. Watch ZDR: persisted scores and reviewer prompts must obey retention rules.
5. Skills: package format with sidecar policy, budgeted catalogue with omission signalling, typed authority locators (so learned skills and sandbox skills read through one tool), explicit-only flag, injection telemetry, authoring doctrine as the rubric for the learned-skill reviewer, tests in packages.
6. Code mode (roadmap 23.4): V8 isolate with no I/O, tools as async functions, store/load, token caps, separate host process with a versioned protocol; run behind execution-core.
7. Automations as full agent runs with a restricted RRULE whitelist, a run contract (memory file plus a structured inbox footer), jitter, heartbeat threads, and plugin-provided templates; this replaces the single-call scheduled step and feeds the Findings and Routines pages.
8. Admin requirements document (allow-lists plus residency) delivered signed from Control Plane and enforced inside the runtime.
9. Typed settings manifest with agent access levels; layered instruction stack with a "who set this" view; instruction_sources returned to clients.
10. Artifact templates and the visualize contract as the basis for Verevon artifacts and widgets; external agent import to ingest ChatGPT/Claude exports.
11. Dynamic tools with persistence and namespaces for client-supplied tools; thread-follower model for approvals from a second client.

Adapt with care:
- Remote plugin catalogue and sharing: map to org and Space principals; never let Space membership widen an owner-plane ACL.
- Memory pipeline: keep consolidation rate-limit aware and exclude externally sourced context ("polluted" flag), but respect ZDR.
- Model provider records with gateway OAuth: Integration Core must own the tokens; the Model Plane sees only opaque connection ids.

Avoid:
- Rewriting plugin files at launch; keep enablement in the registry.
- Plugins without declared data scopes; require scopes that the capability gate enforces.
- Installing skills from arbitrary GitHub paths before signing and scorecards exist.
- Copying proprietary text. Bundled plugins are marked Proprietary; the curated skill pack carries an Apache-2.0 licence file; design from shapes, wire formats and behaviours, not from prompt prose (guardian prompt, skill bodies, confirmation tables).
- Mixing the two loops: the gateway loop must stay read-only; every effect still goes through execution-core's capability, hook, permission and HITL pipeline.

## 11. Gaps versus Verevon's chat/agent pages and Model Plane

| Area | Codex | Verevon today (baseline) | Gap size |
|---|---|---|---|
| Plugin loading and install | manifest, marketplaces, signed archives, remote catalogue, install-on-demand tool | plugin_packages CRUD, no loader, no install UI | large |
| MCP from the model | per-tool approvals, templates, tool_search, Apps UI | registration and OAuth work; model invocation denied on all paths | large (policy binding) |
| Hooks | 12 events, 4 handler kinds, trust hash, layers | engine real, gateway-memory only, empty in Do mode | medium |
| Skills | packages, budgets, authorities, policy, telemetry | keyword match, 3 per turn, reattach | medium |
| Code mode | V8 isolate, nested tools | absent | large |
| Slash commands / palette | about 50 plus palette and keymap | /models, /compact in catalogue; 5 composer commands; no palette | medium |
| Scheduling | full-agent automations, inbox, heartbeat | single no-tool 1,024-token call, no delivery | large |
| Import | external agent import | none | medium |
| Governance | requirements, signed cloud bundle, guardian | Control-signed decisions, capability gate, risky-tool substring list | medium (guardian) |
| Generative UI / templates | visualize, MCP Apps, artifact templates | artifacts as text/HTML, no templates | medium |
| Instruction transparency | instruction_sources, /debug-config | hierarchy in chat, not in Do | medium |

## 12. Open questions

1. Where, if anywhere, is plugin signature verification enforced (MSIX updater, native Rust, server side)? Not found in JS or Rust strings.
2. Does any plugin component run in a stronger sandbox than a user-account child process? The package suggests no.
3. Which retrieval method (if any) the dynamic skill selector will ship with, and its measured context reduction (reduction_bps telemetry is only an event name here).
4. Exact JSON shape of the Agent Plugins `extensions` namespace for com.openai and how apps/connectors are referenced from a plugin.
5. Semantics of internal codenames (daybreak, aeon, orbit, tortilla, cumberbatch, chronicle) and of the Spaces/Pages feature (document types with a granola_ prefix), relevant to the Verevon Space comparison in audit 1.
6. Whether guardian scores are persisted by default (persist_scores is a config key) and under what retention.
7. How skill_approval is enforced and which actions it covers.
8. The remote catalogue's review and publishing workflow for third-party authors (not visible locally).

## 13. Corrections to the scout inventory (verified in this pass)

- Hook handler enum is command, mcp_tool, prompt and agent (inventory listed agent_hook with low confidence).
- Runtime version string is 0.162.0-alpha.1 (digits after it are ambiguous in a concatenated literal); inventory said not determined.
- Batch CSV agent jobs: tables existed and were dropped by a migration (inventory said only an identifier was found).
- Code Review exposes 27 literal pull_requests.* names (inventory 28).
- Plugin signing: artifacts verified; no enforcing code found (inventory low on enforcement, consistent).
- Cloud config bundle signature verification is real in codex.exe strings (new).
- The notification catalogue in server.mjs contains thread/reverted and a legacy codex/event/thread_rolled_back; the scout's note that thread/rollback appears in JS bundles only was not contradicted.
