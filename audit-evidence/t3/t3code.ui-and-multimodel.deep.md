# T3 Code 0.0.35 - UI/UX and multi-model design (Audit 3, facet "ui-and-multimodel")

Deep-dive analyst: Sonnet. Source: T3 Code desktop build 0.0.35 (installed program files plus the pre-extracted copy and the source maps shipped inside the app). Read-only audit: the app was never launched, no user-data directories were read, no project code was run.

Companion files: `t3code.ui-and-multimodel.features.json` (109 evidence-backed records, ids `t3code-ui-and-multimodel-001..109`) and the scout inventory `t3code.inventory.md`.

## 0. How to read this

Provenance tags: **SRC** = read in recovered app-owned TypeScript (from shipped `.map` `sourcesContent`) or shipped CSS/HTML; **REA** = observed through the REA tool; **DOC** = in-source comments or help text; **INF** = my inference (basis stated). Confidence is "high" unless marked.

Evidence path aliases (all under the audit scratchpad `SCR`):

| Alias | Meaning |
|---|---|
| `spa/` | `SCR/tmp/uim_spa_src` - the SPA sources merged from the source maps of the main bundle plus the lazy chunks (`textarea` shared chunk, `FilePreviewPanel`, `DiffPanel`, `PreviewPanel`, `PullRequestCodeTab`, comment annotation chunks). Contains `spa/src/**` (about 556 files, about 154k lines of TS/TSX) and `spa/packages/{contracts,shared,client-runtime}/src/**` (about 40k lines). |
| `srv/` | `SCR/tmp/t3code/recovered/server` (about 118k lines of TS). |
| `dsk/` | `SCR/tmp/t3code/recovered/desktop`. |
| `X/` | `SCR/tmp/t3code/server-x/apps/server/dist` (built SPA assets: `client/index.html`, `client/manifest.webmanifest`, `client/assets/index-DSuALXPn.css`). |

Important correction to the scout: the scout recovered only the main bundle map. The shared chunk (`textarea-*.js.map`) holds the contracts package, most of `client-runtime`, the stores (`composerDraftStore`, `rightPanelStore`), the `ui/button|menu|select|tooltip` primitives, and the model-selection code; the lazy chunks hold the whole diff-review, preview and pull-request-code UI. I recovered those maps too (about 130 more files). Everything below that cites `spa/` is therefore broader than the scout inventory.

### REA usage log (honest)

REA was used, but it contributed little beyond structure, because the SPA is a minified Vite bundle and REA's string-seed tracing does not index string literals in this graph.

- `analyze_javascript_application` (format directory, detail summary, integrity record-and-continue, heap 2048 MB) over four shipped SPA chunks copied into scratch (`index-CI6tzIRc.js`, `textarea-9EnFLFB7.js`, `FilePreviewPanel-rne2wibz.js`, `worker-CIcqvLjo.js`). Result `ev_c4296b0b3e57b255897c980d2722151300d0cacf0f9ff08751a9887f8e23af99`: 4 JavaScript files parsed, 8,016,506 text bytes, 2,018,097 AST nodes, 1,775 findings, 0 parse failures, coverage partial (the worker exhausted its heap in the semantic pass for the largest chunks). [REA]
- `inspect_analysis_view` on that evidence (modules page): 1,320 JavaScript module nodes recovered from bundler factories across the four chunks (paths are chunk names, i.e. no original source names), which corroborates the code-split layout. An attempt to re-open the scout's server-bundle evidence failed because it is not retained in this session. [REA]
- `trace_application_feature` with string seeds `sidebar.toggle` (exact) and `t3code:prompt-stash` (contains): both returned no match (graph partial; the tool states this is not evidence of absence). [REA]
- Loaded but not run: `analyze_web_bundle`, `recover_javascript_sources`, `trace_web_module_imports` (need a CDP page or Linux tooling).

Everything else is SRC from the recovered TypeScript, which is far more informative than REA output for this app.

## 1. Executive summary

