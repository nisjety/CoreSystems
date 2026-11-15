# Codex desktop app: agent runtime and model handling (deep dive)

Audit 3 (Model Plane plus Verevon chat and agent pages), facet `agent-runtime-and-models`, source `OpenAI.Codex` 26.1007.2314 (MSIX, app version 26.1007.21434). Companion file: `codexagent.agent-runtime-and-models.features.json` (100 records, ids `codexagent-agent-runtime-and-models-001` to `-100`). Record numbers below are written `#006` and refer to those ids. The sibling facet `codexagent.plugins-and-extensibility` (127 records) covers plugins, skills, MCP, the app-server surface, guardian v2 internals, hosts, persistence tables, memory pipeline and telemetry; this document avoids repeating it and points to its record numbers as "sibling NNN".

Provenance tags: SRC = read directly from shipped files (strings of native binaries, the carved model catalogue, extracted JavaScript, renderer message catalogues); REA = through an REA tool; DOC = documentation text embedded in the binary; INF = my inference. Every claim in the JSON has its own tag and confidence. Nothing was executed.

## 1. Scope, method and limits

What I analysed (all read-only, programme files or extracted copies, no per-user stores):

- `resources/codex.exe` (Rust, 334 MB, no PE resources, no symbols). A read-only Node extractor (`tmp/codex_rt/strx.js`) wrote every printable string to `tmp/codex_rt/codexexe.all.txt` (26 MB, about 1.66 million lines) and a prose-only subset `codexexe.prose.txt` (3.7 MB). Evidence anchors in the JSON of the form `...codexexe.prose.txt:12541` are line numbers in those files; they are reproducible by re-running the extractor. Struct field counts ("struct TurnStartParams with 27 elements") and enum variant names come from serde and clap metadata strings, so I know field names and counts but not wire schemas.
- The model catalogue embedded in `codex.exe` (11 models, 473 KB JSON) was carved by byte offset 264,824,333 (`tmp/codex_rt/models_catalog.json`). This is the single richest SRC object in the audit: it carries per-model prompts, approvals text, auto-review rejection text, multi-agent role text, token-budget text and confirmation policies.
- Helper binaries: `codex-windows-sandbox-service.exe` (56 MB), `codex-windows-sandbox-setup.exe` (18 MB), `codex-command-runner.exe` (8 MB), `codex-code-mode-host.exe` (75 MB). String dumps are in `tmp/codex_rt/bins/`.
- Desktop JavaScript: `.vite/build/main-BklS_2Y2.js` (3.8 MB main process), `worker.js` (git worker), `application-network-startup-*.js`; and the renderer bundles, where I extracted 50,704 react-intl message descriptors (`tmp/codex_rt/ui_strings.json`, via `uistrings.js`; bundles over 3 MB other than app-initial and app-shared were skipped). Renderer asset names cited in the JSON (for example `permissions-mode-dropdown-7bfb50f8e130.js`) were checked to exist.
- The embedded `openai-docs` system skill text inside `codex.exe` (model migration guide, self-knowledge routes, scripts), read as DOC.

REA tools actually used in this run:

1. `analyze_javascript_application` on `.vite/build` (directory, detail summary, record-and-continue, 2048 MB heap, 900 s): parent evidence `ev_e7c1bc1e0691ef7ca490c13417240de6d6b62ce8c855a857a4faf92132df61ae`, summary view `ev_8f60868996be010733cc946da99957da0f2dbbfd597c684acd59699adb6d2503`. 34 files, 10,333,165 bytes, 2,700,745 AST nodes, 1,094 findings, 0 parse failures, coverage partial (semantic analysis of `main-BklS_2Y2.js` and most chunks was not started because the result budget was spent; `browser-page-preload.js` and `zod` exhausted their heap).
2. `inspect_pe_resources` on `codex-command-runner.exe` (`ev_380f3cc9...a25f21`), `codex-windows-sandbox-setup.exe` (`ev_41673289...f392a2`) and `codex-code-mode-host.exe` (first call refused above 64 MiB; re-run with max_file_bytes 128 MiB gave `ev_82fe6865...fb060`). These establish sizes, absence of icon or version resources and one RT_MANIFEST in the setup tool, nothing more.
3. `trace_application_feature` on the retained analysis with string seeds: `sandboxPolicy`, `reasoning_effort` and `app-server` (all no match, which is itself informative: the sandbox, approval and model logic is in the Rust runtime and renderer, not in the Electron main process), `subagent` (`ev_865f1fd1...`, 1 match on a JavaScript endpoint literal) and `automation` (`ev_db085c31...`, 2 matches).

