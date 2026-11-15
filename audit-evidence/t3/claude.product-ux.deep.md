# Claude desktop 2.31226.1: product-surface deep dive (Audit 3, facet "product-ux")

Date of audit: 2026-10-13. Read-only static analysis. Nothing was launched, installed, or modified. Per-user data directories (AppData/Roaming/Claude, AppData/Local/Claude*) were not read. No secret-bearing file was opened.

Companion files:
- Feature records (155): `findings/t3/claude.product-ux.features.json`
- Scout inventory (broad, mostly main-process): `findings/t3/claude.inventory.md`
- Verevon baseline used for gap mapping: `findings/t3/v-chat-agents-ui.baseline.md`
- Reproducibility: generator and helper scripts in `tmp/claude-ux/` (gen/*.cjs build the JSON and verify that every evidence pointer exists at generation time; `events.txt` holds the 2,885 analytics event names; `modules_tier.txt` the 257 lazy modules; `ion-subset/` the 63 core JS chunks given to REA).

Provenance tags used throughout: **REA** = observed through REA analysis of a shipped artifact; **SRC** = read directly in shipped files (including the installed program directory); **DOC** = public documentation (read through search summaries, not full pages); **INF** = my inference, basis stated.

---

## 0. Executive summary: ten things that matter for Verevon

1. **The product is two layers, and the scout inventory missed the bigger one.** The Electron shell (asar, about 44 MB) is a thin host. The product UI is the claude.ai single-page app. The installed program also ships a full local copy of that SPA at `resources/ion-dist` (3,562 files under `assets/v1`, 140.8 MB, 34,213 UI strings, 3,333 chunks per its scc.json report), which the shell serves over `app://localhost` in custom third-party mode with a local stub API. This is the single best source for product behaviour and it is not in the asar. [SRC]
2. **Claude controls its own product through tools.** Claude Code Desktop registers about 30 built-in MCP servers with fixed ids. The `ccd_*` family lets the model manage sessions, the sidebar (sections, groups, views), side panes, window layout, app settings, PRs, connectors, host actions, turn lifecycle (plan and wrap-up), and more. Every UI affordance is a typed tool with mode-aware approval. [SRC; observed live in this session's tool list]
3. **Chat results are generative UI, not text.** 47 lazily loaded "tool cells" (maps, charts, quizzes, recipes, stocks, weather, itinerary, products, comparison tables, MCP app widgets, scheduled-task receipts, skill proposals...) render typed tool output with fixed-height skeletons. Verevon's conversation-node registry is the right foundation; Claude shows how far to take it. [SRC]
4. **Artifacts are a platform.** Versions with a pinned shared version, roles (view/comment/edit), org/public/email sharing, TTL, comments that can summon Claude, per-artifact database/files/room, connector calls on the viewer's own connection with per-use approval, and a sandboxed frame runtime with a pinned capability manifest. [SRC + DOC]
5. **The session start contract is the real API.** One validated options object (about 48 fields) carries system prompt sections, memory-guideline template, inline-widget prompt, MCP lists with withheld hosts, selected folders/projects, egress allow-list, CLI policies, OTLP config, feature toggles, fork cut point, scheduled-task and space ids. The client sends structured intent and toggles; prompts are server-composed. [SRC]
6. **Rust is already inside the client.** Two Rust-built WebAssembly modules ship in the web app: a 1.3 MB collaborative Markdown engine (`yrs` 0.26 + `pulldown-cmark` 0.13) and an 8.2 MB module with `c2pa` 0.90 plus PDF/image/MP4/X.509 crates (content-credential style stamping and parsing). The desktop native layer is also Rust (NAPI-RS). [SRC]
7. **Permission UX is a product, not a dialog.** Auto mode (classifier gating, now the default), Accept edits, Plan, Bypass; approval scopes (once, session, always) with admin switches; one approval card for destructive batches in every mode; pinned Git-origin protection for repo-provided agent config; hash-pinned skill and plugin sources; a security-scan gate before skills go live. [SRC]
8. **Agent work is organised around a small closed vocabulary of states and a plan contract.** Working, Needs input, Plan ready for review, Ready for review, Completed, Archived; a pinned plan the model must maintain; an end-of-turn "next step" card with typed actions; session chapters; side sessions offered when scope changes; background-task backlog. [SRC]
9. **Performance is designed in the module graph.** Every lazy feature is registered with a tier (T1 51, T2 65, T3 141), a large-closure flag and a failure policy; the build emits a chunk-cycle report; transcripts load tail-first and evict under memory pressure. [SRC + REA]
10. **Governance surfaces are wide and declarative.** One settings registry (scopes, version gates, fail-closed values, surface grants) drives UI, policy and MDM export (registry, ADMX, plist, JSON, firewall allowlist); ZDR and retention are per-product org settings with propagation notices. [SRC]

---

## 1. Scope, method and corrections to the scout inventory

### 1.1 What was read

| Source | Use |
|---|---|
| Extracted asar (`extract/claude`, 478 files) | main-process bundles, preload scripts, local renderers (quick entry, buddy, about, find, main window), in-app tool definitions (`index.chunk-1xGiXs9t.js`), 1,266 distinct formatjs messages |
| Installed program dir `app/resources` (read-only) | `ion-dist` (web client), 43 locale JSONs, `claude-browser-shim.js`, `cowork-svc.exe`, `smol-bin.x64.vhdx`, SSH helper binaries, `chrome-native-host.exe` (names and sizes only; no binary was executed or analysed) |
| `ion-dist/i18n/en-US.json` | 34,213 strings; keyword mining per product theme |
| `ion-dist/assets/v1/*.js` | analytics event names (2,885 distinct `claudeai.*`), lazy-module registry, tool cells, wasm crates, route classifiers |
| `ion-dist/_frame-rt` | artifact frame runtime manifest and capability modules |
| DOC | support.claude.com and claude.com pages seen through search summaries (Cowork, memory, incognito, artifacts, Claude Design); code.claude.com desktop docs as read by the scout |
| This session's own tool schemas | `mcp__ccd_*`, `Artifact`, `visualize`, `Claude_Browser`, `claude-in-chrome`, `mcp-registry`: observed live, matching the static server-name table |

### 1.2 REA usage log (tools actually run)

- `analyze_javascript_application`: input = a scratch copy of the 63 core ion-dist chunks plus `index.html` (index, vendor-*, shared-*, frame-*); `format: directory`, `detail: summary`, `integrity_policy: record-and-continue`, `max_heap_mb: 2048`, `analysis_timeout_ms: 900000`. Result: 64 relevant files, 63 parsed JS files, 15,534,030 text bytes, 3,874,991 AST nodes, 4,421 findings, 0 parse failures; coverage **partial** (parent result budget exceeded for many shared chunks; 114 limitations). Evidence: summary `ev_3ebdada1085be3ff6dcbe1261426eeea1bcc897be3cdd8146998956e134486db`, parent `ev_2f7291ebab510bf1e0fccc885d0cfd613cbf0789ce858d3bf8ce420500ff67dc`.
- `inspect_analysis_view` (module item for `assets/v1/shared-4-KMRq4mDb.js`): evidence `ev_2f880766817b1b8d5e8bf4560b29321e0eeee0c7511c7477f839ee5a9b66b698`. Static facts REA retained for that file: 43 imports, vendor `Vite`, 66,022 AST nodes, localStorage key `claudeai.codeTranscriptRendererWanted.v1`, uses of `indexedDB.open`.
- `trace_application_feature` (string seed `/claude-code-desktop`, contains): no graph match (`ev_c38bef1245511c3570068d865e2e0b190bd631452370cc7af041a9ecb823de4b`). As the scout also found, literal seeds do not reach minified route and event strings; SRC greps were used instead.
- Not usable: the scout's evidence ids are not retained in this session (`evidence_integrity_mismatch`), so no scout REA result was reused.
- Not used on purpose: `open_binary`/`binary_session` (shared active target), PE and Mach-O tools on `cowork-svc.exe` and the vhdx (out of facet and no execution allowed).

### 1.3 Corrections and additions to the scout inventory

| Scout statement | Finding |
|---|---|
| "Message rendering is remote: no local markdown, KaTeX, Mermaid..." (category 1) | True for the asar, false for the install: `resources/ion-dist` contains the complete web client (markdown, highlight worker, KaTeX hooks, Mermaid 11.17.2 in the frame runtime, ProseMirror/TipTap composer, code editors). [SRC] |
| `hostLoop` purpose "INF, low to medium" | The `requireFullVmSandbox` setting text says the default host-native mode runs shell commands in the VM while file tools run on the host with path-based access control; enabling the setting forces the agent loop itself into the VM. [SRC] |
| `HV` unresolved (nine flags) | `HV = {status:"supported"}`; those nine features are always supported in this build. [SRC] |
| `epitaxy` unresolved | `epitaxy` is a newer shell/route family: `sidebarMode` enum `chat|code|task|epitaxy`, SPA routes `/epitaxy/*`, "epitaxy chrome" layout, 528 analytics events. It renders Code/Cowork sessions and shared workspaces ("grouping" events). Meaning is inferred (medium). [SRC + INF] |
| Feature table "122 keys" | The support-status schema `Ait` has 123 keys (supported, unsupported with reason codes, unavailable). Preferences have 115 keys (72 tagged `security`). [SRC] |
| `claude-nest` | An app variant enum: `claude | claude-nest | claude-dev | claude-nest-dev`. Nothing more known. [SRC; low] |
| `imagine-server` unconfirmed | Session option `imagineSystemPrompt` plus the built-in server name `visualize` suggest it implements the inline widget tool. [INF, medium] |
| Not in scout | `claude-browser-shim.js` ($BROWSER shim), the installed locale set, `cowork-svc.exe`, `smol-bin.x64.vhdx`, `claude-ssh-*.exe`, `chrome-native-host.exe`, the entire `ion-dist`. [SRC] |

---

## 2. The product surface map

### 2.1 Surfaces (what a user can reach)

| Surface | Where it lives | Notes |
|---|---|---|
| Chat | claude.ai SPA (remote in 1P; local `ion-dist` in custom 3P) | projects, artifacts, memory, voice, research, connectors |
| Cowork | same SPA + local VM/host loop | long-running tasks with folders, browser, computer use, routines, spaces; public docs now say "Cowork and chat are one Claude" [DOC] |
| Code | same SPA (`/code`, `/claude-code-desktop`, `/epitaxy`) + local Claude Code engine | sessions, worktrees, diff/PR/terminal/preview panes, remote control, cloud handoff |
| Claude Design / Slides | canvas artifacts + design systems | codename omelette; DOC says folded into conversations and separate Slides tool [DOC] |
| Claude Security | scheduled repository scans in cloud sessions | server-side surface, found via SPA strings |
| Claude in Slack ("Claude Tag") | server-side agent | layered access, channel instructions, memory silos, spend limits |
| Claude in Chrome | extension + native host | side panel, browser actions, saved shortcuts |
| Office add-ins | Word/Excel/PowerPoint/Outlook | sensitivity-label policies; skill bundles |
| Quick Entry / Quick Write | native overlay and local window | global hotkey capture; hold-to-talk helper (low confidence) |
| Hardware Buddy | BLE maker devices | developer-mode protocol |

The install hub in the SPA lists: desktop, iOS, Android, Chrome, Claude Code (terminal and desktop), VS Code, JetBrains, Claude Design, Cowork, Slack, Teams, Excel, Word, PowerPoint, Outlook (file names under `images/install-hub`). [SRC]

### 2.2 Routes and deep links

The shell's `claude://` handler maps these route words (enum `Ul`): `magic-link, new, sso-callback, mcp-auth-callback, chat, project, settings, admin-settings, customize, directory, create, tasks, task, space, claude-code-desktop, code, epitaxy, design, resume, cowork, local_sessions`. Conversation and project links fall back to `/recents` and `/projects`. SPA classifiers further reference `/chats, /artifacts, /skills, /library, /scheduled-task/:id, /cowork/project/:id, /cowork-artifact/*, /security, /customize/plugins/new?marketplace=&plugin=`. The handler normalises paths, rejects out-of-route redirects and malformed session ids. [SRC: MAIN@833040, @3235915; ION shared-4@21363]

### 2.3 Chrome layouts

`data-theme="claude" data-mode="system" data-density="comfortable" data-color-version="v2"` on the root; "epitaxy chrome" vs frame-owned header per route; panes that scroll in-pane; flush content gutter on chat detail routes. [SRC: ion-dist/index.html; shared-4]

---

## 3. Architecture of the product layer

```
                         claude.ai (1P) / ion-dist over app://localhost (custom 3P)
 Electron main ──────────┐          React SPA ("ion") + TanStack Query + react-router
  - eipc typed IPC (89)  │          ProseMirror/TipTap composer, GrowthBook gates, Datadog RUM
  - prefs (115 keys)     │          lazy module registry T1/T2/T3, tool cells, command palette
  - settings registry    ├────────▶ artifact frames  (frame-shell.html + _frame-rt capability modules)
  - built-in MCP servers │                          Rust/WASM: yrs+pulldown-cmark docs, c2pa+pdf/img crates
  - workers: pty, search,│
    file-index, git, msal│          local agent engine = Claude Code via Agent SDK (child process)
  - NAPI-RS native addon ┘          session start contract (about 48 typed fields) from SPA to engine
```

### 3.1 How the SPA reaches local capability

- Preload `mainView.js` exposes `claude.web.*` services over typed IPC channels `$eipc_message$_<uuid>_$_<ns>_$_<Service>_$_<method>`; sender validation checks `senderFrame.parent === null` and an origin allow-list (claude.ai, preview.claude.ai, claude.com, preview.claude.com, `app://localhost`). Largest services: LocalSessions 321 methods, LocalAgentModeSessions 115, Launch 86, Simulator 49, Custom3pSetup 35, CoworkSpaces 33. [SRC: scout appendix C, verified]
- A separate typed channel `EngineSurface` (19 methods: uiRender, uiPress, uiInput, uiSelect, uiPanes...) lets the engine drive UI panes; only inspected at signature level (low confidence). [SRC]
- Preferences: 115 keys with defaults and a zod schema tagged `security | other | internal_state | ui_state`; a subset is model-settable with `{label, meaning, toolKey, locked reason}`. [SRC: MAIN@1434658, @1518540]
- Settings registry (enterprise): declarative entries with flat key, scopes 1p/3p, available-in version, visibility, category/group/order, show/disabled predicates, fail-closed value, remote-policy flags, `surfaceGrants`. Example: `desktopHome` = standard | simple | no-vm | off with grants for chat, file analysis and Cowork. [SRC: MAIN@681329]
- Feature support schema: each feature is `supported | unsupported(reason code) | unavailable`; reason codes include `disabled_by_enterprise`, `disabled_by_org_policy`, `unsupported_platform`, `virtualization_not_available`, `msix_required`. This lets the UI explain every disabled control. [SRC]

### 3.2 The app-as-tool server table

Built-in MCP servers (fixed ids): claude-in-chrome, office, computer-use, workspace, mcp-registry, plugins, skills, cowork-onboarding, dev-debug, Claude Preview, Claude Browser, Framebuffer, visualize, Window Halo, ccd_session, ccd_directory, ccd_session_mgmt, ccd_connectors, ccd_host, ccd_pr, ccd_sidebar, ccd_turn, ccd_window, ccd_view, ccd_settings, terminal, Claude Code iOS Simulator, Claude Code Android Emulator, remote-devices, scheduled-tasks. [SRC: MAIN PKe table]

Tool contracts worth studying (descriptions are shipped as app text):
- `ccd_sidebar`: list/create/rename/delete groups and sections, `move_sessions` (max 100), `set_pinned`, `set_unread`, `mark_completed`, `set_view` (`group_by` date|project|state|custom|none; `status` active|archived|all; `sort_by` recency|alpha|created; `last_activity` 0|1|3|7|30 days). Built-in sections: Pinned, Routines, Sessions. Renames/deletes ask the user in default mode, a classifier decides in auto mode. [SRC: TOOLS@275775]
- `ccd_view`: `show_pane`/`close_pane` for diff, file (at a line), terminal (tab), pr, tasks, plan, artifact; "prefer showing over describing". Transcript view normal|thinking|verbose. [SRC: TOOLS@299377]
- `ccd_turn`: `update_plan` (first call in parallel with the first real tool call; one in_progress step; `scope` first_task|new_task|continuation; a new task opens a start_session offer) and `wrap_up` (recap box, optional question and buttons restricted to app-listed actions). [SRC: TOOLS@14663, @16340]
- `ccd_session_mgmt`: list/get/search (verbatim snippets labelled untrusted), send, stop, title, model, effort, permission mode, clear, delete (max 25 with one approval card in every mode), export (zip, six per hour), unarchive. [SRC]
- `ccd_session`: `mark_chapter`, `spawn_task`/`dismiss_task`, `start_session`, `hand_off_to_session`, `read_widget_context`, `move_to_cloud`. [SRC + session schema]
- `ccd_settings`: read and set a small allow-list of preferences, with locked reasons. [SRC: TOOLS@270011]
- `ccd_host`: open in editor, reveal path, storage usage report, kept-worktree cleanup, keep-awake request with a mandatory reason. [SRC]

### 3.3 Session start contract

Validator for `LocalAgentModeSessions.start` options lists: `policies, branchCutMessageUuid, sessionId, model, permissionMode, extendedThinkingEnabled, title, systemPrompt, systemPromptRendererAppends, mcpServers, remoteMcpServers, withheldConnectorHosts, images, trailingContentBlocks, userSelectedFolders, userSelectedFiles, egressAllowedDomains, orgCliExecPolicies, enabledCliOps, accountName, emailAddress, userSelectedProjectUuids, scheduledTaskId, scheduledTaskManualStart, spaceId, otelConfig, orgOtlpContentCapture, channel, sessionType, parentSessionId, memoryEnabled, skillsEnabled, syntheticMessage, seededSummon, seededKind, pluginsEnabled, documentFunnelEnabled, docxEditingCarveout, frameArtifactsEnabled, artifactHostGrant, imagineSystemPrompt, memoryGuidelinesTemplate, coworkSyspromptMap, spSectionPrompts, chromeSkipAllPermissionChecks, cuAppHints, branchSession`. [SRC: MAIN@1724616]

Design reading (INF): product behaviour (memory guidelines, widgets, artifacts, plugins, document pipeline) is toggled and prompted per session by the server-side web app; the local engine is generic. The same split fits Verevon's gateway/Model Plane boundary.

### 3.4 Artifact frame runtime

`frame-shell.html` (title "Artifact", `referrer: no-referrer`, `frame-runtime-pin` meta) loads a pinned runtime manifest (`_frame-rt/_runtime/manifest.<hash>.json`). Capability modules: artifact, assets, comments, db, dictation, downloads, embed, endpoints, files, mcp, network, permissions, reads, room, sample, user; plus `ldx` (form/data engine in cold/ctx/data/edit/swap chunks), kernel, transforms, translate, worker; Mermaid 11.17.2 bundled. Modules talk to the host shell over `postMessage` with a broker generation counter (`brokerGen`) and "capability rewiring" so a revoked capability tears the call down; text sent to the Claude session has size caps and character filters. [SRC]

### 3.5 Local-server stub for third-party mode

In custom-3P mode the shell serves `ion-dist` through `protocol.handle("app")` on `app://localhost` (origin check 403 on mismatch), answers `/api/bootstrap/*/app_start`, `system_prompts`, `current_user_access`, `/plugins/list-plugins`, `/marketplaces/*`, `/dust/command_display_names`, `/dust/generate_session_title`, `/dust/generate_title_and_branch`, `/i18n/*`, `/v1/code/github/` itself, forwards an allow-list to the vendor host, and returns typed 503s (`custom_3p_stub_failed`, `custom_3p_forward_failed`) for un-stubbed API paths. [SRC: MAIN@4275325]. In first-party mode the main window loads `https://claude.ai` (function `Eg`), and `getMainWindowUrl()` selects the origin (selector not read). [SRC; medium]

---

## 4. Data model sketches (as far as the client reveals them)

- **Session**: local record (bridge ids, folder, branch/worktree, permission mode, model/effort, title with source, archived, kept-dirty worktree) with a **lineage**: kinds sideSession, handOff, suggestedTask, fork; `lineageDetached`; archived rollups up to 32 hops; popout/layout restore. Remote (cloud) sessions mirror through the bridge and `/v1/code/sessions(+watch)`. [SRC: TOOLS@293481-296759]
- **Space** (Cowork): id, name, description, origin auto|manual, instructions, folders, projects, links, memory index; remote-session-to-space map; migration from Projects with memory copy. [SRC]
- **Artifact**: uuid, name, starred, versions, shared version pin, access list (owner/editor/commenter/viewer, groups, outside invitees), auto-publish, MCP tool allow-list, files/db/room storage, comments, retention TTL. [SRC + DOC]
- **Memory**: account memory (view/edit/pause/reset), project memory (nightly summary), space/channel memory files, global and per-account Cowork memory files, instruction layers. [SRC]
- **Routine / scheduled task**: schedule (cron or once), GitHub-event trigger with filters, API trigger token, host-runs support flag, watcher history, dispatch claims, on-hold states, run history. [SRC]
- **Environment**: vendor-hosted or self-hosted, setup script, env vars (local ones never sent), tokens, network policy (none; trusted = package managers and MCP servers; custom allow-list; all domains) overridable by org policy. [SRC]
- **Notification preferences**: `{permission, idle, question}` each `off | badge | banner`, sound `system | none`, dock bounce. [SRC]

---

## 5. Notable design decisions

1. **Everything is a session**, and sessions are addressable, forkable, archivable, searchable and orchestratable by other sessions. Cross-session tools label foreign transcript text as untrusted.
2. **Queue, do not block.** The composer queues messages behind a running turn with reorder, send-now and requeue; editing earlier messages is blocked while a queued message exists.
3. **Turn lifecycle contract in tools, not prose.** Plan, progress, scope classification and wrap-up are tool calls the UI renders.
4. **Consent is scoped and named.** Dialogs name hosts, folders, apps, and data flows ("sent to Anthropic"), and add per-host memory ("always allow reads on host") with extra warnings for network-local names.
5. **Fail closed in policy.** Registry entries carry a `failClosedValue`; unavailable remote policy means the safe value.
6. **Declarative everything.** Settings registry, feature support schema, preference schema with model-facing meanings, MDM export from the same registry.
7. **Cheap model for chrome tasks.** Titles and branch names are generated by a small model through dedicated endpoints.
8. **Lazy by tier with failure policy.** `onUnrecoverable: "reload"` for large route closures; fixed-height skeletons per tool cell.
9. **Resilience events are first-class telemetry.** Draft restore, stream loss, recovery, TTFT/TTLT, dead ends, pending-turn stuck idle, worker degraded.
10. **Local-first where it protects privacy**: Quick Entry resizes and caps attachments locally; Chrome login import is explicit and unavailable on Windows for Chrome/Edge; browser storage defaults to none.
11. **Product family hand-offs** ("try this in Claude Science", recommend-apps cell) instead of dead ends.
12. **Unified naming of states** across sidebar, notifications and APIs.

---

## 6. Best in class (what to copy), by taxonomy category

### Category 1: Chat experience
- Message queue and steering (5), rewind with file checkpoints and undo (5), fork lineage (5), side chat (4), pinned plan + wrap-up card (5), tool cells (5), artifacts platform (5), artifact comments with agent summoning (5), project knowledge auto-switch to retrieval (4), windowed transcripts (4), selection actions (4).
- Weak spots: edit/regenerate are table stakes; no model comparison (Claude is single-vendor).

### Category 2: Multimodal & creation
- Voice mode with sound cues and voice samples; dictation as a capability available to composer, overlay and generated apps (one streaming contract, partial/final, 15 min cap); Read Aloud; Claude Design with design systems and deck export; annotate-and-point tools for previews; Rust/WASM document and provenance engines.
- Not found: image generation in the app (no tool cell, no endpoint). Video: none. [SRC negative; scout agrees]

### Category 3: Research & web
- Research mode shows a plan first; per-domain fetch consent; connector registry suggestions with opt-in and resumable connect intents; tool-level policy validated against MCP annotations; built-in browser site policies; Drive open/export.
- Only light local signal for research (9 strings); the loop is server-side.

### Category 8: Memory, learning & personalization
- Memory is visible and editable (pause, org toggle, per-project nightly summaries, file-like views, import from other AI by copy-prompt/paste), incognito semantics (not in history, memory, search or training; org can still see), layered instructions (global, folder, space, project, channel, profile, org), skill authoring with Claude plus try-in-chat, watch-record/teach demonstrations, model-settable preferences.
- Learning is user-supervised: proposals and suggestions as tool cells; no autonomous self-improvement loop observed.

### Category 12: UI/UX & product design
- Command palette (T1) with history switcher, full keyboard model, split view and popouts with restorable layout, side panes steered by the agent, sidebar sections and groups with auto-archive, Quick Entry, notification model by event type, offline game page, onboarding with role chips writing into instructions, install hub, tiered lazy modules, ICU i18n with locale overrides, accessibility affordances (reduced motion, drag-to-rank spoken labels, OpenDyslexic).

### Other categories touched
- Agent runtime: routines with three trigger kinds, continue on cloud, remote control, background task backlog, context ring.
- Safety: auto mode, permission scopes, pinned git origin, security scan gate, per-product ZDR.
- Platform: bundled SPA + stub API seam, session start contract, settings registry, EngineSurface (low).

---

## 7. Weaknesses, limitations and risks

1. **Remote-first UI**: in first-party mode the chat renderer, research, memory UI and prompts are server-side; the local app cannot be re-created from the shell alone. Offline use is a game page.
2. **Codename opacity and churn**: chicago, sableOtter, epitaxy, grouping, lantern, copper heron, violinBow, lanyard, yukonSilver... Many gates are `unavailable`; product naming is moving (Cowork becomes "one Claude", Design folds into chat, Slides splits out). DOC pages disagree with each other in places (artifact sharing rules, memory legacy dates).
3. **Surface sprawl**: about 45 settings families, 115 preferences, 123 features, 3 deployment modes (1P, 3P, self-hosted), SSH/WSL/cloud environments. High support cost; the app ships extensive diagnostics to compensate.
4. **Windows depth**: Cowork needs Virtual Machine Platform and a 37.7 MB VHDX plus a Windows service; computer use is a research preview; Chrome login import is Firefox-only on Windows.
5. **Agents controlling the app** enlarges the injection surface (sidebar, settings, delete, export). Mitigations seen: unattended-session gating, mode-aware approvals, one approval card for deletes, untrusted-snippet labelling. Residual risk is unmeasured.
6. **Telemetry volume**: 2,885 named events plus RUM; admins can block nonessential telemetry, but the default surface is large.
7. **No image or video generation, no model comparison, no multi-vendor routing** in the product (by design; Verevon can differentiate).
8. **Prompt text is server-owned**: Verevon cannot learn Claude's system prompts from the client; only field names and toggles are visible.
9. **Evidence limits**: no runtime observation; REA coverage partial; wasm crate lists come from embedded panic paths; Quick Write, coordinator mode and EngineSurface are low confidence.

---

## 8. Mapping to Verevon: copy, adapt, avoid

Verevon state is from `v-chat-agents-ui.baseline.md` (SolidJS 2 RC SPA, Rust Axum BFF, Model Plane owns sessions/runs; plane rules: no cross-plane DB access, model proposes and Quarry executes browser actions, ZDR propagates, Space access never widens an owner-plane ACL, Channel Plane docs-only).

| Verevon gap (baseline) | Claude pattern | Recommendation |
|---|---|---|
| MCP tools cannot run from chat | Per-tool policy (always/ask/session/blocked), read/write annotation validation, per-session vs persistent approvals, withheld connector hosts, tool timeout | **Copy** the policy model; bind MCP tools to capability-core ids using annotations (readOnlyHint) so read-only tools may run inline after policy and writes need durable approval. Keep "model proposes, platform executes". |
| Persona/org/Space instructions do not reach Do runs | Session start contract carries prompt sections and instructions for every session type | **Copy**: one typed run-start contract in Model Plane; the BFF sends structured intent only. |
| No checkpoint/rewind | Rewind v2 pairs transcript position with file checkpoints, undo, failure messages | **Adapt**: implement in session-core with sandbox snapshots; mark provider writes non-rewindable (matches Verevon's effect immutability). |
| Branches/versions client-side | Server-owned fork lineage (`branchCutMessageUuid`, sideSession/handOff/fork kinds) | **Copy** the lineage model into session-core. |
| No share link / import | Snapshot sharing with disclaimers, org/public toggles, unshare in Privacy; import from CLI, other tools, vendor export | **Adapt** with ZDR-aware refusals; build share as a Data Plane object, not BFF state. |
| Projects are browser-only | Projects with instructions, knowledge with auto retrieval, per-project memory; Spaces with folders/links/memory; shared members, invite links | **Copy** server-owned Spaces/Projects in Application Plane with owner-plane ACLs. |
| Artifacts read-only | Versioned, editable, commentable artifacts; frame runtime; collaborative Markdown via Rust yrs | **Adapt**: start with versions + restore + comments; adopt the capability-gated frame runtime pattern; reuse yrs/pulldown-cmark in Rust and WASM. |
| Voice push-to-talk only | Voice mode + sfx, dictation streaming over WebSocket with partial/final | **Adapt**: add streaming dictation with partials and a state machine; sound cues; keep ZDR refusal. |
| Computer use not wired | Background vs full control, per-app grants, deny list, approvals naming sites, $BROWSER shim to route links into the pane | **Adapt the UX only**: keep Quarry as the executor; reuse consent copy and the pane-routing idea. Avoid an extension/CDP bridge (ruled out). |
| Wide research has no entry point | Research mode plan-first | **Copy**: plan-first card with explicit toggle. |
| Scheduled runs weak | Routines: cron/event/API triggers, local vs cloud, run limits, hold states, run history | **Copy** trigger model; schedule full agent runs, not single calls. |
| Temporary chat unavailable (no ZDR attestation) | Incognito = no history, memory, search; org still sees; separate from training policy | **Consider** decoupling client-visible incognito (no Verevon storage or memory) from provider ZDR attestation, with honest disclosure; keep strict ZDR as an additional tier. |
| Plugins have no install/marketplace | Marketplaces with pinned sources, org distribution, security scan gate, Customize hub | **Adapt**: pinned sources and scan-before-live first; defer marketplace UI. |
| Agent pages are blueprints | Session states vocabulary, sidebar sections, pinned plan, wrap-up card, side panes, background-task backlog, overview of outputs | **Copy**: states, plan contract and side panes first; they make the Run Console legible. |
| Streaming perf (no virtualization, full reparse) | Tail-first windowed transcripts, eviction, tiered lazy modules, tool cells with skeletons | **Copy**: virtualise, parse incrementally, tier lazy modules (Solid lazy + Suspense), add a chunk-cycle CI check. |
| No live spend meter / shortcut sheet / templates | Usage popover with roster, shortcuts dialog (Ctrl+/), prompt-argument modal, skills as slash commands | **Copy**. |
| i18n leaks | ICU messages with hashed ids, override catalogs, language picker | **Adapt** the extraction pipeline (no/en first; nb exists in Claude's desktop catalog). |
| Thread list capped at 80 | Windowed sidebar, "view all", transcript search worker | **Copy** paged list and server search in session-core. |
| Channel Plane docs-only | Claude Tag: layered access (default/workspace/channel), channel instructions, memory silos, spend limits, identity profiles, pause | **Use as the reference spec** for the Channel Plane docs. |

### 8.1 Where Verevon is already ahead (keep)
Conversation-node model, served-by line, privacy-tier floor, router policy with admin table, effect-class immutability with proofs, context inspector, answer confidence and verification, sources panel split (read vs not read), multi-provider catalogue. Claude exposes none of these; they are the multi-model, EU/ZDR-first differentiators.

### 8.2 Avoid
- Codename-driven flags in user-visible config; keep names descriptive.
- Giving the model unrestricted control of destructive app tools without a hard-confirm class (Claude keeps one approval card in every mode).
- Three parallel deployment modes unless required.
- Shipping prompt logic only on the server without versioned contracts (Claude's per-session toggles are workable, but opaque to customers; Verevon's transparency goal argues for inspectable prompt assembly).

---

## 9. Rust re-creation notes and improvements

- **Share the engines**: `yrs` + `pulldown-cmark` (documents) and `c2pa` (provenance stamping) are already used by Claude as WASM; Verevon's Rust services can compile the same crates for the browser (wasm-bindgen) and the Application Plane.
- **Typed contract generation**: Claude uses a codegen for IPC (`@ant/ipc-codegen`). For Verevon, generate TypeScript types and zod schemas from Rust (ts-rs/specta) for gateway routes and the run-start contract; version the contract.
- **Native addon pattern**: NAPI-RS in the desktop; if Verevon ships a desktop wrapper, Tauri with Rust commands removes Electron weight (not required for the web chat app).
- **Declarative registry in Rust**: one `serde` schema with scopes, version gates, fail-closed values and surface grants, rendered to UI, docs and MDM exports.
- **SolidJS**: model the tool-cell registry as `lazy()` components keyed by tool name with fixed-height fallbacks; keep a tier annotation and a failure policy; add a chunk-cycle report like `scc.json`.
- **Improve on Claude**: expose prompt assembly and routing to users, add model comparison and failover, make run proofs first-class, offer an open event protocol (AG-UI/MCP) rather than proprietary IPC, and publish the state vocabulary as API.

---

## 10. Open questions

1. Which surfaces does the 1P desktop load from the remote site versus `ion-dist`? (`getMainWindowUrl` selector not read.)
2. What does `EngineSurface` render, and which engine plugins use it?
3. What exactly is stamped by the c2pa module (exports, downloads, images)? No user-facing strings explain it.
4. Which models and engines run title/branch generation in first-party mode?
5. Realtime transport for voice mode (WebRTC strings exist in two chunks; endpoints remote).
6. Meaning of "grouping", "lantern", "nuthatch", "chicago", "yukonSilver" beyond inference.
7. How tier T1/T2/T3 are enforced at build time (the registry is visible, the build step is not).
8. Whether the bundled SPA at `ion-dist` is feature-identical to the hosted app (event and string counts suggest near parity).

---

## 11. Evidence pointers

- Main bundle: `extract/claude/.vite/build/index.chunk-EzXl5k4A.js` (offsets in the JSON are character offsets into the single minified line). Tool definitions: `index.chunk-1xGiXs9t.js`. Renderers: `extract/claude/.vite/renderer/*`. Preloads: `.vite/build/{mainView,coworkArtifact,designWindow,computerUse*,watchRecordChooser,localExecConsent,buddy}.js`.
- Installed program: `C:/Program Files/WindowsApps/Claude_2.31226.1.0_x64__pzs8sxrjxfjjc/app/resources/{ion-dist,claude-browser-shim.js,cowork-svc.exe,smol-bin.x64.vhdx,claude-ssh-*.exe,chrome-native-host.exe,*.json locales}`.
- i18n evidence format: `ion-dist/i18n/en-US.json#<message id>`. Event evidence: analytics event names listed in `tmp/claude-ux/events.txt`. Lazy-module evidence: id plus chunk file.
- REA ids: see section 1.2.
- DOC: support.claude.com (Cowork and chat are one Claude; schedule recurring tasks; import and export memory; incognito chats; publish and share artifacts), anthropic.com/news/claude-design-anthropic-labs, claude.com/product/design, code.claude.com/docs/en/desktop (via the scout).
