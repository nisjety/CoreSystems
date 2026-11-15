## 7. What each reference does uniquely well, and how that helps Verevon

This section takes each of the 12 reference sources in turn. The provenance tag says how the capability was observed: [REA] findings from the shipped installed apps, which REA analysed for structure but which were mostly read directly from the extracted bundles (the cards tag the same findings [SRC]; see Appendix A.3), [SRC] source code, [DOC] public documentation only, [INF] inference. Where a capability is carried by a recommendation, the link points to it; where no recommendation carries it, the reason is stated.

### 7.1 Claude desktop app 2.31226.1

Claude desktop is the overall reference the owner wants to re-create in Rust. Its strength is governed autonomy with calm UX: typed contracts for permissions, scheduling and plugins, and restraint in what the user is shown [REA]. Its safety comes from typed contracts plus UX discipline, which maps directly onto Verevon's signed capability gate [INF, from the REA findings and the Verevon baseline].

- **Closed run-state vocabulary with an inbox sidebar.** Sessions carry a fixed set of states, with mark-completed, sections and mixed chat and task Recents [REA]. Verevon's rail shows run-status chips but has no shared vocabulary or attention ordering: [F-t3-015](#F-t3-015).
- **Approval discipline.** Anti-decoy cards, "always" coerced to session scope, a plan fingerprint re-checked at execution and a never-auto tool set that survives any mode [REA]. Verevon's approvals are single-use and risk is classified by substring on the tool name: [F-t3-006](#F-t3-006), [F-t3-003](#F-t3-003).
- **Scheduled tasks that store approved permissions.** Approved permissions are stored per tool with an admin-managed lifetime (no expiry when it is unset) and a non-delegable tool set, and high-risk notifications drop the Allow action [REA]. Verevon's scheduled runs are a single no-tool call: [F-t3-018](#F-t3-018).
- **Per-site browser consent with admin allow/block lists.** Consent is batched by site with an explicit allow-all switch, and private origins are DNS-pinned [REA]. Verevon's browser risk classes exist but agent runs fail closed on the grant: [F-t3-008](#F-t3-008).
- **Credential masking with sentinel injection.** Secrets are replaced by sentinel values and injected per host, with SigV4 re-signing [REA]. Verevon's sandbox has no network and no injection proxy: [F-t3-043](#F-t3-043).
- **Document skills with scripts and validators.** Six always-on skills ship with the app; four are document skills with scripts, three share an OOXML schema validator, and runtime execution was not observed [SRC]. Verevon produces Office files from ad-hoc interpreter code: [F-t3-014](#F-t3-014).
- **Typed plugin manifest and 33 hook events.** The manifest declares typed user config and sha-pinned binaries; hooks have command, HTTP, prompt and agent handlers [REA]. Verevon's plugins have no install path and hook rules live in memory: [F-t3-059](#F-t3-059), [F-t3-032](#F-t3-032).
- **Self-configuring product.** The model reads and sets app preferences through built-in servers whose keys declare what the model may change [REA]. Verevon's action registry already carries that metadata but does not expose settings actions to the model: [F-t3-061](#F-t3-061), [F-t3-025](#F-t3-025).
- Not recommended: the desktop shell process model, local and SSH execution environments and the installer with auto-update. They are desktop table-stakes that do not apply to a cloud web product [INF].

### 7.2 Hermes Agent, commit b23600860f (2026-10-12)

Hermes is the learning reference: it stages what it learns, keeps a reversible ledger and treats prompt-cache stability as an invariant [SRC]. Verevon's Dreaming extractor and recall pipeline already match or beat Hermes on provenance; Hermes is ahead on governance of what is learned [SRC for Hermes; Verevon status from the code baseline].

- **Fail-closed staging and a skill ledger.** Unattended memory replace and remove are staged fail-closed, skill write approval is opt-in (off by default), and every skill mutation is recorded with a coarse actor and before/after manifests so one edit can be rolled back; the ledger calls itself telemetry, not a gate [SRC]. Verevon's learning loop writes org-matchable skills with no gate or undo: [F-t3-007](#F-t3-007).
- **/learn and lessons-not-logs authoring.** A user command distils a session into a skill, written as lessons rather than transcripts [SRC]. Verevon has only /models and /compact: [F-t3-058](#F-t3-058).
- **Prefix-cache stability as an invariant.** Wire-only recall is replayed byte-identically and the todo store is re-injected after compression [SRC]. Verevon's OpenAI stable-prefix order is built but off and the hit rate is unmeasured: [F-t3-022](#F-t3-022).
- **Durable row ids with FTS and trigram search, and a recall tool without an LLM call.** Rewind is a truncating operation over durable ids; recall results are treated as untrusted data [SRC]. Verevon has no server-side search or branch model: [F-t3-012](#F-t3-012).
- **Cron jobs as full agent runs.** Jobs run with skills, context chaining, multi-platform delivery and an at-most-once ledger; wake gates and a silent sentinel avoid empty runs [SRC]. [F-t3-018](#F-t3-018), [F-t3-047](#F-t3-047).
- **Supervised MCP (Model Context Protocol) client runtime.** Servers are supervised with recovery and per-server and per-tool configuration [SRC]. Verevon registers MCP servers but no loop can call their tools: [F-t3-003](#F-t3-003).
- **Pinned plugin catalogue with a kill list.** About 570 entries are each pinned to a 40-hex commit, CI clones every entry and a removed list acts as a kill switch [SRC]. [F-t3-059](#F-t3-059).
- **Provider profiles and credential pools.** 41 overridable provider profiles and a failover with credential rotation [SRC]. [F-t3-065](#F-t3-065), [F-t3-072](#F-t3-072).
- **Loop guard.** A side-effect-free controller detects repeats, cycles and no progress and applies stop gates [SRC]. Verevon has no repetition or no-progress detector: [F-t3-063](#F-t3-063).

### 7.3 DeepSeek harness, commit d743267388 (2026-10-09)

The DeepSeek harness is the transparency reference: it shows the user the most information about what the model saw and did, and it is engineered for customisation [SRC]. Verevon already adopted its parallel tool scheduling and its postmortem culture [SRC, Verevon decision ledger].

- **Trajectory ledger with request capture and a timing waterfall.** Every step keeps the exact model-visible request, tool schema and options, with per-step timing [SRC]. Verevon's Trace tab is a flat event list capped at 5,000 rows: [F-t3-034](#F-t3-034).
- **Typed end reasons, visible retries and a non-destructive compaction marker.** Retries and compaction are events the user can see; nothing is silently rewritten [SRC]. Verevon retries tool calls silently and emits no compaction event: [F-t3-049](#F-t3-049).
- **Four density levels and an activity grammar.** Work detail is set per turn, group or row with a fixed activity vocabulary and `hidden="until-found"` for collapsed rows [SRC]. [F-t3-048](#F-t3-048).
- **Effective-configuration provenance.** Every setting shows its effective value and where it came from, with override markers and stale-revision refusal [SRC]. [F-t3-068](#F-t3-068), [F-t3-061](#F-t3-061).
- **Checkpoint-based resume.** Checkpoints are written before model requests, tool effects and next steps; a cold resume repairs the log tail and closes dangling calls [SRC]. Verevon's continuation worker re-runs only three action kinds and the loop itself does not resume: [F-t3-002](#F-t3-002).
- **Verify-and-render Office output.** docx, pptx and xlsx skills carry a structural checker that does not judge pagination, clipping, fonts or Excel recalculation; rendering comes from a separate LibreOffice Kit, and Claude desktop is the stronger verify-and-render reference [SRC and DOC]. [F-t3-014](#F-t3-014).
- **Recorded-session replay tests and i18n gates.** Recorded sessions replay in CI; typed dictionaries and a gate reject hard-coded copy [SRC]. [F-t3-040](#F-t3-040), [F-t3-066](#F-t3-066).
- **Sandbox enforcement reports.** Each call reports whether enforcement was full or partial, and denial signatures are matched only after runner-failure rules [SRC]. Verevon's ledger leaves this vocabulary open: [F-t3-043](#F-t3-043).
- **Batched web tool and chunked reader.** Multi-query fan-out, open by reference and find-in-page in one tool; long pages are read in chunks [SRC]. Verevon's chat `web_search` takes one query per call: [F-t3-054](#F-t3-054).

### 7.4 T3 Code 0.0.35

T3 Code is the clean-UI, multi-model reference. It is best at a calm work list, a fast transcript and an honest composer, built on idempotent command receipts [REA].

- **Inbox-style sidebar and status pills.** Pinned, active with status and elapsed time, snoozed and a collapsed Settled tail [REA]. [F-t3-015](#F-t3-015), [F-t3-049](#F-t3-049).
- **Virtualised timeline with lazy heavy panels.** Hidden mounts are capped and heavy work runs in workers [REA]. Verevon reparses the whole message on every streamed token and has no virtualisation: [F-t3-021](#F-t3-021).
- **Settled-turn folding.** Completed turns fold to a summary line [REA]. [F-t3-048](#F-t3-048).
- **Atomic composer chips and per-thread drafts.** Context references are atomic Lexical tokens and drafts persist per thread [REA]. Verevon persists only the Home draft: [F-t3-052](#F-t3-052).
- **Changed-files card and review comments.** A per-turn changed-files section with auto-expand thresholds, and a serialised review comment that round-trips into the next prompt [REA]. [F-t3-076](#F-t3-076), [F-t3-031](#F-t3-031).
- **Provider health and model picker.** A provider health snapshot and a searchable picker with favourites and a legacy fold [REA]. [F-t3-042](#F-t3-042), [F-t3-072](#F-t3-072).
- **Serial command queue with idempotent receipts.** Every command is a receipt the client can reconcile after reconnect [REA]. Verevon's cancel and queued input live in process memory: [F-t3-016](#F-t3-016).
- **Palette over commands, threads and files, and OKLCH theming.** One palette reducer with mutually exclusive modes; about 56 colour roles generated with contrast solved [REA]. [F-t3-077](#F-t3-077), [F-t3-098](#F-t3-098).
- **Real human-versus-agent input arbitration.** Expected synthetic input is recorded and anything else is treated as the human [REA]. [F-t3-026](#F-t3-026).

### 7.5 Manus (Manus Studio) 2.0.6

Manus is the computer reference: it controls a computer as a user would, with a declared side-effect taxonomy and a replayable record of what it did [REA]. Its web product adds the task-as-URL model and projects with roles [DOC].

- **Accessibility-first computer use.** A UI Automation tree with short handles, element-targeted actions, synthesised input only as fallback and a strict window-identity ladder [REA]. Verevon has no desktop and no computer use; the path runs through its own sandbox desktop: [F-t3-024](#F-t3-024).
- **Per-task sandbox computer with a typed executor protocol.** The cloud loop proposes actions and a host executes them over a validated protocol [REA]. Verevon's execution is bwrap inside the execution-core container with no VM per thread: [F-t3-009](#F-t3-009).
- **Computer panel with a viewer per tool kind.** Browser, terminal, files and desktop each get a viewer, with picture-in-picture that follows the user [REA]. [F-t3-035](#F-t3-035).
- **Takeover with hand-back.** The agent suggests takeover for logins, 2FA and CAPTCHAs and resumes after a hand-back message [REA, with DOC strings]. [F-t3-026](#F-t3-026).
- **Scrubbable replay with per-action briefs.** The screen re-renders at any past action [REA]. Verevon keeps an event timeline, not a scrubbable replay: [F-t3-050](#F-t3-050).
- **Declared side-effect taxonomy and an exactly-once action ledger.** Five declared levels on its 22 browser tools drive replay deduplication and MCP annotations, not approvals, and an in-memory ledger (4096 entries, 10-minute horizon) returns a replay conflict on digest mismatch [SRC]. Driving approvals from a declared class is a Verevon synthesis [INF]. Verevon decides risk by substring matching: [F-t3-003](#F-t3-003), [F-t3-002](#F-t3-002).
- **Wide research with an estimate and a spend gate.** Parallel isolated sub-agents with a pre-run estimate, a subtask grid and a gate above a threshold [REA and DOC]. Verevon has the fan-out engine but no user entry point or estimate: [F-t3-036](#F-t3-036).
- **Validated slides pipeline.** An MCP slides server owns a project manifest with per-slide state, XML validation hooks and a present gate [REA and DOC]. [F-t3-038](#F-t3-038).
- **Live cost, projects with roles, named persistent agents and a cross-chat library** [REA]. [F-t3-028](#F-t3-028), [F-t3-013](#F-t3-013), [F-t3-019](#F-t3-019), [F-t3-080](#F-t3-080).

### 7.6 Codex desktop app 26.1007.2314.0

Codex is the plugin-system reference with ChatGPT-model capabilities. It is best at typed contracts for decisions, plans and goals, a full MCP Apps implementation and a code mode that keeps tools behind the guard [REA].

- **Open plugin manifest with adapters and atomic upgrade.** An agent-plugins 1.0 manifest, foreign-namespace adapters, a capabilities-from-this-plugin prompt block and rollback on failed upgrade [REA]. [F-t3-059](#F-t3-059).
- **Full MCP Apps.** The ext-apps protocol with a gated JSON-RPC bridge and form elicitation [REA]. Verevon renders HTML in an opaque iframe with no bridge: [F-t3-045](#F-t3-045).
- **Typed approval decisions.** Approved, approved-for-session, approved-with-amendment, denied and abort, with about 55 versioned card templates [REA]. [F-t3-006](#F-t3-006).
- **Decision-complete plan mode.** A read-only interview produces a proposed plan; sync and async `request_user_input` pause the run [REA]. Verevon has an editable plan card but no ask-user tool: [F-t3-011](#F-t3-011).
- **Persistent goals with a token budget and a completion check.** A thread goal persists with an audited completion judge and auto-continuation [REA]. [F-t3-037](#F-t3-037).
- **Code mode in a V8 isolate.** Model-written JavaScript calls tools as async functions with bounded output, and every sub-call re-enters the guarded pipeline [REA]. [F-t3-070](#F-t3-070).
- **PTY exec with yield and poll.** `exec_command` with a tty, yield time and max output tokens, plus stdin polling [REA]. [F-t3-035](#F-t3-035).
- **Model lifecycle and usage-limit fallback.** Deprecation, retirement date and upgrade target per model; a fallback model on usage limit with automatic switch-back [REA]. [F-t3-073](#F-t3-073), [F-t3-072](#F-t3-072).
- **Review as a run.** A read-only reviewer with strict findings and a report-only-what-the-author-would-fix standard [REA]. [F-t3-086](#F-t3-086).
- Not recommended as-is: the Guardian reviewer on the permission path, which conflicts with Verevon's standing rule of no LLM on the permission hot path; only a tighten-only variant is kept as a nice-to-have pending an ADR: [F-t3-097](#F-t3-097).

### 7.7 Google Antigravity (agent hub 2.11.0)

Antigravity is the Gemini reference. It is best at a browser subagent with its own model, budget and recording, Gemini-specific controls, and a step inspector [REA]. A Gemini provider family is the prerequisite for using any of its Gemini-specific strengths [INF].

- **Browser subagent with its own model, budget and recording.** The subagent has separate `read_url` and `execute_url` permissions with a server denylist (fail-closed per documentation, DOC), and its recording is stored as an artifact [REA]. Verevon's `browser_agent` is a separate loop but not offered in chat: [F-t3-008](#F-t3-008), [F-t3-050](#F-t3-050).
- **Widest browser action vocabulary with per-action state diffs.** DOM, pixel and hybrid modes [REA]. [F-t3-008](#F-t3-008).
- **Gemini thinking levels and typed media parts** [REA]. Verevon's `ModelFamily` is only OpenAI-compatible or Anthropic: [F-t3-041](#F-t3-041), [F-t3-090](#F-t3-090).
- **Trajectory and step inspector** [REA]. [F-t3-034](#F-t3-034).
- **Quota display and cost by token category** [REA]. [F-t3-028](#F-t3-028).
- **Accept or reject by file, hunk or all, with written feedback returned to the trajectory** [REA]. [F-t3-076](#F-t3-076).
- **Knowledge items distilled from work, with limits** [REA]. [F-t3-093](#F-t3-093).
- **Strict mode and an action(target) grant store with deny over ask over allow** [REA]. [F-t3-032](#F-t3-032), [F-t3-074](#F-t3-074).
- **Composer-level MIME table and typed scope items** [REA]. [F-t3-017](#F-t3-017), [F-t3-052](#F-t3-052).

### 7.8 Cursor 3.17.21

Cursor is the leading AI IDE (integrated development environment) harness. Verevon is not an IDE, so its harness ideas transfer and its editor features do not [INF]. It is best at typed request context, event-triggered automations and a careful edit engine [REA].

- **Typed request context with hydration flags.** A request carries what it was given (rules, history, memory) and a component-tree system prompt [SRC; the hydration flags are inferred from field names, INF]. Verevon's Do runs start with a fresh history and no persona, org or Space instructions: [F-t3-001](#F-t3-001).
- **Automations with event triggers, LLM filtering and dry runs** [REA]. [F-t3-047](#F-t3-047).
- **Background commands with output notifications, and early dispatch of completed tool calls** [REA]. [F-t3-070](#F-t3-070).
- **StrReplace edit engine.** A matching ladder, a structured result and a shared per-file lock; ripgrep-shaped grep with a broad-glob guard [REA]. Verevon has no edit tool at all: [F-t3-030](#F-t3-030).
- **Shadow lints.** A shadow workspace computes diagnostics for a change without touching the user's files [REA]. [F-t3-088](#F-t3-088).
- **Agent definition files** with tools, model and permission mode [REA]. [F-t3-019](#F-t3-019).
- **Design-mode point-and-comment on a pausable browser view** [REA]. [F-t3-026](#F-t3-026).
- **Docked trays** for plan, questionnaire, queue, goal and subagents above the prompt [REA]. [F-t3-011](#F-t3-011).
- **Firecracker cloud agents** and prebuilt images with health tracking [SRC, from agent prompt text and a client proto enum; the orchestration is server-side]. [F-t3-009](#F-t3-009), [F-t3-084](#F-t3-084).
- Not recommended: best-of-N parallel workspaces and git worktrees for parallel agents. They conflict with the 2026-08-24 decision-ledger ruling against workspace isolation for parallel agents and are IDE features [DOC, Verevon ledger; INF].

### 7.9 manus.im (public docs, October 2026)

Manus web rests on public documentation only [DOC]. It is best at making everything a task with an id and a URL, and at live view and take-over of a cloud computer [DOC].

- **Everything is a task with an id and URL** [DOC]. [F-t3-064](#F-t3-064).
- **Live view and take-over of the agent's computer, with a login carried over to similar tasks** [DOC]. [F-t3-026](#F-t3-026).
- **Wide research architecture** with parallel isolated sub-agents [DOC]. [F-t3-036](#F-t3-036).
- **Reference past tasks as context, browsed lazily** [DOC]. [F-t3-052](#F-t3-052).
- **Package this workflow as a skill** [DOC]. [F-t3-058](#F-t3-058).
- **Always-on cloud computers with chosen compute and region** [DOC]. [F-t3-085](#F-t3-085).

### 7.10 perplexity.ai (public docs, October 2026)

Perplexity rests on public documentation only [DOC]. It is best at verifiable answers: per-claim source checks, a multi-model council and the most layered documented injection defence [DOC].

- **Per-claim Check Sources** [DOC]. Verevon's citation chips are placed by client regex and there is no per-claim support view: [F-t3-053](#F-t3-053).
- **Model Council**: parallel multi-model answers with a synthesis of disagreement [DOC]. [F-t3-092](#F-t3-092).
- **Layered injection defence**: intent anchoring, a screening classifier, containment and runtime monitoring [DOC]. Verevon's semantic screening is a shadow pass: [F-t3-033](#F-t3-033).
- **Comet's per-domain Control, Read only or No access** [DOC]. [F-t3-008](#F-t3-008).
- **Per-tool allow, ask or never with thread grants** [DOC]. [F-t3-003](#F-t3-003).
- **Credit-aware pause** rather than cancel on exhaustion [DOC]. [F-t3-083](#F-t3-083).
- **Automation hubs** with Active, Paused and Completed and Run now, Pause, Edit, Pin and Delete [DOC]. [F-t3-019](#F-t3-019).
- **The Brain knowledge wiki** maintained from activity [DOC]. [F-t3-094](#F-t3-094).
- **Audit webhooks to a SIEM** (security information and event management) system [DOC]. [F-t3-004](#F-t3-004).

### 7.11 chatgpt.com (public docs, October 2026)

ChatGPT rests on public documentation only [DOC]. It is best at enterprise governance of connectors, models and audit, and at a deep research flow with an editable plan [DOC].

- **Connectors with read versus write gating** and graduated app permissions [DOC]. Verevon's connector reads are not proven end to end and writes are not built: [F-t3-010](#F-t3-010).
- **Compliance API**: an immutable log with a stateful API and per-category admin keys [DOC]. Verevon's audit outbox fix is built but not deployed: [F-t3-004](#F-t3-004).
- **Lockdown mode** and Requirements-versus-Defaults admin framing [DOC]. [F-t3-032](#F-t3-032).
- **Plan, role and org model entitlements** with admin defaults and caps [DOC]. Verevon's `allowed_models` field has no owning service: [F-t3-023](#F-t3-023).
- **Deep research with clarifying questions and an editable plan** [DOC]. [F-t3-027](#F-t3-027).
- **Curated widgets and Sites** [DOC]. [F-t3-044](#F-t3-044), [F-t3-045](#F-t3-045).
- **Meetings without a bot**, private by default with audio deleted after notes [DOC]. [F-t3-090](#F-t3-090).
- **MCP Events** and consumption of WebMCP site tools [DOC]. [F-t3-047](#F-t3-047), [F-t3-089](#F-t3-089).
- Caution rather than recommendation: ChatGPT retired group chats on 2026-07-09, which is a warning for multi-author Space threads [DOC]. [F-t3-013](#F-t3-013).

### 7.12 claude.ai (public docs, October 2026)

claude.ai rests on public documentation only [DOC]. It is best at open standards for skills and artifacts and at a per-conversation computer with graded egress [DOC].

- **Open SKILL.md standard** with three-stage loading and executable bundles [DOC]. [F-t3-014](#F-t3-014), [F-t3-058](#F-t3-058).
- **Artifacts as apps with a host bridge** (call the model, connectors and storage), SRI-pinned libraries and a frame on a separate origin [DOC]. [F-t3-045](#F-t3-045).
- **Data-connected dashboards with visible queries** [DOC]. [F-t3-091](#F-t3-091).
- **Four sandbox egress tiers** with plan-dependent defaults [DOC]. [F-t3-043](#F-t3-043).
- **Per-conversation VM** [DOC]. [F-t3-009](#F-t3-009).
- **Sensitive-topic exclusion for memory** and user view, edit, pause and reset controls [DOC]. [F-t3-033](#F-t3-033), [F-t3-020](#F-t3-020).
- **Project knowledge that switches from context to retrieval with a visible indicator** [DOC]. [F-t3-013](#F-t3-013).
- **Published feature lists excluded under ZDR** (Zero Data Retention) [DOC]. Verevon's own ZDR design already exceeds this; the blocker is attestation: [F-t3-005](#F-t3-005).

## 8. What Verevon already does well (protect it)

Verevon matches the references on the chat substrate and is ahead of every one of them on governance, tenancy, ZDR, provenance and honesty contracts; the gaps are in continuity of the agent loop, the conversation data model, extensibility that executes, and the surfaces users and admins see [SRC, code baseline; INF for the ranking]. Each item below names the code to protect and what no recommendation may regress.

- **Signed, fail-closed capability gate.** Every dispatch calls capability-core `EvaluatePolicy`; an Allow needs Ed25519-signed evidence; unknown tools fail closed [SRC]. Evidence: apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:501-552; apps/Model Plane/rust/services/execution-core/src/capability_policy.rs:741-836; apps/Model Plane/go/services/capability-core/internal/policy/engine.go. Do not regress: [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006), [F-t3-045](#F-t3-045) and [F-t3-070](#F-t3-070) route through the gate, never around it.
- **Explicit Ask/Do authority switch with a plan gate and a per-call autonomy ladder** (read_only, workspace_write, danger_full_access with justification) [SRC]. Evidence: apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:3030-3052; apps/Model Plane/rust/services/execution-core/tests/autonomy_ladder_contract.rs. Do not regress: named modes in [F-t3-032](#F-t3-032) resolve onto this ladder; no silent escalation from Ask to Do.
- **Two loops kept separate as an authority boundary.** The gateway loop refuses side effects; every effect goes through execution-core's capability, hook, permission and HITL (human-in-the-loop) pipeline; unification was rejected after measurement [SRC and DOC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:2430; apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:467; apps/Model Plane/docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md. Do not regress: [F-t3-001](#F-t3-001) shares the context resolver, not the loop.
- **ZDR by construction.** One retention resolver with a golden matrix, a signed claim that cannot be downgraded, typed localised refusals, a persistence-free temporary turn and a per-turn minimum privacy tier enforced before the network [SRC]. The design is built but not live: with no attested provider every ZDR turn is refused and Temporary chat is unavailable ([F-t3-005](#F-t3-005)). Evidence: apps/Model Plane/rust/crates/mp-contracts/src/retention.rs:1-5; apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs:1-5; apps/Model Plane/rust/services/inference-core/src/provider/zdr.rs:1-8; apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs:1-12; apps/Model Plane/rust/services/inference-core/src/config.rs:259-283. Do not regress: every new store in [F-t3-002](#F-t3-002), [F-t3-012](#F-t3-012), [F-t3-031](#F-t3-031), [F-t3-046](#F-t3-046) and [F-t3-050](#F-t3-050) refuses or degrades under ZDR and adds a golden-matrix case.
- **Honest outcomes.** Terminalisation receipts with `finalization_pending` instead of fake success, cost null when unknown with a cost basis, a grounding gate that refuses fabricated user-only arguments, deterministic read-back verification of provider writes and append-only tri-state action receipts [SRC]. Evidence: apps/Model Plane/rust/services/session-core/src/terminalization.rs:1-8; apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1123; apps/Model Plane/rust/services/execution-core/src/postcondition.rs:1-10; apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:1-40. Do not regress: [F-t3-028](#F-t3-028) and [F-t3-049](#F-t3-049) extend these contracts and never fabricate a number or a success.
- **Typed ChatEvent taxonomy and a byte-verbatim relay.** 23 event variants gated by feature families with control events never gated, a CI gate on producer and consumer coverage, and a BFF (backend-for-frontend) that relays bytes verbatim with Redis-backed resume across replicas [SRC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:144; apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs:99-109; apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1488-1760. Do not regress: new events in [F-t3-011](#F-t3-011), [F-t3-028](#F-t3-028) and [F-t3-049](#F-t3-049) go through the CI gate; the BFF stays allowlist-free.
- **Credential isolation as a plane rule.** Connector and subscription tokens live only in Integration Core, the BFF mints audience-bound delegated bearers per request, every service verifies RS256 JWTs before binding, and sandbox output is scrubbed [SRC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/auth.rs:1-20; apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs:1-6; apps/Model Plane/rust/services/execution-core/src/auth.rs:1-6. Do not regress: [F-t3-008](#F-t3-008) (model-blind logins), [F-t3-043](#F-t3-043) and [F-t3-065](#F-t3-065) keep secrets out of the model and the sandbox.
- **Model proposes, Quarry executes.** The execution-core planner proposes browser actions and Quarry executes or rejects them under a versioned contract with snapshot, generation and ref binding and no silent retargeting of effectful actions; fetches pass an SSRF (server-side request forgery) guard that resolves, vets and pins DNS [SRC]. Evidence: apps/Model Plane/rust/services/execution-core/src/browser_agent.rs:1-8; apps/Model Plane/rust/services/execution-core/src/llm_planner.rs:1-8; apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/dns_guard.rs:1-18; apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_egress_proxy.rs:1-80. Do not regress: [F-t3-008](#F-t3-008), [F-t3-026](#F-t3-026), [F-t3-050](#F-t3-050) and [F-t3-081](#F-t3-081) build on the contract; add a golden contract test shared by Model Plane and Quarry.
- **Kernel-enforced sandbox and Space-scoped CAS workspaces.** Bubblewrap plus Landlock and seccomp, probed at boot, with fail-closed AllowDomains; content-addressed workspaces with lease, snapshot and overlay-merge promotion into Data Plane documents that respect Space ACLs [SRC]. Evidence: apps/Model Plane/rust/services/execution-core/src/sandbox.rs:1-8; apps/Model Plane/rust/services/execution-core/src/workspace_cas.rs:1-8; apps/Model Plane/rust/services/execution-core/src/workspace_promote.rs:1-8; apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go:1-20. Do not regress: [F-t3-009](#F-t3-009) keeps this as the in-guest second layer and never lets a thread workspace bypass a Space ACL.
- **Context management ahead of the references.** Tiered compaction with protected pins, `reattach_context` after compaction, a compaction-recall eval in CI, and an itemised context inspector with real token counts per segment [SRC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/compaction.rs:1-20; apps/Model Plane/rust/services/model-gateway/src/context_reattach.rs:1-12; apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx:571-720. Do not regress: [F-t3-022](#F-t3-022) must not trade pins or recall for cache hits.
- **Memory provenance and learning signals.** `memory_recall` with stated, inferred or unrecorded provenance and in-place correct and forget; a bounded prefetch with a trivial-prompt gate; tiered feedback with an implicit-dissatisfaction classifier and Wilson-bounded quarantine; admin-gated fine-tuning [SRC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/memory_prefetch.rs:1-10; apps/Model Plane/rust/services/model-gateway/src/memory_provenance.rs:1-10; apps/Model Plane/rust/services/model-gateway/src/dissatisfaction.rs:1-15; apps/Model Plane/go/services/orchestrator-core/cmd/activities/quarantine.go. Do not regress: [F-t3-007](#F-t3-007), [F-t3-020](#F-t3-020) and [F-t3-061](#F-t3-061) add governance on top, not a second memory store.
- **Research honesty.** A time-sensitivity heuristic that forces search, fetch-then-answer grounding, a relevance gate, a Sources panel that separates "read and used" from "not read", a mandatory could-not-verify section in deep research, and server-enforced source scopes with a `source_scope_applied` receipt [SRC]. Evidence: apps/Model Plane/rust/services/model-gateway/src/grounding.rs:1-15; apps/Model Plane/rust/services/model-gateway/src/relevance.rs:1-15; apps/Model Plane/rust/services/model-gateway/src/deep_research.rs:1-48; apps/Model Plane/rust/services/model-gateway/src/normalize.rs:58-62; apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx:356-538. Do not regress: [F-t3-027](#F-t3-027), [F-t3-053](#F-t3-053) and [F-t3-054](#F-t3-054) keep the read-versus-not-read distinction and the receipt.
- **Rendering, accessibility and a calm workspace.** An own Markdown parser, Mermaid in strict mode rendered only after the fence closes, accessible SVG charts with a data table and keyboard navigation, Office exports built in the browser with no network call, a live region that announces lifecycle rather than tokens, and a tabbed pane opened by the work in a fixed priority with Trace never stealing focus [SRC]. Evidence: apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1086-1600; apps/Frontend Plane/verevonv3/src/features/chat/components/ChatChart.tsx:1-15; apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-export/index.ts:1-60; apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx:231-283; apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx:449-540. Do not regress: [F-t3-021](#F-t3-021) keeps accessibility when virtualising and [F-t3-048](#F-t3-048) keeps the calm-by-default pane rule.
- **Multi-tenant, server-side runtime.** Runs continue after the client closes, streams resume across gateway replicas, session-core is the single conversation writer with a Postgres store, tenant scoping and GDPR erasure, lease-based outboxes, and proto-first contracts with breaking-change CI; plane ownership is a stronger rule than any reference's package layering [SRC]. Evidence: apps/Model Plane/proto/model_plane/v1/sessions.proto:159-193; apps/Model Plane/rust/services/session-core/src/main.rs:190-195; apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs:272-277. Do not regress: [F-t3-016](#F-t3-016) and [F-t3-064](#F-t3-064) build on it; bytes keep flowing through Data Plane APIs, never across plane databases.
- **Intent-based routing with provenance.** Budget, Balance and Genius combine task complexity with the org's cost posture, a tool-use floor and a fallback ladder, editable at runtime; the served-by line carries provider, declared residency and the strongest substantiated privacy tier from server-stamped values [SRC]. Evidence: apps/Model Plane/rust/services/inference-core/src/provider/intent.rs:1-60; apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs:127-222. Do not regress: [F-t3-042](#F-t3-042) and [F-t3-073](#F-t3-073) extend the resolver and the served-by line rather than replace them.