Not used: `open_binary`, `binary_overview`, `search_strings`, `list_strings` (shared active target; read-only string extraction was used instead), `inspect_artifact`.

Limits that matter:

- The Rust runtime was never decompiled. Behaviour is inferred from names, messages and prompt text. Where a mechanism is inferred I say so (records #060 and #095 are medium confidence for that reason).
- The server side (model backend, cloud tasks, aeon, portals, memory consolidation, guardian classifier) is invisible; only client contracts are visible.
- Roughly 20 records rest on renderer message catalogues rather than code (#074 to #082, #087, #088, #090). They prove a feature exists and how it is described to users, not how well it works.
- Model names (GPT-6 Astra, 6.1 Sol, Luna, Terra, Daybreak) are strings in this build. The embedded migration guide and the embedded catalogue disagree on naming (the guide targets 5.6 Terra and Luna, the catalogue lists 6.x), so documentation text lags catalogue data inside the same binary.
- Quality scores are relative judgments from feature names in the other audits in `findings/t3`, not measurements.

## 2. Architecture in one page

Codex is a thin Electron shell around a large Rust runtime. The same `codex` binary is the engine for the desktop app, a terminal UI, a headless `exec` mode, code review, and a background daemon. [#067]

```
Renderer (React, react-intl, 20k assets) <-chunked IPC-> Electron main (spawn, power, tunnels, git worker)
                                                              |
                                                  JSON-RPC (app-server, stdio / unix socket / ws)
                                                              |
                                   codex (Rust workspace: core, app-server, tui, exec-server, hooks,
                                   state, thread-store, rollout, goal, guardian, memories, web-search, ...)
                                      |            |             |               |
                          code-mode host (V8, gRPC)  sandbox helpers   MCP clients    Responses API (HTTP/WebSocket)
```

Key properties:

- The app-server protocol is bidirectional JSON-RPC with 346 client request variants and a set of server-to-client requests for approvals, user input, dynamic tools, auth refresh, attestation and current time. Clients are thin; the agent loop is never in the UI. [#053, #070, #094]
- Execution is host-abstracted: local, WSL (`wsl.exe ... codex -c features.code_mode_host=true app-server`), SSH (nohup app-server on a unix socket, tunnelled), enrolled devices, and cloud environments reached through an origin-allowlisted MASQUE/QUIC portal with SSH and a VNC bridge. [#064, #065]
- State is an append-only JSONL rollout per thread plus a SQLite index that stores byte offsets of turns; the index self-repairs on read. [#069]
- Features ship behind about 170 named flags, plus remote evaluation, plus admin requirements. [#071]
- The desktop is one app with three product modes (Chat, Work, Codex) sharing the same runtime and UI parametrised by product mode (`chatgptMode` and `codeMode` string variants). [#081]

## 3. Agent runtime

### 3.1 The turn contract
`turn/start` carries 27 fields: client message id, trigger, parent and root turn ids, environments, approval policy and reviewer, sandbox or permission profile, service tier, effort, personality, collaboration mode, multi-agent mode, output JSON schema, disabled plugin ids, additional context. `thread/start` carries 27 more (dynamic tools, selected capability roots, ephemeral, history mode, base and developer instructions, provider-model fallback). Every turn is attributed (kind: user, subagent, guardian_review, memory_consolidation, other; trigger; parent and root; initiating agent path) and every thread reports a status with waiting-on-approval and waiting-on-user-input flags and a session source (cli, vscode, appServer, several subAgent kinds). [#001, #085]

Lesson: the entry contract is typed, per-turn and lineage-aware. Chat, scheduled runs, subagents, reviews and memory jobs all use it.

### 3.2 Context assembly and long-run survival
- World-state fragments. The prompt is rebuilt each step from versioned sections (permissions, environments, tools, skills, plugins, collaboration mode, model catalogue, AGENTS.md, context window, realtime, approved command prefixes); only changed sections are re-sent; duplicate section ids are rejected; replay ignores patches without a full snapshot. [#019]
- Compaction has two paths. Local: a summariser with a fixed hand-off prefix; on overflow the oldest item is dropped and compaction retried. Remote v2: the server returns exactly one opaque compaction item. Reasons are typed: user_requested, context_limit, model_downshift, comp_hash_changed (a prompt-template change triggers recompaction), and a failed compaction by the previous model is retried by the current one. [#017, #039]
- Context-reset recovery (shipped disabled in this catalogue): near exhaustion the model is told to write structured notes, then calls `new_context`; in the next window it recovers with read-only history tools using opaque window and item ids; every non-assistant item carries a trailing id marker. [#018]
- Rollout budget: a token-weighted cap on a whole rollout with reminder points. Inferred from config validation strings. [#020]
- Persistence of authorisation: the base prompt says user authorisation persists across turns. [#056]

### 3.3 Planning, goals, persistence
- Plan Mode is a collaboration mode: read-only exploration, intent interview, then one decision-complete `<proposed_plan>`; `update_plan` is rejected in Plan Mode; planning can have its own reasoning effort. `update_plan` otherwise is a plain checklist tool. [#004, #005]
- Thread goals are the strongest long-running pattern in the audit: an objective with token budget and time accounting, auto-continuation when idle, an audit prompt that demands requirement-by-requirement evidence before completion, a blocked state only after three identical-blocker turns, and states active, paused, stalled, usage-limited, budget-limited, complete. They are an extension crate behind a flag. [#006]
- Persistent mode adds async user messages, a sleep primitive with an "Up next" note, and rules that bound follow-ups by outcome rather than count. [#007]
- Experimental prediction threads exist but the request is "not implemented yet". [#090]

### 3.4 Steering, interruption, rewind
Mid-run messages steer by default; a persisted, reorderable, SQLite-backed queue holds the rest; steering carries an `expectedTurnId` guard; review and compact turns refuse steering with a typed error. Interrupts flush the rollout, stop code cells, and fire an Interrupt hook. Rewind is `thread/rollback` or `thread/revert`; forks are by reference to source rollout segments, validated against lineage; deleting a source with live forks is refused; side conversations open a throwaway chat that is seeded with `thread/inject_items` and restored to the parent afterwards (the fork step itself is inferred). [#002, #003, #022, #023]

### 3.5 Delegation
Multi-agent v2 gives a root agent `/root`, canonical task paths, `spawn_agent` with `fork_turns`, `followup_task` (triggers a turn) versus `send_message` (does not), `wait_agent` (waits for mailbox updates and ends on steered user input), `interrupt_agent`, `list_agents`, `close_agent` (closes descendants). Policy: do not spawn unless the user, AGENTS.md or a skill asks, except in Ultra effort where delegation is proactive and a usage warning appears. Roles come from config or disk; there is an agent message board with an idempotent SQLite log; idle agent threads are unloaded and an unloaded v2 sub-agent can only be resumed through its parent. A legacy v1 tool surface remains for older models, selected per model in the catalogue. [#008 to #013]

### 3.6 Tools: code mode and deferred discovery
For code_mode_only models (every GPT-6 and 5.6 entry in this catalogue) the model gets one `exec` tool that runs JavaScript in a fresh V8 isolate inside a separate host process (gRPC session, cell actors, `wait` to resume), with `tools.<name>()` calls, `text()`, `image()`, `store()` and `yield_control()`; nested tool calls go through the same approval handlers; the host can also be remote. Third-party tools are hidden and discovered with a BM25 `tool_search` (direct versus deferred exposure; placeholder tools for servers that are down). [#014, #015]

### 3.7 Hooks, clarification, dynamic tools
Hooks have three handler types and spill large output; `request_user_input` has sync and async forms and must not be used for permissions; clients can register dynamic tools per thread. [#016, #026, #028]

### 3.8 Automations, cloud, aeon
Automations: local or cloud, RRULE, heartbeat wake-ups into an existing chat, an inbox contract. Cloud tasks: environments per workspace with domain-scoped secrets, setup scripts, and diff/apply. Beyond tasks, the renderer contains a hidden persistent assistant (aeon, shown as a "dot") that has its own mailbox and phone number, Slack and Teams presence, a live computer panel with human take-over, top-of-mind tasks, reminders, memory and scheduled tasks, access gated by a seat type. [#024, #025, #074, #075]

### 3.9 Agent builder (workspace agents)
A GPT-builder-like surface: draft and publish with version history and optimistic-concurrency copy, instructions with @mentions, per-agent model and effort (Auto options), capability toggles, skills, apps with shared versus personal connections (with an explicit blast-radius disclosure), Slack channel deployment, schedules with a save-and-run-preview step, sharing (can chat versus can edit, workspace link, directory), an Activity tab with run states, a Memory tab of agent files. [#076 to #079]

## 4. Model handling

### 4.1 The catalogue is data
Each model record declares context window (272k default, 872k max for most; 372k for one cyber model), modalities, truncation policy, tool mode (code_mode_only versus classic), multi-agent version, shell and patch tool types, reasoning ladder with descriptions, service tiers, plan availability (25 plan identifiers), minimal client version, upgrade target with migration copy and retirement date, and the prompts themselves (instructions template 21 KB, persistent instructions, approvals, collaboration modes, auto-review, multi-agent roles, token budget, guardian classifier, confirmation policies). A change in `comp_hash` triggers recompaction. A client refreshes it per account with an ETag cache and can override it with a local file; unknown models fall back to default metadata with a warning. `model/list` returns a client-facing summary. [#029, #031, #039, #045, #098]

### 4.2 Reasoning and speed
Ladder low, medium, high, xhigh, max, ultra (Ultra means maximum reasoning plus automatic delegation, with a usage warning). Reasoning can be changed mid-conversation by appending a configuration-update item, which preserves the cached prefix (per the embedded guide). Service tiers (Fast with a usage multiplier, an ultrafast flag) are per model and broken out in credit history. Separate model slots exist for review, auto-review, memory extraction and consolidation, compaction fallback and subagent defaults. [#032 to #034, #040]

### 4.3 Routing, fallback, transport
No per-request router was observed in client code; "routing" is (a) the user's picker with a Default that selects among recommended models and a single power slider, (b) backend-declared fallback model lists on usage limits with automatic switch and switch-back, (c) catalogue upgrade and retirement with auto-switch, (d) step-level model switching, (e) provider abstraction (OpenAI backend, Ollama, LM Studio, Bedrock, gateway OAuth) with retry and idle-timeout budgets. Transport uses a persistent Responses WebSocket with previous-response continuation, a 60 minute connection cap, a full-request retry when continuation fails, and fallback to HTTPS. [#035 to #038, #044]

### 4.4 What the embedded documentation adds (DOC)
The embedded guide describes async tool calling (the application returns a result later by call id while the model keeps working), mid-turn steering over WebSocket, in-band reasoning changes, asynchronous misalignment monitoring, removed sampling parameters, tool calling only through the Responses API, and a 30 minute prompt-cache TTL option. It also ships a migration playbook as an agent skill: inventory every model site, classify, map roles (flagship, balanced, fast) without collapsing a multi-model design, preserve reasoning effort, never weaken schemas, validate with a five-arm comparison and a mandatory report. [#091, #092]

### 4.5 Voice and web as model capabilities
Realtime voice is a front agent that only delegates to the main agent through one tool and has a silence tool; transports are WebRTC or WebSocket; protocol versions v1 to v3 appear. Web access is one multi-operation tool (search, image search, open, click, find, PDF screenshot, finance, weather, sports, time). [#083, #084]

## 5. Safety, permissions and sandboxing

This is the area where the runtime is most mature and most Windows-specific.

### 5.1 Policy model
Three coarse modes (read-only, workspace-write with network toggle, danger-full-access) plus external sandbox, expressed as named permission profiles with special paths, globs and deny-read lists; approval policies untrusted, on-request, never, granular; approvals reviewer user or auto-review. Admin requirements can restrict all of it (sibling 070). [#051]

### 5.2 Per-command escalation
The model-facing contract is a single tool argument set: `sandbox_permissions` (default, additional permissions, escalated), a `justification` that is the user-facing question, a reusable `prefix_rule`, and structured `additional_permissions` for network and paths. Decisions are typed (approved, approved with amendment, approved for session, MCP policy amendment, network allow or deny, denied, abort), and the thread status exposes who is blocked. A repeated-denial circuit breaker is a typed error. [#052, #053]

### 5.3 Reviewer and doctrine
"Approve for me" delegates routine escalations to a reviewer model. The catalogue text forbids working around a rejection, requires reporting what is blocked and why, and the base prompt tells the agent to do all authorised work first so that approval is the last concrete step, to carry authorisation across turns, and never to ask permission through the clarification tool. Full-access has deliberate friction (capability checklist, banner, Ultra guard, cyber-model nudge). [#055 to #057]

### 5.4 Enforcement
- Backends: Seatbelt, Landlock plus seccomp (optionally bwrap), Windows restricted token, and a Microsoft MXC container backend (BaseContainer, falling back to AppContainer). [#048]
- Windows elevated design: SYSTEM service, local group and accounts (online and offline), DPAPI-protected secret, WFP filters that block non-loopback and loopback traffic except the managed proxy, a private desktop, ACL capability SIDs per writable root, deny ACEs, and a complete uninstall. Commands run through a runner using restricted tokens, ConPTY and job objects. The UI gates sending on setup, handles UAC cancellation, offers a non-admin path and degrades to read-only when policy forbids the update. [#049, #050, #058]
- Violations are recorded with backend, reason, path or host, protocol and decision; blocked hosts can be allowed or denied and the answer amends the policy. The managed proxy enforces limited mode, requires MITM for HTTPS policy and blocks local and private ranges. [#054]
- Static command analysis: tree-sitter lowering of bash and PowerShell to literal word lists with fail-closed rejection of dynamic constructs, dangerous-command and URL-launch detection (inferred from module names and errors). [#095]
- Environment filtering and brokered credentials in shell snapshots; a no-follow, descriptor-handoff file helper in the exec server (inferred). [#059, #060]
- Platform-specific guardrails are embedded in tool descriptions (Windows safety rules in `exec_command`). [#062]

## 6. Computer and environment control

- Unified exec is a PTY session contract: `exec_command` returns output or a session id; `write_stdin` writes or polls with bounded waits (5 to 300 s for polls); output is capped by tokens. Clients can also run commands through `command/exec` with write, resize and terminate, and user shell commands are separate thread items. [#062, #096]
- `apply_patch` is a grammar-constrained freeform tool with shell-heredoc interception so that patches cannot bypass approvals through the shell. [#063]
- Hosts: one spawn/file interface across local, WSL, SSH, devices; cloud portals with origin allowlist, short-lived tokens renewed over stdin, SSH host-key check and a VNC bridge; power blockers keep the machine awake while threads run. [#064 to #066]
- Git: attribution trailer, apply-last-diff command, PR watch with auto-merge, worktree policy settings, managed worktrees as tools. These are coding features; for Verevon they are mostly reference only. [#072, #073, #099, #100]

## 7. Platform architecture and transparency

- One binary, many fronts, with a CLI that includes `exec`, `review`, `resume`, `fork`, `queue` (inject a message into an existing session), `sandbox`, `execpolicy check`, `apply`, `doctor`, `debug`, and `app-server daemon` with remote control. Schemas are generated for clients (TypeScript and JSON Schema). [#067]
- Transparency tooling is first-class: `codex debug prompt-input` renders exactly what the model sees, `debug models` renders the catalogue, `debug trace-reduce` replays a trace bundle, `doctor` produces a redacted machine-readable health report. In the transcript, shell commands are parsed into semantic actions (read, list, search) and connector actions have about 900 human-readable activity templates. [#068, #088, #097]
- Storage: rollout plus offset index with read-repair and throttled migration; the client has a version gate, stable operation error codes and a bounded input queue. [#069, #094]
- The runtime asks the client for the time and an attestation, so a remote runtime is never trusted for those. [#070]

## 8. What is best-in-class here

1. Per-command escalation contract plus typed approval decisions that amend policy (#052, #053): the agent never edits policy, it asks, and the answer becomes a rule.
2. Goals with budgets and an evidence-based completion audit (#006).
3. Catalogue as data, including prompts, with hash-triggered recompaction (#029, #039, #017).
4. World-state fragments with stable ids (#019) together with `debug prompt-input` (#068): inspectability by construction.
5. Windows sandbox depth (WFP offline user, private desktop, DPAPI, uninstall) (#049) and graceful degradation (#058).
6. Code mode in an isolated V8 host with nested approvals and deferred tool discovery (#014, #015).
7. Rollout-plus-index storage with read-repair (#069).
8. Shared versus personal connection semantics for agents (#077).
9. A model-migration playbook shipped as a skill (#091).

## 9. Weaknesses and limits

- Complexity: roughly 170 flags, two multi-agent tool surfaces, two compaction paths, two Windows sandbox modes plus MXC; many features are experimental or disabled in the shipped catalogue (#018, #020, #090).
- Server-side dependence: remote compaction v2, guardian classification, notes and history endpoints, aeon, cloud tasks, portals and attestation cannot be reproduced from the client; a Verevon implementation needs its own services.
- Heavy platform coupling: the elevated Windows sandbox needs a service, local accounts and UAC; a lot of the safety value is Windows-specific.
- Documentation lag inside the binary (guide targets 5.6 tiers while the catalogue is on 6.x) and several hidden features ("restricted.aeon", Chronicle) mean UI strings can describe states the product does not currently ship.
- Telemetry stack is large (Sentry, Statsig, OpenTelemetry, analytics reducer), which matters for ZDR parity.
- No eval harness is shipped in the app (INF from the absence of one); quality claims are therefore not measurable from the artifact.
- Single-vendor Responses API reliance for WebSocket continuation, configuration-update items and cache options; the design does not transfer directly to Anthropic or Azure routes.

## 10. Comparison with the other audited apps (INF, feature names only)

- Hermes has a larger hook vocabulary, a smart-approvals guardian with a circuit breaker, an egress credential proxy and cron with at-most-once slots; Codex has deeper per-command escalation, typed decisions with policy amendment and a more rigorous catalogue.
- DeepSeek harness is the transparency peer (compaction as a log transaction, runtime-context snapshots, crash-safe JSONL store); Codex matches it on prompt inspection and exceeds it on sandboxing breadth.
- Manus has near-identical permission-mode labels ("Approve for me") and a Windows safe-mode design, plus remote-control grants and a sandbox sidecar; Codex's WFP firewall user and portal tunnel are the distinguishing designs.
- Claude has the strongest VM-based sandbox and a signed catalogue; Codex is stronger on typed approval protocol and long-run primitives (goals, persistent mode, remote compaction).
- T3 Code is stronger on multi-provider breadth; Codex is single-provider-family with richer per-model metadata.

## 11. What Verevon should copy, adapt or avoid

Mapped to the Verevon Model Plane baseline (`v-modelplane.baseline.md`), whose gap rows I cross-checked while writing relevance notes.

Copy now (low to medium cost, high effect):

1. Typed turn envelope and attribution (#001, #085) for gateway, Do mode, scheduled and subagent runs; add a waiting-on flag to run status.
2. Approval protocol: decision enum with session and policy-amendment outcomes, per-call justification and proposed rule, reviewer delegation with a binding rejection protocol, and, above all, let the LLM loop resume after approval (baseline: post-approval continuation re-executes only three action kinds). (#052 to #056)
3. A goal object with token budget and completion audit for scheduled and Do runs (baseline: scheduled step is one no-tool 1,024-token call, no delivery). (#006, #007, #024)
4. Stable context-section ids, a user-facing prompt-input export, and compaction reasons including model downshift and prompt-hash change (Verevon auto modes change models mid-thread). (#019, #017, #068)
5. Model capability record and `model/list` summary (efforts, modalities, tiers, plan access, prompt hash) so the SPA picker and routing share a source of truth; add an effort ladder for the OpenAI and Azure path (baseline: no reasoning effort sent). (#029, #032, #098)
6. Usage-limit fallback with declared fallback list and switch-back for the BYO ChatGPT route (#036).
7. Rewind and fork with reference-based lineage on the event-sourced session store (baseline: no branch model, no rewind) and a byte-offset or equivalent turn index for pagination. (#022, #069)
8. Typed run and thread status vocabulary, per-tool activity labels, and parsed command actions to make Do runs legible. (#085, #088, #097)

Adapt (needs a design decision):

- Deferred tool discovery and, later, code mode (baseline: MCP tools denied by the capability gate; code mode absent). Nested tool calls must still pass the capability, hook, permission and HITL pipeline. (#015, #014)
- Agent builder patterns for the Agent pages: draft versus live, version history, conflict copy, directory visibility, preview-run before scheduling, shared versus personal connection disclosure. (#076 to #079)
- Voice as a front agent that delegates to the governed loop rather than a second agent stack (#083).
- Memory citations with rate and delete controls and agent memory as inspectable files (#082, #079). Chronicle-style ambient memory only with the consent model of #080 and never under ZDR.
- Persistent assistant with channels (#074, #075) as the concrete target shape for the Channel Plane, to be evaluated after governance (Space authority, ZDR) is specified.
- Multi-agent: take path addressing, fork-turns, mailbox wait semantics and the explicit-authorisation policy tied to tier (#008, #010); skip the message board until needed (#012).

Evaluate only if Verevon grows a local executor: OS sandbox backends and Windows elevated design (#048 to #050, #058), cloud-portal tunnelling and takeover (#065, #075), TOCTOU-safe file helper (#060), static command lowering (#095).

Avoid or treat with care:

- Flag sprawl without lifecycle discipline (#071) and dual surfaces for one capability (#009).
- Copying prompts verbatim: they are per-model tuned data; copy the mechanism (catalogue field plus hash), not the text.
- Treating UI strings as proof of shipped behaviour (aeon, Chronicle, security scans).
- Assuming Responses-API-only features (configuration-update items, WebSocket continuation) work on Anthropic or Azure routes.

Rust-portable notes: a schema-first JSON-RPC surface with generated clients; an extension registry that lets features contribute tools, prompt fragments, hooks and world-state sections; a sandboxing crate with a violation recorder; V8 hosted over gRPC for code mode (rusty_v8 or WASM are alternatives); tree-sitter for command lowering; append-only rollouts with a seekable compressed reader and a SQLite index; typed error enums (`CodexErrorInfo`) with stable codes surfaced to clients. These map well onto Verevon session-core (event-sourced) and execution-core.

## 12. Open questions

1. What does the Rust runtime actually enforce when the dangerous-command classifier and exec-policy prefix rules disagree (#095, #052)? Needs decompilation or a runtime trace.
2. Is remote compaction v2 usable outside OpenAI-hosted models, and what is the item format (#017)?
3. What decides the choice between local and cloud execution for automations at run time (#024)?
4. How is the VNC session in cloud portals used in the UI (take-over versus passive view) (#065, #075)?
5. What are the semantics of the safety_buffering stream field and the misalignment monitor (#061)?
6. What exactly does Chronicle capture, and where is the summarisation performed (#080)?
7. Which of the roughly 170 flags are on by default in the shipped build (#071)? `experimentalFeature/list` would answer it at runtime.
8. How do aeon tasks relate to automations and cloud environments in the server (#074)?

## 13. Evidence index

- String dumps: `scratchpad/tmp/codex_rt/codexexe.all.txt`, `codexexe.prose.txt`, `bins/*.txt`, derived by `strx.js`.
- Carved catalogue: `scratchpad/tmp/codex_rt/models_catalog.json` (byte offset 264,824,333 of `resources/codex.exe`); prompt extracts `astra_*.txt`.
- UI strings: `scratchpad/tmp/codex_rt/ui_strings.json` (50,704 descriptors).
- Desktop bundles: `scratchpad/extract/codex/.vite/build/*.js` (char offsets cited as `@N`), `scratchpad/extract/codex/webview/assets/*.js`.
- Build and check scripts (read-only helpers I wrote): `scratchpad/tmp/codex_rt/build/` (`assemble.js`, `check_evidence.js`, `check_ui.js`). The checkers confirmed 194 line anchors resolve (192 contain a keyword from their own description; two are false alarms), 53 message ids exist, 29 asset files exist and 20 bundle offsets are in range.
- Sibling facet: `scratchpad/findings/t3/codexagent.plugins-and-extensibility.features.json` and `.deep.md`.
- Scout inventory: `scratchpad/findings/t3/codexagent.inventory.md` (used for orientation; counts re-verified where relied upon).