T3 Code is a local-first, multi-model **agent harness UI**: it does not call model APIs itself; it drives five agent CLIs/SDKs (Claude Agent SDK, Codex app-server, Cursor and Grok over ACP, OpenCode) through one canonical event stream, and renders their work in a very polished React 19 / Electron / Tailwind v4 interface. It is a coding tool (git worktrees, diffs, PRs, terminal, preview browser), not a general chat or research product.

What it is best at, in order of value to Verevon:

1. **Attention management for parallel agents.** A single inbox-style sidebar with a closed status vocabulary and deterministic precedence (Pending Approval > Awaiting Input > Working > Plan Ready > Monitoring > Completed), pins, snooze with event-based wake, settle with PR-aware auto-settle, "Woke" notices, and banners that explain every hidden state (records 020-022, 099).
2. **Multi-provider model plumbing.** An open `driver kind` vs `provider instance` split, per-instance accounts, continuation groups, capability descriptors for reasoning/fast/context options, per-protocol discovery, CLI-version gating, and a model picker with a provider rail, fuzzy search, favorites and legacy fold (records 001-011, 055-058, 086-090).
3. **A canonical run-event taxonomy** (48 event types, 16 canonical item types, 10 request types) that normalizes five very different protocols and keeps the raw payload (record 056).
4. **Timeline craft.** Virtualized list with a strict live-follow state machine, settled-turn folding ("Worked for 2m 14s"), grouped tool summaries, a left-edge minimap, steering by sending while running (records 012-015, 107).
5. **A power-user input and navigation system.** VS Code-style context keybindings with a conflict-aware editor, three-mode search overlay, composer with inline chips, panel launcher with letter shortcuts (records 023, 027, 037, 045).
6. **Server-side rigor.** Serial command queue with idempotent receipts, transactional event append, snapshot-then-resume sync with per-thread watermarks, shell/detail projections, cache-first rendering (records 047-048, 091, 108).

Where it is weak (relative to what Verevon needs): no citations/research UI, no math/diagrams/charts in chat, no memory/personalization, no voice, no image generation, no edit/regenerate/branch/share/export of conversations, no semantic search, no OS notifications, token streaming demoted to "legacy", default runtime mode is **Full access**, single-user trust model, no i18n, and no user-facing MCP/plugin management. Several of these are areas where Verevon already leads (see section 13).

## 2. Architecture and data model

### 2.1 Process and protocol picture

```
Electron main (dsk/)  --spawns-->  Node server (srv/, bin.mjs from server.asar)  --drivers-->  Claude SDK | Codex app-server | ACP (Cursor, Grok) | OpenCode serve
   |  window.loadURL                    |  WebSocket RPC /ws (Effect RPC)                       |  per-session MCP endpoint /mcp (t3-code tools)
   v                                    |  HTTP: snapshots, attachments, assets, OTLP proxy
SPA (spa/, React 19 + TanStack Router)  <-- also served to browsers, phones and the hosted static app (app.t3.codes)
```

- The desktop wrapper contains no UI; the SPA lives inside the server bundle and is loaded from the server URL [SRC; scout]. The same SPA runs as a PWA (`manifest.webmanifest`, standalone) and as a hosted static client that attaches to user-owned servers via pairing links (record 093) [SRC].
- Clients address servers through **environments** (primary local, paired bearer, managed relay, SSH, WSL); every ref is scoped by `environmentId` because ids are only unique per environment (record 049) [SRC].

### 2.2 Event-sourced orchestration (what the UI renders)

- UI actions are **commands** with ids; the engine processes them from one unbounded queue, appends events and updates projections in one SQL transaction, then publishes to a PubSub; replaying a command id returns the stored receipt (accepted sequence or rejection) (record 091) [SRC `srv/src/orchestration/Layers/OrchestrationEngine.ts`].
- Events carry `sequence`, `commandId`, `causationEventId`, `correlationId` and metadata (`providerTurnId`, `providerItemId`, `adapterKey`, `requestId`, `ingestedAt`, client `origin`) [SRC `spa/packages/contracts/src/orchestration.ts:1359-1400`].
- Read models: `OrchestrationThreadShell` (cheap summary with pending-approval/input flags, `backgroundLiveness`, `planProgress`, settle/snooze/pin fields) and `OrchestrationThread` (messages, activities, plans, checkpoints) (record 048).
- Sync: snapshot over HTTP (gzip) -> WebSocket subscription with `afterSequence` -> dedupe by sequence; windowed history by `turnLimit`/`beforeCursor` with a per-thread `threadSequence` watermark so older pages are merged only after the live stream passed that point; capability advertisement gates pagination for old servers (record 047) [SRC].
- Client cache: IndexedDB `t3code:connection-runtime` v4 (catalog, shell, thread, server-config, vcs-refs) gives instant first paint with a "Loading/Syncing messages..." pill (record 108) [SRC].

### 2.3 Client state layering

- Server-derived state: Effect atoms (`@effect/atom-react`) over an RPC client; platform services injected through Effect `Context` so the same `client-runtime` package serves web, desktop and a mobile app that is in the monorepo but not in this build [SRC + INF for the mobile app].
- UI-only state: small zustand stores with persisted, **versioned** shapes and migrations (`right-panel-state:v2`/v11, `composer-drafts:v1`/v8, `prompt-stash:v2`, `ui-state:v1`, diff panel, terminal UI, preview mini player) (record 050).
- Settings are split into client-local (`ClientSettingsSchema`) and server-authoritative (`ServerSettings`) with defaults-by-decoding and deliberate "fresh key" renames to reset retired behaviours (record 060).

## 3. Multi-model design

### 3.1 Driver, instance, continuation (records 001, 002, 003, 055)

- `ProviderDriverKind` is an **open branded slug** (not a union): unknown drivers must parse and surface as `unavailable` shadow snapshots. `ProviderInstanceId` is the routing key; threads/sessions/events reference instance ids only. `defaultInstanceIdForDriver(kind) = kind` keeps legacy data routable [SRC `providerInstance.ts`].
- Instances carry `displayName`, `accentColor`, `environment[]` (with sensitive flags), `enabled`, and opaque `config` decoded once by the driver's own schema. Multiple accounts of one CLI coexist through home-path isolation (`CODEX_HOME`, `shadowHomePath`, `CLAUDE_CONFIG_DIR`) (record 007).
- The server driver SPI is plain values: `ProviderDriver<Config,R>` -> `ProviderInstance{snapshot, adapter, textGeneration, continuationIdentity}`; no per-driver singletons; instances share no mutable state (record 055).
- `continuation.groupKey` says which instances can continue the same session (same underlying login). After a thread starts, the picker is locked to the thread's **driver** and (if present) continuation group; `requiresNewThreadForModelChange` blocks any model change for providers that cannot switch (record 003).

### 3.2 What the UI learns about a provider (records 002, 005, 089, 090)

One `ServerProvider` snapshot per instance: install/enabled, `auth{status,type,label,email}`, probe `status` (ready/warning/error/disabled), version, `models[]` with `capabilities.optionDescriptors`, `slashCommands[]`, `skills[]`, `versionAdvisory`, `updateState`, `availability`. Model discovery differs per protocol: Claude uses a static catalog filtered by installed CLI version (with "Claude Code vX is too old for Opus 5, upgrade to vY" messages), Codex reads a live `model/list` (reasoning efforts and service tiers become descriptors), Cursor/Grok use ACP config options and the unstable `session/set_model`, OpenCode lists configured sub-providers (`subProvider` labels).

`optionDescriptors` are `select` or `boolean` with `isDefault`, `currentValue` and `promptInjectedValues`. The composer renders them generically (reasoning effort including "Ultracode" and prompt-injected "Ultrathink", fast mode, context window 200k/1M, agent). Selections are stored as `{id,value}[]` on `ModelSelection.options`; legacy object shapes decode through a transform and DB migration 026.

### 3.3 Switching rules and session restarts (records 003, 057)

Per-turn `modelSelection` is part of `ProviderSendTurnInput`. Adapters advertise `sessionModelSwitch: "in-session" | "unsupported"` (Claude and Codex are in-session). `ProviderCommandReactor` restarts the provider session when runtime mode, cwd, instance or (for unsupported adapters) model changes, resuming with the cursor unless the model changed. Claude restarts on any selection change. The UI mirrors this with blocking toasts rather than letting an unsafe switch through.

### 3.4 Picker and curation UX (records 004, 086-088, 097)

- Popover with a left rail (Favorites + one icon per enabled instance, account badge when several share a driver), a virtualized model list, tokenized fuzzy scoring over name/shortName/subProvider/driver/instance name with a +24 favorite boost, a collapsible "Legacy models" group per instance, Cmd/Ctrl+1..9 jumps (`modelPicker.jump.N` when `modelPickerOpen`), `modelPicker.toggle` (Cmd+Shift+M), `/model` in the composer, tooltips explaining unavailable instances/models, length-prefixed composite keys so slugs containing colons parse safely.
- Settings: per-instance custom models, hide, reorder, star; client `providerModelPreferences{hiddenModels,modelOrder}`; sticky last choice per provider; per-project default model; remote-updatable **model manifest** (hourly refresh from the project's main branch; remote -> last good copy -> bundle) decides current vs legacy (record 058).

### 3.5 Utility models (record 059)

A cheap separate model (default `gpt-5.6-luna`, reasoning `low`; Claude haiku-4-5; Cursor composer-2) writes titles, commit messages, branch names and PR text with JSON-only prompts. Title prompts target "recognizable weeks later", have a regenerate variant that sees the previous title, and writing-style presets (`repo_conventions`, `conventional_commits`, `custom`).

### 3.6 What multi-model does **not** include [SRC; absence by search]

No side-by-side comparison, no router/fallback chain (only Codex's server-side `model/rerouted` notice is surfaced), no cost/latency columns in the picker, no local-model integration beyond what OpenCode offers, no prompt-cache controls. "Multi-model" here means "many agent backends, one UI", not "one chat, many models per answer".

## 4. Chat and agent UI

- **Timeline** (records 012-015, 018, 079): `LegendList` virtualization; follow re-arm band of 40 px (comment documents that the library's near-end check yanked readers down); anchor-on-send; composer floats over the list with an end inset; disclosure toggles suspend end-pinning for two frames; structural sharing of rows; self-ticking timers that write DOM text; "Scroll to end" pill; "Load earlier" header; min-2-item left-edge minimap with hover cards (user prompt + final answer) whose hit strip is capped to the gutter. Settled turns fold everything between the first and terminal assistant messages behind "Worked for ..."; unsettled state derives from turn lifecycle, not transient "working", to avoid flicker.
- **Work log** (record 014): `WorkLogEntry` with tone, lifecycle status, command, changed files, agent role; consecutive tool calls group into summaries ("Read 4 files, ran 2 commands and searched the web 1 time"); latest row stays visible; superseded in-progress markers collapse into completed twins; shell commands are tokenized to friendly program names; a heuristic reclassifies "web_search" items containing `grep` as code search. Summaries are rule-based (no LLM summaries found).
- **Assistant delivery** (record 079): the server buffers assistant text per message and emits it at completion (or when an approval/question opens, or after 24,000 characters). Token streaming is a "legacy" setting. Cost: no live typing feel; benefit: fewer events and no partial-markdown churn.
- **Markdown** (records 016-017): react-markdown + GFM + raw HTML sanitized, GitHub alerts, task lists, details, tables with expand and copy-as-Markdown/CSV, code blocks with fence titles, wrap and copy, Shiki with a size-aware LRU cache (bypassed while streaming), file-path chips that open in the panel at a line, in the editor, in the browser, or revealed, workspace images through signed URLs. No math, Mermaid, charts or citations.
- **Composer** (records 027-035, 103): Lexical editor with atomic inline chips for `@file`, `$skill` and terminal selections; `/model`, provider slash commands, `/skill:name`; Enter/Shift+Enter/Cmd+Enter(background) semantics (mobile: Enter inserts a newline); type-anywhere-to-focus; surround-selection; footer with runtime mode, plan/build toggle, traits, model picker, context ring, Stop and Send (Send stays available while running for steering); image attachments with background upload (3 per environment), HEIC conversion, client downscale to the 10 MB cap, 8 images, 120k characters; drafts persisted (v8 migrations, 300 ms debounce) and a provider-agnostic prompt stash (20 entries).
- **Approvals and questions** (records 029-030): inline strip with the exact command/file/app request and Cancel / Decline / Always allow this session / Approve (provider-supplied options win); multi-question stepper with options and free text; collapsible so it does not cover the thread.
- **Plans and tasks** (record 031): proposed-plan card (copy, download, save to workspace), "Plan ready" banner, implement-in-new-thread, segmented task progress bar; the Plan/Build toggle is a legacy beta flag.
- **Context** (record 032): ring + popover (used/max, total processed, auto-compact threshold) with Compact (injects the native `/compact`), and a stale-session prompt (70 min, 100k tokens) for Claude.
- **Steering and background work** (records 107, 099): sending during a run steers the same turn; lifecycle banners show "N agents working/Monitoring" with Stop, and explain snoozed/settled/woke threads.

## 5. Workspace surfaces

- **Right panel** (records 036-037): per-thread ordered surfaces (browser tabs, terminals with splits, diff, files, file, pull-request tabs, agents); store keeps descriptors only, features own their resources; context menu (copy path, mute, close, close others/right/all); letter-shortcut launcher (B/T/F/D/P/A) with a typing-context guard; inline at wide widths, sheet at <= 980 px; maximize.
- **Diff review** (records 038-041): scopes per turn / branch (automatic or chosen base) / working tree; stacked or split; word wrap; whitespace ignore; collapse state per scope; inline line comments become `<review_comment>` blocks in the next prompt and render back as cards; each answer ends with a "Changed files" card that auto-expands only for <= 5 files and <= 200 changed lines.
- **Checkpoints** (record 081): hidden git-ref checkpoints per turn; "Revert to this message" restores files and rolls back the provider conversation.
- **Browser / computer use** (records 043, 073, 100, 102): Chromium `<webview>` shared by human and agent; controller badge ("Agent controlling browser"), visible agent cursor, PiP mini player; 14 MCP tools with locator-first targeting (Playwright locator strings via an injected Playwright runtime), semantic snapshot, readonly/destructive annotations; element picking (React component + source), region marks, strokes, style-change requests and a screenshot as one structured annotation; recording to MP4/WebM at 12 fps. Desktop-only; `enableAgentBrowserAccess` removes both tools and their prompt text.
- **Terminal** (record 066): Ghostty VT WASM on canvas, IME-safe, selection popup with "Add to chat" (`<terminal_context>`).
- **Agents panel** (records 042, 109): fixed-height rows, one "Working" state for all in-flight statuses, "Idle - resumable", phase-grouped workflows with live counters and a read-only script viewer.
- **PR workspace** (record 044): list/detail tabs, batched review with a verdict; not relevant to a non-IDE product beyond the batched-review store pattern.

## 6. Navigation and organization

- **Sidebar** (records 020-022, 080, 082): attention-ordered inbox; pinned (dnd-kit), snoozed, settled tail; status pills with colour/pulse; unseen-completion by `lastVisitedAt`; project grouping modes (repository, repository_path, separate); multi-select bulk actions; thread jump hints (Cmd+1..9, 200 ms delay); resizable width (clamped to keep main content >= minimum). Lifecycle logic is pure functions with client/server twin invariants (e.g. you cannot snooze or settle a thread that is waiting on you).
- **Palette** (record 023): Cmd+K (commands, projects, threads, content snippets), Cmd+P (files), Cmd+Shift+F (content) share one reducer so they never stack; add-project flows (local, clone, WSL, remote).
- **Keybindings** (record 045): default map, `when` expressions (`terminalFocus`, `previewFocus`, `modelPickerOpen`...), last matching rule wins, user `keybindings.json` (<= 256 rules), per-project script bindings, conflict detection, key capture.
- **Settings** (records 046, 098): eight sections, ~40 searchable settings with anchor + pulse, reset-to-default per setting, per-project settings, explicit legacy labels.

## 7. Visual system and craft

- Tailwind v4.3 with semantic oklch tokens, glass tokens, Base UI primitives, CVA variants (13 button sizes, 9 variants), `data-slot` attributes reused as global keyboard-guard selectors (records 063, 095).
- Theme engine (record 024): 57 colour roles shared with the mobile app, five built-ins, light/dark "halves" chosen independently, a palette generator with gamut mapping and contrast solving, VS Code theme import, Open VSX install, a live editor above the router with an element inspector; boot-time inline script paints the splash in the selected theme (record 068).
- Density and typography (record 025): four font families, four size scales, contrast 50-200, glass 40-100, grayscale smoothing; default interface font 16 px with 12-13 px controls on desktop.
- Responsive (record 069): offcanvas sidebar, sheet panel, mobile Enter semantics, 16 px inputs on coarse pointers, View Transitions on phones, hover gated by `@media (hover:hover)`.
- Accessibility and motion (records 052, 105): reduced-motion honoured in 6 places and in CSS; forced-colors blocks; focus-within for hover actions; `role=progressbar` on the context ring; no i18n, no a11y tests observed.

## 8. Transparency and control surface

In-app diagnostics (live processes with SIGINT/SIGKILL, resource history, trace analytics with failures and slow spans, logs folder) (record 076); usage/cost page derived from provider transcripts (record 054); slow-request toast at 15 s (record 053); per-provider NDJSON event logs (scout); context ring; work log; agent roster token counts; version-skew prompts (record 083). Not present: per-answer cost badges, live spend meter, reasoning trace panel (reasoning appears only as a "thinking" tone row), benchmarks/evals, audit log.

## 9. Safety and governance observations

- Runtime modes: Supervised, Auto-accept edits, Auto, Full access; **default Full access** (records 029, 062).
- Capability gate removes tool and prompt text server-side (record 061); per-session MCP credential (record 077); scoped, revocable sessions with DPoP (record 094); hold-to-quit (record 067).
- No SSO, org roles, ZDR or data-retention controls, no prompt-injection defenses observed; the model manifest and triage playbook fetch from a public branch (trust boundary, record 058; scout).

## 10. Best-in-class (relative to the peers in this audit)

1. Attention inbox + lifecycle (settle/snooze/woke) - nobody else in the reference set models "needs me / done / later" so explicitly.
2. Canonical run-event stream and driver SPI - the cleanest provider-normalization layer seen.
3. Timeline follow/anchor/fold mechanics and grouped tool summaries.
4. Keybinding engine with when-clauses and a conflict-aware editor.
5. Snapshot-then-resume sync protocol with per-aggregate watermarks and capability-gated pagination.
6. Theming system (role tokens, contrast solver, live inspector).
7. Idempotent command queue with causal metadata (portable to Rust almost verbatim).

## 11. Weaknesses and limitations

- **Scope**: coding-first; chat/research/multimodal breadth is thin (images only; no voice; no generation; no artifacts rendered inline; no citations).
- **Conversation model**: no edit-and-resend, regenerate, branch, share, export; search is SQL `LIKE` with limit 50.
- **Streaming**: buffered by default.
- **Governance**: default Full access; single-user trust; no org policy.
- **Dependence on CLIs**: provider capabilities, login and updates are outside the app; "Sign in via the CLI" is the only auth path; manual compaction only for Claude.
- **Complexity**: `ChatView.tsx` is 7,334 lines, `Sidebar.tsx` 3,960, `ChatComposer.tsx` 3,599, `ConnectionsSettings.tsx` 3,463; Effect 4.0 beta and `@pierre/diffs` beta are load-bearing; Playwright internals are extracted by string markers.
- **Platform gaps**: preview/computer-use is desktop-only; Windows background service unsupported (scout); no desktop notifications.
- **Unverified**: runtime behaviour, defaults and first-run flows (never launched); desktop updater internals; whether non-Claude adapters receive the same MCP server.

## 12. What Verevon should copy, adapt or avoid

Baseline used: `v-chat-agents-ui.baseline.md` (Verevon already has: SSE with resume, rich event taxonomy, KaTeX/Mermaid/charts/citations, plan gate with authority rungs, approvals, context inspector, run Trace, cost views, run notify watcher, memory/skills runtime, i18n no/en, screen-reader narration).

**Copy (high value, low conflict)**
- Timeline mechanics: virtualization, follow re-arm band, anchor-on-send, settled-turn folding, grouped tool summaries (addresses baseline gap "no virtualization, full reparse per token").
- Command palette + customizable keybinding engine (baseline: none).
- Attention-ordered run list with closed status vocabulary and unseen-completion (baseline: Fleet console stub, thread list capped at 80).
- Composer chips for Data-plane objects (documents, people, skills, sources) and type-anywhere-to-focus.
- Right-panel "workspace of surfaces" per run (document viewer, browser session, sandbox terminal, trace) with descriptor-only store.
- Slow-request toast, condition-keyed banners, thread-scoped toasts, cache-first rendering.

**Adapt**
- Driver SPI and canonical event taxonomy -> Model Plane adapters (Rust traits); add item/task/hook granularity where Verevon's `ChatEvent` is coarser.
- Instance-scoped identity -> BYO subscriptions and org keys; keep intent modes (Budget/Balance/Genius) and privacy-tier floors as the primary UX, with an explicit "pin a model" path using T3's picker.
- Settle/snooze -> agent runs and scheduled work, tied to external state (PR merged, ticket closed) and run watchers.
- Idempotent commands + sequence-resume sync -> replace the in-memory 10-minute resume buffer.
- Checkpoint/rewind per run for document and record mutations (baseline gap).
- Diff-style review with anchored comments for generated artifacts.
- Structured visual annotation (marks, strokes, requested changes + screenshot) for artifact editing.

**Avoid**
- Default Full access; buffered-by-default delivery for chat; literal-only search; localStorage-only drafts; credentials in client IndexedDB; extracting third-party internals by string markers; unbounded complexity in one 7k-line view component.

## 13. Rust and Solid portability

- Pure, directly portable: thread status/precedence, settle/snooze rules, turn folding, tool grouping, keybinding parser + `when` AST + matcher, fuzzy scoring, model ordering, option-descriptor logic, version-skew compare, command receipts, decider/projector.
- Needs replacement: Effect service graph (-> traits/actors/axum state), node-pty (-> portable-pty), fff native index, Ghostty VT WASM (usable as is in browser), Lexical (Solid has no equivalent; evaluate ProseMirror/TipTap or CodeMirror 6 for chips), LegendList (-> a Solid virtual list), Base UI (-> Kobalte/Ark), `@pierre/diffs` (check for a framework-agnostic core).
- Contracts: Effect Schema -> serde types with generated JSON Schema (settings forms rendered from schema annotations, record 008).

## 14. Corrections to the scout inventory

- Runtime modes are four (`approval-required`, `auto-accept-edits`, `auto`, `full-access`), default `full-access`; the scout listed three.
- Reduced-motion handling exists (verified in 6 source files and CSS).
- Settings "Integrations" contains only Browser defaults; there is no UI for user MCP servers.
- `Search` is SQL LIKE over active threads; limit 50.
- Assistant text is buffered by default; token streaming is the legacy mode.
- Manual compaction is Claude-only and works by injecting `/compact`.
- Model catalogs for Claude are static and CLI-version-gated; Codex is live; Cursor/Grok via ACP config.
- Source-map coverage was extended (shared and lazy chunks), adding contracts, client-runtime state, diff/preview/PR UI.

## 15. Open questions

- How do the Codex, Cursor, Grok and OpenCode adapters expose the `t3-code` MCP server (only Claude was traced)?
- What are runtime defaults for telemetry, auto-settle and keybindings on first launch (never launched)?
- Does `model.rerouted` (Codex) change the picker state or only show a notice?
- How does `orchestration v2` (mentioned in the subagent runtime header) change the thread model, and is the v1 fold already scheduled for removal?
- Is the hosted static app's trust and pairing flow reviewed for token leakage (tokens travel in the URL fragment and are stripped after exchange)?

## Appendix A. Record index

See the generated index below (id, category, quality, name).

