# DeepSeek Harness (dsh): Audit 3 inventory (T3 scout, DeepSeek source)

Scope: `C:/dev/deepseek-harness` (git `master`, HEAD `d743267`, 2026-10-09, merge of `release-0.2.1-alpha.2`, 21,405 commits). Read-only: nothing was installed, built, tested, launched, or written inside the repo. `node_modules` is absent and no root `.env` exists, so no dependency code was executed.

Pass 2 (2026-10-13): this file was re-checked against the checkout. Counts were re-run, the repo paths it cites were checked for existence (three were wrong and are fixed inline), and the claims of Verevon's prior analysis were compared in Section 7. Pass 2 corrections are inline; new material is in Sections 3.1, 3.4, 3.5, 3.10, 3.13 and 7, and in Section 6 items 10 and 11.

Provenance tags used throughout:
- **SRC**: read directly in source files (path given).
- **DOC**: repo documentation (README, docs/, package READMEs, generated catalogs such as `docs/tool-catalog.md`, `docs/config-catalog.md`, `docs/cli-help.md`). Generated docs mirror source and carry a "do not edit" header. They were treated as DOC and not re-verified.
- **INF**: my inference, with the basis stated.
- **REA**: not used. This repo is source code, and the scratch `extract/` folder holds no DeepSeek build (its folders are antigravity, claude, codex, grokbot, manus, slack, t3code).
- Negative findings ("not found") come from keyword sweeps over `packages/`, `apps/` and `python/`, excluding tests. They are listed in Appendix D.

Paths are relative to `C:/dev/deepseek-harness/` unless absolute.

---

## 1. Identity

**Product.** DeepSeek Harness, short name DSH, CLI `dsh`, npm `@deepseek-ai/dsh` (`apps/cli/package.json`, `bin.dsh = lib/bin.js`). Root package `@deepseek-ai/dsh-root` (`package.json`). Self-description: "an open-source agent harness developed by DeepSeek AI", built on an "everything-is-a-plugin" architecture powered by Cordis (`README.md`, DOC). Status: "developer preview", with compatibility-breaking changes expected (`README.md`, DOC). `SAFETY.md` says it is experimental, has not had a security audit, and is not production-ready (DOC). License MIT (`LICENSE`).

**Version.** `0.2.1-alpha.2` in root, `apps/cli`, `apps/web`, `apps/desktop`, `apps/desktop-host` (SRC). Latest git tag `dsh-v0.2.1-alpha.2`, then `dsh-v0.2.1-alpha.1`, `dsh-v0.2.0-rc.2`, `dsh-v0.2.0-rc.1`, `dsh-v0.1.7-rc.2`. The Python manifests carry the placeholder `0.0.0.dev0` (`python/sdk/pyproject.toml`, `python/sdk-runtime/pyproject.toml`, SRC). INF (medium confidence): the release scripts in `scripts/release/` stamp real versions at publish time.

**Runtime and languages.**
- Node `^22.19.0 || >=24.0.0` (root `package.json` `engines`, SRC). A local `node` v24.19.0 exists on this machine. It was not used to run project code.
- TypeScript, strict and ESM everywhere (`AGENTS.md` "Type safety", SRC). Root devDependency `typescript ^6.0.3`.
- Python SDK and runtime under `python/sdk` and `python/sdk-runtime` (`uv.lock`, `hatch_build.py`, SRC).
- C test oracle for the POSIX flock contract (`native/system/test/fixtures/flock-oracle.c`, SRC). Node-API native packages under `native/system/packages/`.
- No Rust or Go source in the repo (`find` for `Cargo.toml` and `go.mod` returned nothing, SRC). This matters for the "recreate in Rust" goal.
- Package manager pnpm `11.28.5` (`packageManager`, SRC). Build and dev scripts run through `tsx`.

**Core framework.** Cordis, vendored under `vendor/cordis`, plus `cosmokit`, `group`, `hmr`, `include`, `loader`, `logger-console`, `schemastery` and `timer`. Plugins contribute services, typed events and reversible effects, and nothing in the core is privileged (`docs/architecture.md` "Cordis", DOC).

**Dependency fingerprint, grouped by capability.** Declaring manifests are in parentheses. The full table is in Appendix A.
- *Agent and model SDKs:* `@earendil-works/pi-ai ^1.0.2` (`packages/llm/llm-pi-ai`), `@anthropic-ai/claude-agent-sdk 0.3.263` and `@anthropic-ai/sdk` (`packages/subagent/subagent-claude-code`), `@openai/codex 0.153.4` (`packages/subagent/subagent-codex`), `@agentclientprotocol/sdk 1.4.0` (`packages/acp/acp`, `packages/subagent/subagent-acp`, `apps/cli`), `@modelcontextprotocol/sdk ^1.31.0` (`packages/subagent/subagent-claude-code`; the MCP client in `packages/mcp/mcp-client` declares example MCP servers), `eventsource-parser` (`packages/llm/llm-deepseek`).
- *Browser and computer control:* `playwright` and `@playwright/mcp` (`packages/experimental/browser-use-playwright-mcp`; `apps/web` tests), `chrome-devtools-mcp` (`packages/experimental/browser-use-chrome-devtools-mcp`), `@browserbasehq/stagehand 4.1.0` and `@puppeteer/browsers` (`packages/experimental/browser-use-stagehand-native`), `@trycua/cua-driver 0.28.0` (`packages/experimental/computer-use-cua-driver-native`).
- *Speech:* `sherpa-onnx-node 1.13.8` (`packages/experimental/speech-to-text-sensevoice`).
- *Processes and terminals:* `node-pty 1.2.0-beta.15` (`packages/subprocess/subprocess-local`), `@xterm/headless` (`packages/api/terminal-controller`, `packages/terminal/terminal-bash`), `koffi 3.1.1` (Win32 FFI; source import in `packages/subprocess/win32-process/src/koffi.ts`).
- *Search and code navigation:* `@vscode/ripgrep` (`packages/fs/tool-fs-search`), `typescript-language-server` (`packages/lsp/lsp-stdio`), `chokidar` (file watching in skill, fs and boot packages).
- *Web access:* `undici` (`packages/web/web-fetch-http`, `packages/util/http-proxy`), `turndown` and `@joplin/turndown-plugin-gfm` (`packages/web/tool-web`).
- *Documents and media:* `sharp` (`packages/attachment/attachment-local`, `packages/spill/spill-policy`), `pdfjs-dist`, `xlsx`, `exceljs`, `@fortune-sheet/core` and `@fortune-sheet/react` (`packages/client/ui-sidebar-documentpreview`), `fast-xml-parser`, `papaparse`, `dompurify`, `extract-zip`, `tar`, `fflate`. Office conversion uses LibreOffice (native engine) or a Node WASM engine (`packages/document/office-to-pdf`, DOC).
- *Web UI:* `react ^18.2.0` and `react-dom`, `zustand ~4.5.7` and `immer` (`packages/client/store`), `lexical ^0.49.0` (`packages/client/ui-conversation`), `@xterm/xterm ^6.0.0` (`packages/client/ui-sidebar-terminal`), `@tanstack/react-virtual` (`packages/client/ui-chat`, `ui-trajectory`), `katex` with `rehype-katex` and `shiki` (`packages/client/ui-primitives`), `anser` (ANSI to HTML), `simple-icons`, `clsx`, `dayjs`, `big.js`.
- *Schemas and config:* `zod ^4.4.3` (typed Remote contracts and config; 67 manifests), `ajv` (`apps/cli`, `packages/boot/app-boot`), `yaml`, `js-yaml`, `smol-toml`, `semver`, `@standard-schema/spec` (vendored Cordis and schemastery).
- *HTTP server and TLS:* `ws` (`packages/api/gateway`, `apps/web`, `apps/cli`), `compression`, `serve-static`, `negotiator`, `mime-types`, `selfsigned` (`packages/host/webserver`), `ipaddr.js`.
- *Telemetry:* `@opentelemetry/api`, `api-logs`, `sdk-logs`, `otlp-exporter-base`, `otlp-transformer`, `resources`, `core` (`packages/telemetry/otel`, `packages/host/product-telemetry-otel`, `packages/session/session-telemetry-otel`).
- *Scheduling:* `@js-temporal/polyfill` (`packages/schedule/schedule`).
- *GitHub ingress:* `@octokit/webhooks` (`packages/experimental/webhook-github`).
- *Desktop shell:* `electron ^44.7.0`, `electron-builder ^26.17.0`, `electron-updater`, `app-builder-lib`, `@electron/notarize`, `@electron/get` (`apps/desktop/package.json`).
- *Packaging and tooling:* `@yao-pkg/pkg 6.21.0` (root), `@yarnpkg/cli-dist 4.18.0` (root devDependency; runtime role not located, low confidence), `cos-nodejs-sdk-v5` (`apps/desktop/scripts/desktop-cos.ts`; release-upload tooling, not runtime, SRC).
- *Quality and build:* `vitest ^4.1.11` with `@vitest/coverage-v8`, `@testing-library/react` and `dom`, `jsdom 29.1.1`, `fast-check ^4.9.0` (property tests), `oxlint 1.81.0` with `oxlint-tsgolint`, `publint`, `jscpd` (duplication), `lefthook`, `eslint-plugin-sonarjs`, `@stylistic/eslint-plugin`, `tsdown ^0.22.2`, `tsx ^4.23.13`, `esbuild`, `lightningcss`, `vite` (root devDependency 8.2.2; at least one workspace pins `^6.4.3`).
- *Docs site:* `vitepress`, `vue`, `mermaid 11.17.2`, `cytoscape`, `@panzoom/panzoom` (`website/`, root). Mermaid is not a dependency of the Web chat client (SRC grep).

**Scale.** 340 workspace `package.json` files under `packages/`, 369 manifests in total, 4,779 dependency edges, 519 unique dependency names (SRC, `jq` aggregation). Root `package.json` has 195 scripts, including 63 `verify-*` and 18 `gen-*` entries (SRC). Approximate file counts: `packages/` 7,076; `.agents/` 3,717; `snapshots/` 1,562 (600 recorded session logs); `apps/` 1,164; `docs/` 692; `scripts/` 388; `vendor/` 76; `native/` 79; `.github/` 56; `python/` 47; `benchmarks/` 45; `website/` 32; `patches/` 7 (SRC counts).

---

## 2. Directory and module map

Top level (SRC, `ls`):
- `apps/`: launchers and shells.
  - `apps/cli`: the `dsh` launcher. Subcommands `web`, `headless`, `acp`, `sdk`, `sdk-minimal`, `plugin`; profile and `--patch` handling; `--dump-config`; startup diagnostics (`src/bin.ts`, `profile-boot.ts`, `plugin.ts`, `startup-diagnostics.ts`, `dump-config.ts`).
  - `apps/web`: Vite SPA entry (`src/main.ts`, `index.html`). Its `dist/` is served by `dsh web`. It also holds Playwright e2e and stress tests.
  - `apps/desktop`: Electron shell (main process, tray, menus, updates, browser guests, command installer, welcome window; packaging under `installer/` and `scripts/`).
  - `apps/desktop-host`: Node-mode host process for Electron (`src/index.ts`, `office-engine.ts`, `update-tasks.ts`, `platform-session.ts`).
- `packages/`: 340 workspace packages in 52 group directories (`packages/README.md`, DOC). Manifest counts per group are in Appendix E.
  - Product groups: `core/` (agent, agent-loop, session, tools, system-prompt, scope), `api/` (Remote BFF: session, workspace, settings, terminal, job, account, workspace-files controllers, gateway), `client/` (63 Web UI packages: shell, connection, store, `ui-*`), `host/` (webserver, directory picker, open-in-app, plugin inventory, frontend static, product telemetry), `llm/` (provider seam, pi-ai, DeepSeek adapters, retry, token meter), `session/` (durable session data plane, JSONL, projections, titles, migrations), `session-query/` (SQLite FTS5 search, export), `shell/`, `terminal/`, `fs/`, `ssh/`, `subprocess/`, `sandbox/`, `computer-use/`, `browser-use/`, `web/`, `subagent/`, `jobs/`, `workflow/`, `schedule/`, `goal/`, `compaction/`, `context/`, `skill/`, `mcp/`, `lsp/`, `ptc-runtime/`, `extensions/`, `interaction/`, `credentials/`, `settings/`, `storage/`, `spill/`, `attachment/`, `deliverables/`, `document/`, `plan/`, `todo/`, `preset/`, `bundle/`, `boot/`, `sdk/` (JSON-RPC protocol and clients), `acp/` (automation ACP server), `typert/` (Remote type graph and registry), `util/` (17 zero-dependency utilities), `test-support/`.
  - `packages/experimental/` (44 manifests): capabilities under evaluation, shipped switched off by default. Listed in Section 3 and Appendix E.
- `vendor/`: pinned Cordis and related packages, with the rescoping policy in `vendor/README.md` (DOC).
- `native/`: `@deepseek-ai/node-addon-system`. The `entry` package provides a Linux Landlock launcher and POSIX `flock` through Node-API. Prebuild packages exist for `darwin-arm64`, `darwin-x64`, `linux-arm64` and `linux-x64`. No Windows prebuild exists (SRC).
- `python/`: `sdk` (import package `deepseek_harness`: `DeepSeekHarness`, `Session`, `HarnessClient`, Office helpers) and `sdk-runtime` (`deepseek_harness_runtime`, which bundles the platform `dsh` runtime; `platforms.json`) (SRC).
- `docs/`: `architecture.md`; `subsystems/` (67 English pages, one per subsystem); `cordis-tutorial/`; `cookbook/` (how-to guides for adding a package, tool, LLM adapter, Remote API, settings card); `user/` (Web UI, providers, schedule, GitHub review, MCP memory, network proxy, public deployments, Python SDK); `persistence-changes/`; `upgrade-guide/`; `postmortem/` (4 English entries, 0001 to 0004); `i18n/`; and generated catalogs (`tool-catalog.md`, `config-catalog.md` with 152 plugin sections, `cli-help.md`, `persistence-catalog.md`, `module-graph.md`, `graph-atlas.md`).
- `benchmarks/`: performance gates (`active-stream-reconnect`, `agent-continuation`, `conversation-fold`, `long-session-browser`, `session-corpus`).
- `snapshots/`: keyless recorded-session replay trees (`acp/`, `sdk/`, `session/`, `web/`; 600 `.jsonl` logs). Replayed through the `llm-replay` plugin (SRC, DOC).
- `scripts/`: gates and generators (`verify-*.ts`, `gen-*.ts`, `run-gates.ts`), release tooling (`scripts/release/{bump,pack,publish,verify}.ts`), `dev-web.ts`, `build.ts`.
- `website/`: VitePress documentation projection (DOC).
- `patches/`: 7 pnpm patch files (SRC).
- `.agents/`: maintainer agent workflows, not product features. 16 skills (`dsh-code-review`, `dsh-pre-push-checks`, `dsh-error-handling`, `dsh-prose-standard`, `dsh-create-upgrade-guide`, `dsh-translate-docs`, `dsh-speed-up-perf`, `dsh-find-simplifications`, `dsh-merging-stacked-prs`, `dsh-ci-test-reliability`, `dsh-client-ui-ux`, `dsh-doc`, `dsh-archive-agent-notes`, `dsh-trim-cot-leakage`, `agent-experience`, `record-browser-gif`) and about 2,454 decision notes under `.agents/notes/` (SRC, DOC).
- `.github/`: 23 workflows (CI, e2e, Windows packaging, node-addon-system, python-release, release, release-vendor, pi-ai-provider-e2e, build-preview-cloudflare, weighted-approval, issue-lifecycle, issue-policy and others), plus `review-ownership/`, `issue-management/`, `preview-sizing/` (SRC).
- `.gitlab-ci.yml`: a second CI definition (SRC).
- Root files: `AGENTS.md` (the detailed repo rules), `CLAUDE.md` (9 bytes containing the text `AGENTS.md`; the symlink was not materialized on this Windows checkout, INF), `README.md` and `.zh.md`, `SAFETY.md`, `BRAND_GUIDELINES.md`, `THIRD_PARTY_NOTICES.md`, `Makefile` (`make web|dev-web|desktop|dev-desktop|build`), `pnpm-workspace.yaml`, `tsconfig*.json`, `vitest*.ts`, `lefthook.yml`.

---

## 3. Feature candidates, grouped by taxonomy

Format: `- [TAG] capability: detail (pointer)`. Experimental items are marked `[exp]` and ship switched off unless the composition enables them (`packages/experimental/README.md`, DOC). Negative findings are marked `NOT FOUND`. Bullet count in this section: 191 (158 positive, 13 [exp], 20 NOT FOUND).

### 3.1 Chat experience

- [SRC] Streaming assistant output: live `agent/assistant-stream` start, chunk and end frames, committed as one `assistant/message` or a log-only `assistant/attempt` (`docs/architecture.md` "Turn flow", "Session log"; `docs/subsystems/llm-streaming.md`).
- [SRC] Markdown rendering: GFM plus math via micromark and mdast extensions and `rehype-katex`; Shiki syntax highlighting; `DiffBlock`, `TerminalBlock` (ANSI), `JsonTree`, `ImageLightbox`, `WebBlock`, `ReadBlock`, `SearchBlock` (`packages/client/ui-primitives/src/*.tsx`; `packages/client/ui-primitives/package.json`).
- [SRC] Mermaid diagrams: NOT FOUND in the Web chat client. Mermaid appears only in the docs site and the `verify-mermaid` gate (root `package.json`, `website/package.json`).
- [SRC] Per-turn usage and context meters: `TurnUsagePanel.tsx`, `StatsPills.tsx`, `TurnTailNodeView.tsx`, `ContextMeter.tsx`, `context-occupancy.ts` (`packages/client/ui-chat/src/client/chat/`, `packages/client/ui-conversation/src/client/`).
- [SRC] Queue and steer: `prompt(content, mode: 'queue' | 'steer')` appends a turn or interrupts the running one; pending queue items can be edited, removed or steered (`packages/api/session-controller/src/client/contract/session.ts`; `packages/client/ui-conversation/src/client/queue/QueueDock.tsx`).
- [SRC] Cancel with FIFO resume: `cancel()` stops the running turn and queued work resumes in order (same contract file).
- [SRC] Branch into a new conversation at a message: `forkAt` with the label "Branch into a new conversation" (`packages/client/ui-chat/src/client/apply.ts`, `chat/ChatNodeSeat.tsx`, `chat/TurnTailNodeView.tsx`).
- [SRC] Conversation node registry: `ConversationEventRegistry` extends a definition registry keyed by `ConversationNodeDefinition`, so a plugin adds a renderable conversation node type without a central switch (`packages/client/ui-conversation/src/client/conversation/event-registry.ts`, `assembler.ts`, `contract/conversation.ts`). Equivalence-style tests exist in `packages/client/ui-conversation/tests/conversation-assembler.client.spec.ts` (grep hit; the exact three-path test was not read).
- [SRC] Model retry status UI: scheduled, started, cancelled and failure states with delay (`message.retry.*` strings in `packages/client/ui-chat`; `packages/llm/llm-retry`).
- [SRC] History paging: `loadOlder()` adds at least 50 messages and two turn starts (cap 500); `loadThrough(seq)` jumps to a turn (same contract file).
- [SRC] Rename: `rename(title)` sets an explicit title that pins it against automatic regeneration (same contract file).
- [SRC] LLM session titles: first-prompt and all-prompts providers, durable title snapshots (`packages/session/session-title*`, `packages/experimental/session-title-all-prompts-llm`; `docs/subsystems/session-title.md`).
- [SRC] Sidebar: multi-level session tree, search, grouping, state dots, collapse to a 56px rail, Workspaces and Sessions; New Session uses the selected Workspace (`packages/client/ui-sidebar/README.md`).
- [SRC] Archive: archived-session gate in the session controller and an "archived-sessions" UI route (`packages/api/session-controller/src/archived-session-gate.ts`; `packages/client/ui-sidebar`).
- [SRC] Session search: sidebar "Search sessions"; model tools `session_search` and `session_trace`; SQLite FTS5 backend (`packages/session-query/session-query-sqlite`).
- [SRC] Session export: ZIP of the session tree (session, sub-sessions, attachments) from the header menu or `/export` (`packages/session-query/session-log-export/README.md`).
- [SRC] Projects: Workspaces (directory registry) are the project unit; sessions group under a Workspace (`packages/workspace/workspace`; `docs/user/guide/index.md` "Choose a workspace").
- [SRC] Keyboard shortcuts: registry, physical-key routing, recording and editable bindings (`packages/client/shortcuts`, `packages/client/ui-shortcuts`; `apps/desktop/src/keybindings.ts`, `keyboard.ts`).
- [SRC] Themes: light, dark and system, `--dsw-*` tokens and an Appearance row (`packages/client/ui-theme`).
- [SRC] Onboarding: versioned welcome notice and product onboarding dialogs (`packages/client/ui-settings-general`, `ui-settings-models`); Desktop Welcome window (`apps/desktop/src/welcome-window.ts`, `client/WelcomePage.tsx`).
- [SRC] Trajectory ledger: turn-aware timeline with a timing overview and a record inspector for tokens, duration, input and output (`packages/client/ui-trajectory/README.md`).
- [SRC] Localization: English and Chinese (zh-CN) typed dictionaries; 60 locale-bearing source files; `verify-client-ui-i18n` gate (`packages/client/locale/src/locales/en.ts`, `zh.ts`).
- [SRC] NOT FOUND: import of prior conversations (the "import" hits are module imports).
- [SRC] NOT FOUND: public shareable chat links.
- [SRC] NOT FOUND: edit-and-resend of a sent message, and regenerate of a response. The only "edit" paths act on pending queue items and the composer. The one "regenerat" hit is the schedule cron builder (`packages/client/ui-schedule/src/client/TaskDetail.tsx`).

### 3.2 Multimodal and creation

- [SRC] Image input: images in prompts (durable `ImageAttachmentRef`), `read_image` tool, image-capable routes via `input: [text, image]` (`packages/fs/tool-fs`; `docs/user/guide/providers.md` "Image input").
- [SRC] File upload: Agent-scoped streaming intake with staged receipt (`packages/client/file-upload`); attachments stored content-addressed under `DSH_HOME` (`packages/attachment/attachment-local`).
- [SRC] Image offload during compaction: over-budget request images are replaced with placeholders and the request is retried (`packages/compaction/compaction-image-offload`).
- [SRC] File previews: Markdown, code, PDF (`pdfjs-dist`), common images, HTML and plain text; Word and PowerPoint convert locally to PDF; spreadsheets open in the browser (`packages/client/ui-sidebar-documentpreview/README.md`).
- [SRC] Office creation and editing skills: Word, PowerPoint and Excel workflows, structural checks, file delivery; defaults to the bundled Python environment (`packages/skill/skill-office/README.md`).
- [DOC] Office conversion: DOC, DOCX, XLS, XLSX, PPT and PPTX to PDF; LibreOffice native engine where declared, otherwise Node WASM (`packages/document/office-to-pdf/README.md`, `docs/subsystems/office-to-pdf.md`).
- [SRC] Python SDK Office helpers: `convert`, `recalculate`, `render_images` with typed results (`python/sdk/src/deepseek_harness/office.py`).
- [SRC] Deliverables: the `present` tool declares up to four existing files as final deliverables; the Web shows delivery cards and changed-file comparison tabs (`packages/deliverables/tool-present`, `packages/client/ui-deliverables`).
- [exp] Voice input: microphone capture with guarded draft insertion (`packages/experimental/client-ui-voice-input`); speech-to-text through the `speech` Remote (`packages/experimental/api-speech-to-text`); local SenseVoiceSmall ONNX with Silero VAD on CPU (`packages/experimental/speech-to-text-sensevoice`, `sherpa-onnx-node`). Desktop microphone access is limited to the app frame (`apps/desktop/src/microphone-permissions.ts`).
- [SRC] NOT FOUND: text-to-speech or spoken output (no `speechSynthesis` or TTS provider in non-test code).
- [SRC] NOT FOUND: image generation or editing (no tool, provider or route).
- [SRC] NOT FOUND: video generation or analysis; realtime audio API.
- [SRC] Charts: no charting library in the Web client. Excel chart parts are parsed inside the spreadsheet preview (`packages/client/ui-sidebar-documentpreview/src/client/excel/xlsx-archive.ts`) (INF on scope, low confidence).
- [SRC] Code as the data-analysis and creation engine: `run_code` runs a TypeScript program (or Python when the runtime language is python) with `tools.name(args)` bindings and curated output (`packages/core/tools/src/ptc.ts`, `py-types.ts`; `packages/ptc-runtime/*`).
- [SRC] NOT FOUND: a dedicated deep-research mode. Research-style loops are composed from subagents, `workflow` and `ralph` (INF).

### 3.3 Research and web

- [SRC] `web_search` through `ctx.web` with three providers: DeepSeek native search via the Anthropic-compatible endpoint (`packages/web/web-search-deepseek`), Exa (`packages/web/web-search-exa`) and Perplexity (`packages/web/web-search-perplexity`). Provider selected by `DSH_WEB_SEARCH_PROVIDER`; API key, endpoint and per-request budget in Settings (`packages/client/ui-settings-web-search`).
- [SRC] `web_fetch`: anonymous public HTTP(S) provider (`packages/web/web-fetch-http`), HTML to Markdown (`turndown`), selected by `DSH_WEB_FETCH_PROVIDER`; fetch network policy in `docs/subsystems/web.md`.
- [SRC] Citations: the DeepSeek search provider builds a URL to cited-text map from `citations[]` (`packages/web/web-search-deepseek/src/provider.ts`, lines 118 to 126). The Perplexity provider also handles citations. `WebBlock.tsx` renders results (`packages/client/ui-primitives/src/WebBlock.tsx`).
- [exp] Translation: anonymous Bing (default) or Google text translation, with an optional paid DeepSeek Flash route; requires a durable session (`packages/experimental/translator`; `packages/experimental/client-ui-cot-translation`).
- [SRC] MCP resource tools for external data: `list_mcp_resource_templates`, `list_mcp_resources`, `read_mcp_resource` (`packages/mcp/mcp-resources`).
- [SRC] NOT FOUND: first-party SaaS connectors (Drive, Gmail, Notion, Slack, Jira). MCP is the only integration path.
- [SRC] Cross-session context: `@session` references carry untrusted model context (`packages/context/session-reference`; `packages/client/ui-reference`).
- [SRC] Read-only search over past sessions: `session_search`, `session_trace`, `session_event_search`, `session_event_read`, `session_event_trace`, authorized to the calling session (`packages/session-query`; owner `packages/experimental/tool-session-query`). Shipped status is open: the tools appear in the generated catalog without `[exp]`, but the owner sits under `packages/experimental/` (see Section 7).

### 3.4 Agent runtime

- [DOC/SRC] Turn and step loop: durable `turn/*` and `step/*` events; waterfalls `agent/pre-step`, `agent/request`, `llm/stream`, `tools/pre-execute`, `tools/execute`, `tools/post-execute`; serial `agent/turn-stopping` (`docs/architecture.md`; `packages/core/agent-loop`).
- [SRC] Parallel tool calls: `maxParallelToolCalls`, default 10 (`DEFAULT_MAX_PARALLEL_TOOL_CALLS` in `packages/core/agent-loop`); Settings card in `packages/client/ui-settings-agent-loop`.
- [SRC] Subagent tools: `subagent` (and `subagent_fork`), `list_subagent_models`, control tools `send_message`, `list_agents`, `interrupt_agent` (`packages/subagent/tool-subagent`, `tool-subagent-control`).
- [SRC] Subagent providers: `spawn-in-process` (fresh child on `ctx.agents`), `fork-in-process` (child seeded with a prefix of the parent log), `acp` (out-of-process ACP child), `claude-code` (one-shot Claude Agent SDK), `codex` (one-shot Codex app-server), `dsh-sdk` (child DSH runtime over JSON-RPC) (`packages/subagent/subagent-*`).
- [SRC] Subagent settings: recursion depth (`maxDepth`), parallel capacity, and which models agents may choose (`packages/client/ui-settings-subagent`).
- [exp] Agent Teams: a Lead, named teammates, direct messages, durable shared task DAG; tools `spawn_teammate`, `team_task_create`, `team_task_get`, `team_task_list`, `team_task_update`, `wait_agent`, `send_message`, `list_agents`, `interrupt_agent` (`packages/experimental/agent-team`, `tool-agent-team`, `client-ui-agent-team`).
- [SRC] Background jobs: `run_in_background` on bash and pwsh; `job_list`, `job_output`, `job_kill`; owner-isolated registry (`packages/jobs`, `packages/jobs/tool-jobs`).
- [SRC] Persistent terminals (opt-in): `terminal_open`, `terminal_send`, `terminal_read`, `terminal_signal`, `terminal_list`, `terminal_close`; owner-scoped persistent `bash` and `pwsh` (`packages/terminal`, `packages/shell/tool-bash-persistent`, `tool-pwsh-persistent`, `packages/terminal/terminal-bash`).
- [SRC] Scheduling: `schedule_create`, `schedule_list`, `schedule_update`, `schedule_delete`; one-shot, fixed-rate, daily, weekly and cron inputs; a cold session is restored when a reminder is due; each recurring task keeps only its latest missed occurrence; delivery goes to the original session (`packages/schedule/schedule/README.md`; `docs/user/guide/schedule.md`).
- [SRC] Goals: `create_goal`, `get_goal`, `update_goal`; pause, resume, complete, block, clear; compare-and-set updates; round cap, default 256; auto-continuation through `goal-round-driver` (`packages/goal/*`; `packages/client/ui-goal`).
- [exp] Ralph: repeated fresh child agents with a bounded structured handoff; the shared workspace is the only cross-round memory (`packages/experimental/tool-ralph`, `ralph-bundle`).
- [SRC] Workflows: `workflow` runs JavaScript orchestration in a fresh Node PTC process with `agent()`, `parallel()`, `pipeline()`, `phase()`, `log()` hooks; durable workflow-run Conversation node (`packages/workflow/tool-workflow`, `workflow-ptc`; `packages/client/ui-workflow-run`).
- [SRC] Plan mode: per-session logged plan state, `/plan` command, user-reviewed `exit_plan_mode` (`packages/plan/plan-mode`; `packages/client/ui-plan`).
- [SRC] Todo list: `todo_write`, session-owned, rendered as a checklist from the latest `todo/write` event (`packages/todo/tool-todo`).
- [SRC] Ask the user: `ask_user_question`, blocking by default; timed mode is opt-in (`packages/interaction/tool-ask-user`; `packages/client/ui-user-questions`).
- [SRC] Compaction: `compaction-basic` (token-meter-driven LLM summary), `/compact`, tool-result pruner (head, middle, tail), image offload; `compaction/*` session events (`packages/compaction/*`; `docs/subsystems/compaction.md`).
- [SRC] Spill: oversized tool output saved to private session files with retrieval locators (`packages/spill/spill-local`, `spill-policy`).
- [SRC] Crash durability: checkpoints before model requests, before tool side effects and before the next step; resume after crash (`packages/session/session-checkpoint-policy/README.md`).
- [SRC] Retry and guards: provider-routed LLM retry (`packages/llm/llm-retry`); per-tool deadline returning `TOOL_TIMEOUT` (`packages/guard/timeout-policy`); advisory repeat-call reminders (`packages/guard/repeat-tool-reminder`).
- [SRC] Monotonic tool guards: a deny-only layer evaluated after the extensible `tools/pre-execute` waterfall. Global, then scope-chain guard layers are checked and the first denial wins, farthest layer first. Guards are registered after the extensible pre-execute stage (doc comment near line 1133). The doc comments give the reason later plugins cannot shadow them (`packages/core/tools/src/index.ts` lines ~725, 767, 1133, 1150, 1490). How strictly this holds was not tested.
- [SRC] Compaction lock: `compaction/start` is log-only and holds a lock until `compaction/end`. A numbered compaction owner is enclosed by an open turn (`packages/compaction/compaction/src/types.ts` lines ~20-24; `packages/core/session/src/known-event-types.ts`).
- [SRC] Tool presentation per agent: native tool calls or PTC mode (`packages/core/agent-tool-presentation`).
- [exp] Hook bridges for Claude Code and Codex hook names: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart`, `Stop`, `SubagentStop` (`packages/experimental/hook-protocol`, `hooks-claude-code`, `hooks-codex`).
- [SRC] Resume and fork: `dsh headless --session-id` adopts a persisted session; explicit-id `session.create` resumes; live-session fork API (`docs/subsystems/session.md` "Live-session fork API"; `packages/api/session-controller`).
- [SRC] Turn outline and session stats projections (`packages/session/session-turn-outline`, `session-stats`).
- [DOC] Evaluation infrastructure: keyless recorded-session snapshots (600 logs under `snapshots/`), `llm-replay`, `llm-mock-server` with SSE faults, real-API e2e that self-skips without a key, per-file 100% coverage gate (`AGENTS.md` "Commands"; `docs/testing.md`; `packages/test-support/*`).
- [SRC] NOT FOUND: undo or rewind of workspace file edits to an earlier point. The "undo" hits are composer text editing (`packages/client/ui-conversation/src/client/input/`). Checkpoints are for crash durability, not user rewind.

### 3.5 Tools, plugins and extensibility

- [DOC/SRC] MCP client: stdio and Streamable HTTP servers; tools exposed as `mcp__<server>__<tool>`; server instructions; resources (`packages/mcp/mcp-client`; `docs/subsystems/mcp.md`; `docs/user/guide/mcp-memory.md`).
- [DOC] Default-off memory MCP reference configs: Memorix, MCP Reference Memory, Engram (`apps/cli/config/examples/mcp-memory/*.cordis.yml`).
- [SRC] Plugin packages: `dsh plugin add|remove|why` forwards to pnpm in the profile directory (`apps/cli/src/plugin.ts`); Web Plugins panel installs, enables, disables and retries (`packages/client/ui-plugin-manager`; `packages/boot/plugin-manager`); official bundles ship off; GitHub topic `dsh-plugin` (`README.md`).
- [DOC] Profiles and bundles: a profile is an ordered stack of bundles, `cordis.patch.yml`, a home patch and `--patch` overlays. Shipped profiles: `web`, `headless`, `sdk`, `sdk-minimal`, `acp` (`docs/architecture.md` "Profiles and bundles").
- [SRC] Skills: directories containing `SKILL.md`, discovered in `.dsh/skills` and `.agents/skills` (project), custom dirs, the user DSH home, `~/.agents/skills` and bundled roots, in that rank order; the model `skill` tool loads them (`packages/skill/skill-filesystem/src/index.ts`; `docs/subsystems/skills.md`).
- [SRC] Agent instruction files: `AGENTS.md` and `CLAUDE.md` loaded as workspace context (`packages/context/agent-instructions`).
- [SRC] References: `@file` (`packages/context/file-reference-local`, bounded fuzzy index), `@session` (`packages/context/session-reference`), `@subagent` and skill references (`packages/client/ui-subagent`, `ui-skill`); `/` commands and `@` triggers (`packages/client/ui-input-trigger`, `ui-commands`).
- [SRC] Slash commands: `/compact`, `/goal`, `/plan`, `/feedback`, `/permission`, `/export`, and a command popup (`packages/compaction/command-compact`, `packages/goal/command-goal`, `packages/plan/plan-mode`, `packages/feedback/command-feedback`, `packages/client/ui-permission-presets`, `packages/session-query/session-log-export`, `packages/interaction/commands`).
- [exp] Claude Code mods: `defineMod` wrapper runs mods as plugins; their hooks guard tools, rewrite prompts, add commands and tools, and draw a band above the prompt (`packages/experimental/claude-code-mods`, `client-ui-claude-code-mods`; `docs/subsystems/claude-code-mods.md`).
- [SRC] Runtime plugin definition (process-local, not persisted): `define()` on `ctx.dynamicCordisRunner` mounts a dual-half plugin package. The Host half runs in a `node:vm` realm; the browser half runs in the Client runner behind an approval UI. `cordis_define` is the tool-call row key used by the browser card (`packages/extensions/ui-cordis/src/client/CordisDefineRow.tsx`); no model-facing `cordis_define` tool appears in the 67-tool catalog. Pointers: `packages/extensions/cordis-host-runner/src/index.ts` (~line 170), `src/sandbox.ts`, `packages/extensions/cordis-client-runner`, `docs/subsystems/extensions.md`. Not in the experimental group; `packages/extensions/README.md` describes the cards as "historical".
- [SRC] Plugin-author inspection tools: `cordis_inspect_list`, `cordis_inspect_query` (`packages/extensions/tool-cordis`).
- [SRC] TypeScript SDK: `DeepSeekHarness` high-level turns API over stdio JSON-RPC (`packages/sdk/client`).
- [SRC] SDK JSON-RPC: newline-delimited JSON-RPC over stdio. Requests: `initialize`, `session/prompt`, `session/wait`, `session/working-directory/get`, `session/working-directory/set`, `shutdown`. Notifications: `session.event`, `session.status`, `subagent.started`, `subagent.finished` (`packages/sdk/protocol/src/types.ts`).
- [SRC] Python SDK: `DeepSeekHarness`, `Session`, `RunResult`, `HarnessClient`, `HarnessError`; launches `dsh --profile sdk` by default; the runtime wheel bundles the CLI per platform (`python/sdk/src/deepseek_harness/`; `python/sdk-runtime/`; `docs/user/guide/python-sdk.md`).
- [SRC] ACP server, automation-only: create or resume sessions, select model and reasoning effort, attach MCP servers, submit or cancel prompts, receive semantic updates, close sessions (`packages/acp/acp`; `dsh acp`).
- [DOC] CLI modes and flags (`docs/cli-help.md`; `apps/cli/src/args.ts`): `dsh web` (`--host`, `--port`, `--no-open`, `--public-url`, `--trusted-host`, `--tls-cert`, `--tls-key`); `dsh headless` (one task; `--json` run events; `--session-id`); `dsh acp`; `dsh sdk`; `dsh sdk-minimal`; `dsh plugin`; launcher flags `--patch`, `--dump-config`, `--dump-config-schema`, `--dump-default-config`, `--from-default-profile`.
- [exp] Webhooks: programmatic webhook rules create root sessions in a Web Workspace (`packages/experimental/webhook`); signed GitHub ingress (`packages/experimental/webhook-github`); review-session guide (`docs/user/guide/github-review.md`).
- [SRC] Bundled dependency payloads: `load_workspace_dependencies` returns absolute paths into the bundled Python, Node.js and pnpm payload (`packages/skill/tool-workspace-dependencies`).
- [SRC] Presets and personas: `agent-preset` and `agent-preset-registry` (per-agent Cordis YAML compositions) and a `persona` section (`packages/preset/*`); Web surfaces for the default and this session's seat (`packages/client/ui-agent-preset`).

### 3.6 Computer and environment control

- [SRC] Shell: `bash` and `pwsh` tools for one-shot, background and persistent use; managed `DSH_*` shell environment (`packages/shell/*`, `packages/shell/shell-env`).
- [SRC] Filesystem: `read`, `write`, `edit`, `str_replace_editor` (view, create, unique literal replace, line insert), `glob` and `grep` (ripgrep via `@vscode/ripgrep`); read-before-edit and version-guarded writes (`packages/fs/*`; `packages/fs/fs-observation-policy`).
- [SRC] Sandboxing: policies `read-only`, `workspace-write`, `danger-full-access`; backends bwrap (Linux), Landlock (npm launcher and `native/system/packages/entry`), macOS Seatbelt (`sandbox-exec`), Windows restricted token with a capability-SID ACL allowlist; enforcement reported as `full` or `partial`; fail-closed errors (`packages/sandbox/sandbox-local`, `sandbox-windows-acl`, `sandbox-policy`; `docs/subsystems/sandbox.md`). Known partial limits: `packages/sandbox/sandbox-local/README.md`.
- [SRC] Remote POSIX over SSH: filesystem, subprocess and sandbox providers over one OpenSSH helper (`packages/ssh/*`; `docs/subsystems/ssh.md`).
- [SRC] Git: branch and checkout creation (`packages/experimental/worktree`, `tool-worktree`, `[exp]`); per-turn workspace change snapshots from git working-tree state (`packages/deliverables/workspace-changes`).
- [SRC] Sidebar browser tabs for HTTP(S) pages, including loopback services; iframe on Web, Electron `<webview>` on Desktop; visited content gets no Node or Electron access (`packages/client/ui-sidebar-browser/README.md`; `apps/desktop/src/browser-guests.ts`).
- [exp] Browser automation, one provider at a time: Playwright MCP, Chrome DevTools MCP, Stagehand (AI-assisted act, observe, extract); Chromium first (`packages/browser-use/browser-use`; `packages/experimental/browser-use-*`; `docs/subsystems/browser-use.md`).
- [exp] Computer use: Cua Driver as an installed MCP executable or as the native npm runtime; one exclusive provider; the caller coordinates observe, act and verify (`packages/computer-use/computer-use`; `packages/experimental/computer-use-cua-driver-*`; `docs/subsystems/computer-use.md`).
- [SRC] LSP: `lsp` tool with `goToDefinition`, `findReferences` and related operations; stdio language servers keyed by file extension (`packages/lsp/*`).
- [SRC] Working directory: `working_directory` tool reads or changes the session directory; permission roots and origin stay fixed (`packages/session/tool-working-directory`).
- [SRC] Open in app: open the workspace in an installed application; the host serves the catalog and icons (`packages/host/open-in-app`; `packages/client/ui-open-in-app`).
- [SRC] Native directory picker in Desktop (folder dialog attached to the window) and a browse picker in Web (`apps/desktop/src/directory-picker.ts`; `packages/host/directory-picker-*`).
- [SRC] NOT FOUND: mobile app, device pairing or remote-device control. The "mobile" hits are a Safari skeleton style and app catalog entries.
- [SRC] NOT FOUND: deploy or hosting tool. The Cloudflare preview workflow is repo CI, not a product feature (`.github/workflows/build-preview-cloudflare.yml`).

### 3.7 Coding capability

- [SRC] Edit and apply: `edit` (exact replacement), `write`, `str_replace_editor`, `read`, `read_image` (`packages/fs/tool-fs`, `packages/fs/tool-str-replace-editor`).
- [SRC] Diff review: changed-files card with per-file comparison tabs and clickable file references (`packages/client/ui-deliverables`); `DiffBlock` (`packages/client/ui-primitives`).
- [SRC] Test and run loop: `bash`, `pwsh`, `run_code`, `workflow`, `ralph`. No dedicated test-runner tool was found.
- [SRC] Codebase navigation: `lsp` tool, `glob`, `grep`, and a bounded fuzzy file-reference index (`packages/context/file-reference-local`).
- [SRC] NOT FOUND: vector embeddings or a semantic codebase index. The "embedding" hits are DOM and dockkit terms.
- [SRC] NOT FOUND: inline tab completion.
- [exp] PR review: GitHub webhook ingress creates review Sessions (`packages/experimental/webhook-github`; `docs/user/guide/github-review.md`).
- [DOC] Repo-only maintainer skills, not product features: `.agents/skills/dsh-code-review`, `dsh-find-simplifications`, `dsh-speed-up-perf`, `dsh-pre-push-checks`.
- [DOC] Editor-facing protocol: ACP over stdio. The README describes it as automation-only (`packages/acp/acp/README.md`); IDE-style clients would be the natural consumer (INF).
- [SRC] Code-execution sandboxes: PTC Node provider (`packages/ptc-runtime/ptc-runtime-node`); PTC Python provider (`packages/experimental/ptc-runtime-python`, `[exp]`).

### 3.8 Memory, learning and personalization

- [SRC] Instruction files: `AGENTS.md` and `CLAUDE.md` loaded per workspace (`packages/context/agent-instructions`).
- [SRC] Persona and presets: `packages/preset/persona`, `agent-preset`, `agent-preset-registry`; Web surfaces set the default for later sessions and the current session's composition (`packages/client/ui-agent-preset`).
- [SRC] Cross-session recall: `session_search`, `session_trace` (read-only); `@session` references (`packages/session-query`, `packages/context/session-reference`). A global cross-session search bundle exists as `[exp]` (`packages/experimental/session-search`).
- [DOC] Third-party memory through MCP, all default-off: Memorix, MCP Reference Memory, Engram (`docs/user/guide/mcp-memory.md`).
- [SRC] NOT FOUND: a built-in long-term memory store, a learned user model, automatic preference extraction, or self-created skills. "Long-term memory" appears only in the Ralph docs, where the workspace plays that role (`packages/experimental/tool-ralph/README.md`).
- [SRC] Layered user settings: defaults, composition base, user document, with owner scopes (`packages/settings/settings`; `docs/subsystems/settings.md`).
- [SRC] Durable condensed context: session titles and compaction summaries (`packages/session/session-title*`, `packages/compaction/compaction-basic`).

### 3.9 Models and routing

- [DOC/SRC] DeepSeek direct: `llm-deepseek` (Messages adapter against `https://api.deepseek.com/anthropic`; base URL in `packages/llm/llm-deepseek/src/config.ts`), `llm-deepseek-api-key`, and `llm-deepseek-account` (account sign-in through browser PKCE, `packages/credentials/deepseek-account-platform`). Model ids in source: `deepseek-v4-flash`, `deepseek-v4-pro`; the default route is `deepseek-flash` (`packages/llm/llm-deepseek*`; `docs/user/guide/providers.md`).
- [SRC] Multi-provider routing through pi-ai: one configuration routes to pi-ai providers, OpenAI-compatible gateways and self-hosted servers. Catalog entries include `deepseek`, `openai`, `anthropic`, `openai-codex`, `azure-openai`, `zai`, `openrouter`, `moonshotai`, `mistral`. The `vllmPriority` option targets vLLM scheduling (`packages/llm/llm-pi-ai/src/catalog.ts`, `config.ts`).
- [DOC] Custom endpoints in the Web form: OpenAI Chat Completions (`openai-completions`), OpenAI Responses (`openai-responses`), Anthropic Messages (`anthropic-messages`). A provider speaks one protocol (`docs/user/guide/providers.md` "Add a custom model API").
- [DOC] Model discovery: "Fetch available models" reads common listing formats from an endpoint (`docs/user/guide/providers.md` "Discover models").
- [DOC vs SRC] OAuth sign-in: `llm-pi-ai` describes stored OAuth or interactive-key sign-in with cross-process refresh locking; `providers.md` says OAuth providers such as Codex are "not supported here yet". This is a conflict to verify (Section 6).
- [DOC] Reasoning effort per model: `off`, `low`, `high`, `max`, `xhigh`, mapped to the wire field `reasoning_effort`; compatibility switches `supportsDeveloperRole`, `maxTokensField`, `thinkingFormat: deepseek` (`docs/user/guide/providers.md` "Reasoning effort", "Request compatibility"; `packages/llm/llm-pi-ai/src/catalog.ts`).
- [SRC] Model picker and per-session model: the selected model becomes the default for new sessions; a session keeps the model in its log (`packages/client/ui-model-selection`; `docs/user/guide/providers.md` "Select a model").
- [SRC] Subagent model choice: `list_subagent_models` plus route validation before delegation (`packages/subagent/tool-subagent`).
- [SRC] NOT FOUND: cross-provider failover. The "fallback" hits are default-selection fallbacks (`packages/acp/acp/src/session.ts`) and summarizer defaults (`packages/compaction/compaction-basic`).
- [SRC] Prompt caching: cache-read and cache-creation token counts are shown (`packages/client/ui-chat/src/client/chat/token-format.ts`; `packages/client/ui-trajectory`); system-prompt updates are appended after cached history on capable routes (`docs/architecture.md` "Turn flow").
- [DOC] Token metering: immutable replay-aware measurements with consumed-log revisions (`docs/subsystems/token-meter.md`; `packages/llm/token-meter`).
- [SRC] Structured tool schemas: JSON-Schema DSL and projections to JSON Schema, TypeScript and Python types for PTC (`packages/core/tools/src/schema.ts`, `json-schema.ts`, `ts-types.ts`, `py-types.ts`). No general `response_format` feature was located for chat (INF, low confidence).
- [SRC] DeepSeek wire extensions: additive request-field registry and a lossless session-log request extension (`packages/llm/deepseek-llm-api-extensions`, `packages/session/session-log-deepseek`; `docs/deepseek-llm-api-wire-extensions.md`).
- [SRC] NOT FOUND: Ollama or LM Studio provider. Self-hosted OpenAI-compatible servers are supported through custom routes.
- [SRC] Per-route retry policy (`packages/llm/llm-retry`).

### 3.10 Transparency, observability and control

- [DOC] Model-visible means logged: every model request must be reconstructable from session events; `request/header` and `request/context` events record it (`docs/architecture.md` "Session log"; `AGENTS.md`).
- [SRC] Trajectory inspector: user, assistant, tool, subtool and compaction records with turn and step boundaries (`packages/client/ui-trajectory`).
- [SRC] Tool-call tree with keyed per-tool presenters (`packages/client/ui-tool`; `packages/core/tools` presentation).
- [SRC] Tool presentation tags: presenters return a `card` tag (for example `generic` or `web`), a `kind` and a `truncated` flag, so a capped result is not shown as complete (`packages/web/tool-web/tests/tool-web.spec.ts` lines ~106 and ~666; `packages/lsp/tool-lsp/src/render.ts` line ~187). Glob reports its cap as `truncated` (`packages/fs/tool-fs-search/src/glob.ts` lines ~254-257). The prior analysis's rule to recompute views at emission and not persist them was not re-verified.
- [SRC] Context-occupancy meter (`packages/client/ui-conversation/src/client/skeleton/ContextMeter.tsx`).
- [exp] Session inspector: sidebar tables for raw session logs and chat nodes (`packages/experimental/session-inspector`; `inspector-profile`).
- [exp] Host and client inspector: Chrome DevTools hub (CDP) for Host and Client consoles, network capture and the Cordis tree (`packages/experimental/inspector`).
- [SRC] Config transparency: `dsh --dump-config`, `--dump-default-config`, `--dump-config-schema` (`apps/cli/src/dump-config.ts`); read-only plugin inventory tab (`packages/host/plugin-inventory`, `packages/client/ui-settings-plugin-inventory`).
- [SRC] Startup diagnostics (`apps/cli/src/startup-diagnostics.ts`).
- [DOC] Telemetry channels: OTel ordinary events and Session-log OTLP (`packages/telemetry/otel`; `docs/subsystems/otel.md`). Product analytics are explicit-event only (`docs/subsystems/product-telemetry.md`). Session-log export to telemetry happens only after explicit feedback, with `FEEDBACK_ONLY` and `DISABLED` modes (`packages/session/session-telemetry-otel/README.md`).
- [SRC] Feedback: per-message like or dislike, a feedback dialog and the `/feedback` command; records are log-only (`packages/client/ui-message-feedback`; `packages/feedback`).
- [SRC] NOT FOUND: currency cost meter. Token counts are shown; billing pages link out to the DeepSeek Platform (`packages/client/ui-settings-account`).
- [DOC/SRC] Audit trail: approval audit events and permission-preset events in the session log (`docs/subsystems/approval.md`; `docs/subsystems/permission-presets.md`).
- [SRC] Performance gates in `benchmarks/`: stream reconnect, agent continuation, conversation fold, long-session browser, projection list.

### 3.11 Safety, permissions and governance

- [DOC/SRC] Permission presets: `workspace-write` with `ask`, and `danger-full-access` with `never`; `custom` is derived; `auto` is reserved for Auto review; a per-session `/permission` popup; the default preset lives in General settings (`docs/subsystems/permission-presets.md`; `packages/interaction/permission-presets`; `packages/client/ui-permission-presets`).
- [DOC/SRC] Approval seam: one-shot `ApprovalRequest` and `ApprovalOutcome`, per-session policy, answerers, audit events (`docs/subsystems/approval.md`; `packages/interaction/user-approval`; composer takeover in `packages/client/ui-approval`).
- [exp] Auto review: the current agent's provider and model assess each pending native or PTC inner call; allowed calls run under Full access; denied calls ask the user. Off by default (`packages/experimental/auto-review`).
- [SRC] Escalation: `bash`, `pwsh` and `run_code` accept a wider `sandbox_permissions` value (`workspace-write` or `danger-full-access`) with a `justification` shown for approval (`docs/tool-catalog.md`, `run_code` and `bash` sections).
- [DOC] Enforcement is reported, not assumed: `full` or `partial`, fail-closed (`docs/subsystems/sandbox.md`).
- [DOC/SRC] Secrets: credential references in config with values resolved per operation; keys are write-only in the UI; provider values are stored in `$DSH_HOME/.credentials.yaml`; `.env` is read as a provider; UI-safe `CredentialInfo` (`packages/credentials/*`; `docs/subsystems/credentials.md`; `docs/user/guide/providers.md`). Secret-bearing files were not read.
- [DOC/SRC] Web trust: the `/api` browser-trust fence checks Host and Origin; a process token is added to the first URL and then a cookie is minted (marked `Secure` under TLS); trusted-host allowlist; wildcard binds are rejected (`docs/subsystems/web-server.md`; `packages/host/webserver`; `docs/cli-help.md` for `--trusted-host`).
- [DOC] Desktop: loopback bind `127.0.0.1`; WebSocket credentials filtered by authority and scheme; microphone limited to the app frame; DevTools toggles disabled in embedded guests (`apps/desktop/README.md`).
- [SRC] Telemetry redaction waterfall (`packages/session/session-telemetry`; `docs/subsystems/session-telemetry.md`).
- [SRC] Untrusted-content marking: session references are described as "durable untrusted model context" (`packages/context/session-reference`). "untrusted" wording also appears in `packages/context/session-reference/src` and `packages/core/system-prompt/src` (keyword grep; exact labeling mechanism not traced, low confidence on the prompt side).
- [DOC] Warnings: the harness executes model-generated code and commands and loads third-party plugins; it has had no security audit (`SAFETY.md`).
- [SRC] NOT FOUND: zero data retention (ZDR). Zero hits for "zero data retention" or "ZDR". Upload of session logs with DeepSeek API requests is an opt-in setting (`packages/client/ui-settings-session-log`).
- [SRC] NOT FOUND: tenant, role, SSO, or organization-level audit. The product is single-operator. The "tenant" hits are inspector realm code.
- [SRC] Anonymous identity for telemetry and feedback correlation (`packages/identity/anonymous-user-id`).
- [DOC] Repo governance, not product: weighted approval workflows and review ownership (`.github/workflows/weighted-approval.yml`, `.github/review-ownership/`).

### 3.12 UI and UX design

- [SRC] Layout: three-column AppFrame with drag handles and a viewing-state service (`packages/client/ui-layout`); a split-tree docking kit with invertible operations (`packages/client/ui-dockkit`).
- [SRC] Right Sidebar tabs: files, browser, terminal, document previews, trajectory, changes, plans, team and subagent views (`packages/client/ui-sidebar-right`, `ui-sidebar-files`, `ui-sidebar-browser`, `ui-sidebar-terminal`, `ui-sidebar-documentpreview`, `ui-trajectory`, `ui-deliverables`, `ui-plan`, `ui-subagent`).
- [SRC] Composer: `/` and `@` triggers, command popup, reference chips, approval takeover, question takeover, goal bar above the composer (`packages/client/ui-input-trigger`, `ui-commands`, `ui-goal`, `ui-approval`, `ui-user-questions`).
- [SRC] Responsive layout: 55 CSS files use media queries; the sidebar collapses to a rail (`packages/client/ui-sidebar`).
- [SRC] Accessibility signals: 962 `aria-*` attribute uses and 56 reduced-motion references across `packages/client` (CSS and TSX).
- [SRC] Performance: list and chat virtualization with `@tanstack/react-virtual`; history paging; cached session list rows; projection stores (`packages/client/ui-chat`; `packages/api/session-controller/README.md`; `benchmarks/`).
- [SRC] Shortcuts: reference, recording and editable preferences (`packages/client/ui-shortcuts`).
- [SRC] Plugins page: capability settings cards plus plugin manager and inventory (`packages/client/ui-settings-plugins`, `ui-settings-*`).
- [SRC] Desktop chrome: tray, application menu (About, Check for Updates, Manage dsh Command), update window and mandatory-update overlay, quit confirmation, background notice, single-instance lock (`apps/desktop/src/*.ts`).
- [DOC] Brand rules: derivative projects should use "DSH" in names; the official brand package is `packages/client/ui-brand-official` (`BRAND_GUIDELINES.md`).

### 3.13 Platform architecture

- [DOC/SRC] Process model: `dsh` boots a profile (plugin tree) in Node. The web profile serves the SPA and Remote API from one Node host. Desktop runs an Electron main process plus a RunAsNode host child. SDK, ACP and headless modes run as stdio or one-shot processes (`docs/architecture.md`; `apps/desktop/README.md`; `docs/user/guide/python-sdk.md`).
- [SRC] Remote protocol: typed unary calls at `/api` and multiplexed streams at `/api/remote.mux` over WebSocket; contracts generated for Host and Client; zod-validated (`packages/api/gateway/src/stream-protocol.ts`, `stream-server.ts`, `client/stream-client.ts`; `docs/api-gateway.md`).
- [SRC] Connection client: `packages/client/connection/src/client/connection.ts` contains connection-generation, readiness and gap-repair terms (grep hit; semantics not read in depth). This matches the "generation plus readiness handshake plus gap repair" pattern in the prior analysis (section 13.2).
- [SRC] Electron IPC: channels named `dsh-desktop:*` for shortcuts, boot, directory picking, browser acquire and release, updates, native theme, window menu, device info and locale (`apps/desktop/src/ipc.ts`).
- [SRC] Wire formats: JSON-RPC 2.0 newline-delimited over stdio (SDK); ACP over stdio; OTLP over HTTP logs; MCP over stdio or Streamable HTTP; HTTP to model providers in Anthropic Messages and OpenAI-compatible shapes.
- [DOC/SRC] Session storage: append-only `session.vN.jsonl` (optional zstd) with immutable generations and adjacent migration packages v0 to v4 (`docs/subsystems/persistence.md`; `packages/session/session-persistence-jsonl`; `packages/session/session-format-v*-to-v*`).
- [SRC] SQLite and JSON storage: `storage-sqlite` (kv facet), `session-query-sqlite` (FTS5), `storage-json` (JSON KV), storage domains with schema validation and change events (`packages/storage/*`, `packages/session-query/session-query-sqlite`).
- [SRC] Projection cache and writer lock: `session_projcache` storage domain; `session/writer-held` for contention (`packages/session/session-projection-cache`; `packages/api/session-controller/README.md`).
- [SRC] Content-addressed attachments and spill files under `DSH_HOME` (`packages/attachment/attachment-local`, `packages/spill/spill-local`).
- [DOC] Distribution: npm package with bundles; pnpm-based plugin install; Desktop installers (Windows EV signing, macOS DMG and notarization) with mandatory-update policy and update journal; Python wheels bundling the runtime per platform (`apps/desktop/README.md`; `python/sdk-runtime/platforms.json`; `@yao-pkg/pkg` in root).
- [SRC] Native code: Node-API prebuilds for darwin and linux (`native/system/packages/*`). Windows uses FFI with Job Objects and restricted tokens (`packages/subprocess/win32-process`, `packages/sandbox/sandbox-windows-acl`). No Rust in-tree.
- [INF] Rust-portable design patterns found in this codebase: branded ids (`packages/util/brand`), typed event maps with merge-extensible declarations, append-only JSONL log with derived projections, Service Definition / Provider / Consumer seams, explicit `resolve(request): Spec` config step (`AGENTS.md` "Conventions"), generation-based session migrations (`docs/subsystems/persistence.md`).


---

## 4. Extension points

Each entry names the surface and where it is defined. All are SRC or DOC unless marked otherwise.

- **Cordis plugin composition (server).** Profiles are ordered bundle stacks plus `cordis.patch.yml`, a home patch and `--patch` overlays. A patch replaces a row by id or inserts rows. `cordis.yml` disables entries with `disabled`, and the `!!js` tag is allowed only under plugin `config` (`docs/architecture.md` "Profiles and bundles"; `AGENTS.md` "Secrets / .env"; `docs/cordis-primer.md`).
- **Plugin packages (npm).** Installed with `dsh plugin add <pkg>` (pnpm in the profile directory), from the Web Plugins panel, or from a prompt through the `plugin_manager` tool (`apps/cli/src/plugin.ts`; `packages/boot/plugin-manager`; `packages/client/ui-plugin-manager`; `docs/user/develop/basic/publish.md`; `docs/user/develop/practice/dynamic-cordis.md`). Package fields: `dsh.profile`, `dsh.bundle`, `dsh.client` (`docs/architecture.md`; `docs/subsystems/client-modules.md`).
- **Client (Web) plugins.** Typed Slots and Component Factories, props derived per slot, sidebar tab types, settings cards, command registry, shortcut registry, per-tool presentation slot, Conversation view nodes (`docs/subsystems/slots.md`; `docs/subsystems/sidebar-right.md`; `docs/subsystems/commands.md`; `docs/cookbook/adding-a-settings-card.md`; `packages/client/ui-tool`).
- **Tools.** Register a model-facing tool on `ctx.tools` with JSON Schema parameters, a presenter and deliverable/observation side effects. Also PTC binding namespaces that let `run_code` or `workflow` call tools (`docs/cookbook/adding-a-tool.md`; `docs/subsystems/tools.md`; `docs/tool-execution-pipeline.md`; `docs/subsystems/ptc-runtime.md`).
- **Capability seams.** Each seam has a Service Definition, one or more Providers and Consumers, and a registry keyed by branded id. Examples: `ctx.llm`, `ctx.subagents`, `ctx.sandbox`, `ctx.fs`, `ctx.shell`, `ctx.subprocess`, `ctx.terminals`, `ctx.web`, `ctx.lsp`, `ctx.skills`, `ctx.jobs`, `ctx.spillStore`, `ctx.workflowEngine`, `ctx.ptcRuntime`, `ctx.computerUse`, `ctx.browserUse`, `ctx.speechToText`, `ctx.translator`, `ctx.webhookRuntime` (`docs/capability-seams.md`; `docs/architecture.md` "Capability seams").
- **LLM adapters.** Provider adapter seam and cookbook; pi-ai routes configured in YAML or the Web form (`docs/cookbook/adding-an-llm-adapter.md`; `docs/subsystems/llm-streaming.md`; `docs/user/guide/providers.md`).
- **Remote API.** Typert `@Remote` methods in `packages/api/*` produce Host and Client contracts (`docs/cookbook/adding-a-remote-api.md`; `docs/api-gateway.md`).
- **Skills.** `SKILL.md` directories in `.dsh/skills`, `.agents/skills`, custom dirs, the user home and bundled roots; the `skill` tool loads them (`docs/subsystems/skills.md`; `packages/skill/skill-filesystem/src/index.ts`).
- **Instruction files.** `AGENTS.md` and `CLAUDE.md` loaded as workspace context (`packages/context/agent-instructions`). Persona and agent presets (`packages/preset/*`).
- **MCP servers.** Stdio or Streamable HTTP servers attached through config or ACP `session` creation (`packages/mcp/mcp-client`; `packages/acp/acp`; `docs/subsystems/mcp.md`).
- **Hook bridges (exp).** Claude Code and Codex hook events (`packages/experimental/hook-protocol`, `hooks-claude-code`, `hooks-codex`). Claude Code mods through `defineMod` (`packages/experimental/claude-code-mods`).
- **Runtime plugins (process-local).** `define()` on `ctx.dynamicCordisRunner` creates dual-half plugin packages at runtime, inspected with `cordis_inspect_*` (`packages/extensions/*`; `docs/subsystems/extensions.md`). Definitions do not survive restart.
- **Subagent providers.** Named provider registry on `ctx.subagents`; six shipped providers (`packages/subagent/subagent-*`; `docs/subsystems/subagent.md`).
- **Workflows and loops.** JavaScript workflow scripts in PTC Node (`packages/workflow/workflow-ptc`); Ralph (exp) and goal rounds (`packages/goal/goal-round-driver`).
- **Webhooks.** `ctx.webhookRuntime.register(rule)` and `dispatch(delivery)` (`packages/experimental/webhook`); signed GitHub route on `ctx.webServer` (`packages/experimental/webhook-github`).
- **Automation surfaces.** TypeScript SDK (`packages/sdk/client`); JSON-RPC protocol (`packages/sdk/protocol`); Python SDK (`python/sdk`); ACP server (`packages/acp/acp`); headless CLI with `--json` events (`apps/cli`).
- **Developer tooling.** Cookbooks for adding a package, vendored package, session format version, settings card and LLM adapter (`docs/cookbook/*.md`); the `dsh-*` maintainer skills under `.agents/skills/`.
- **Schema surfaces.** `dsh --dump-config-schema` emits the JSON Schema for profile entries and patches (`apps/cli/src/dump-config-schema.ts`); `docs/persistence-schema.json`; `docs/config-catalog.md` (152 plugin sections).

---

## 5. Data and protocol hints

- **Session log (durable source of truth).** Append-only events with `seq`, header and versioned file names: JSONL v0 `session.jsonl[.zstd]`, v1 and later `session.vN.jsonl[.zstd]`. Committed generations are never renamed or overwritten. Adjacent migration packages move `v0 -> v1 -> v2 -> v3 -> v4` (`docs/architecture.md` "Session log"; `docs/subsystems/persistence.md`; `packages/session/session-persistence-jsonl`; `packages/session/session-format-v0-to-v1` through `v3-to-v4`; `scripts`: `migrate:sessions-to-v4`).
- **Event vocabulary.** About 51 known session event types across about 28 families. Families include `tool`, `compaction`, `subagent`, `session`, `feedback`, `approval`, `turn`, `team`, `step`, `request`, `llm`, `image`, `hook`, `command`, `assistant`, `workspace`, `web`, `user`, `todo`, `system`, `schedule`, `sandbox`, `plan`, `permission`, `model`, `goal`, `developer`, `deliverables` (`packages/core/session/src/known-event-types.ts`, SRC count). Full catalog: `docs/persistence-catalog.md`, `docs/subsystems/session.md`.
- **Model-visible inputs must be logged** (`AGENTS.md` "Conventions"). Projections are derived from the log (`deriveMessages()`).
- **Storage.** Profile and home state under `$DSH_HOME` (environment variable `DSH_HOME`, default home resolved by `packages/util/home-paths`): `profiles/<profile>/cordis.patch.yml`, `.credentials.yaml` (provider keys), and `.env` (read as a credentials provider). These are documented paths (DOC); no file under `$DSH_HOME` was read. SQLite via `storage-sqlite` (kv facet) and `session-query-sqlite` (FTS5); JSON KV via `storage-json`; storage domains with schema-validated change events; projection cache domain `session_projcache`; attachments content-addressed (`packages/attachment/attachment-local`); spill files private per session (`packages/spill/spill-local`).
- **Web wire.** Unary Typert calls under `/api`; multiplexed streams at `/api/remote.mux` over WebSocket (`packages/api/gateway/src/stream-protocol.ts`, `REMOTE_STREAM_MUX_PATH`). Also `/api/file` for session media (`packages/api/session-controller/src/media-references.ts`), and `/open-in-app/apps`, `/open-in-app/icon`, `/open-in-app/open` (`packages/host/open-in-app`). Errors are `RemoteResult` and `RemoteError` with a code enum (`packages/typert/protocol/src/types.ts`).
- **SDK wire.** Newline-delimited JSON-RPC 2.0 over stdio; requests and notifications listed in Section 3.5 (`packages/sdk/protocol/src/transport.ts`, `types.ts`). ACP over stdio via `@agentclientprotocol/sdk 1.4.0`.
- **Model wire.** DeepSeek Messages-compatible endpoint `https://api.deepseek.com/anthropic` (and `/anthropic/v1`) (SRC grep, 9 + 5 hits); the bare base `https://api.deepseek.com` appears in `packages/credentials/deepseek-account-platform/src/index.ts` (SRC); reasoning is sent as `reasoning_effort` (DOC). Custom protocols: `openai-completions`, `openai-responses`, `anthropic-messages` (DOC).
- **Telemetry wire.** OTLP over HTTP for logs and session records (byte-bounded requests for session logs); product analytics events are explicit (`packages/telemetry/otel`, `packages/session/session-telemetry-otel`, `packages/host/product-telemetry-otel`).
- **Auth.** DeepSeek account by browser PKCE (`packages/credentials/deepseek-account-platform`); API keys (write-only in UI); pi-ai OAuth (conflict noted in Section 6); Web process token and cookie with Host and Origin checks (`docs/subsystems/web-server.md`). Desktop Platform and update requests send client headers (platform, version, locale, timezone offset) (`apps/desktop/README.md`).
- **Sync and multi-device.** No multi-device or cloud sync found. State is per profile and per Host; the Web client has reconnect semantics that reload requested projections (`docs/subsystems/web-client.md`; `packages/api/session-controller/README.md`). INF: DSH is local-first and single-operator.
- **Concurrency.** Session writer lock with `session/writer-held` contention; one live Agent per session; subagents and teams keep their own logs (`packages/api/session-controller/README.md`; `docs/subsystems/subagent.md`).
- **Desktop update channel.** Update check and mandatory policy with journal, backoff and jitter settings (`apps/desktop/src/update-*.ts`; env names `DSH_DESKTOP_UPDATE_CHECK_*`, `DSH_DESKTOP_MANDATORY_UPDATE_CONFIG`).
- **Native.** Node-API `node-addon-system` with a Landlock launcher and POSIX flock (`native/system/packages/entry`). No Windows prebuild. Windows uses koffi FFI and Job Objects (`packages/subprocess/win32-process/src/koffi.ts`).

---

## 6. Blockers and limits

**What I could not verify, and why**
1. No execution of any kind. `node_modules` is absent, so dependency versions were read from manifests only, and no build output, generated type, or test result was observed. The read-only rule also forbids running project code.
2. Generated catalogs (`docs/tool-catalog.md`, `docs/config-catalog.md`, `docs/cli-help.md`, `docs/persistence-catalog.md`) were treated as DOC. Their freshness gates exist (`verify-tool-catalog`, `verify-config-catalog`, `verify-cli-help`, `verify-persistence-catalog`), but I did not run them.
3. Sampling, not exhaustive reading. Covered by README summaries, package descriptions, targeted greps and read-only spot reads: about 340 package READMEs and manifests, the tool catalog, the core subsystem docs, the user guides, and the CLI, SDK and desktop entry points. Not read in depth: 600 snapshot session logs, about 2,454 `.agents/notes/` decision notes (titles and names only), 179 spec and e2e files under apps/web, checked in pass 2 (the earlier 295 figure was not reproduced), the `website/` content, benchmark bodies, `.gitlab-ci.yml`.
4. Vendored and native source: only headers and manifests of `vendor/*` were read. The native C source was not read; only the test oracle path was named. The Python SDK was read at symbol level (`python/sdk/src/deepseek_harness/*.py`), not behavior.
5. Not read by policy: `apps/desktop/.env.macos.example` and `.env.windows.example` (examples, not needed), `apps/desktop/installer/` and `apps/desktop/scripts/` signing and upload configuration (secret-adjacent; COS upload script identified by import only). No `$DSH_HOME`, `AppData` per-user, cookie, token, keychain or session-store file was accessed. No secret was observed. The repo has no root `.env`.
6. Negative findings are keyword-based (Appendix D). Tests were excluded. A feature that uses different vocabulary could be missed; each negative is labeled so the deep-dive agent can re-check it.
7. Experimental packages (44) and most desktop behavior (installers, update flow, tray, notarization) are described from READMEs and source only. Nothing was launched, so no behavior is confirmed at runtime. REA was not applicable (no DeepSeek build in scratch `extract/`).
8. Model behavior (DeepSeek V4 reasoning quality, tool-call accuracy, latency) is out of scope for a source inventory and was not measured.
9. Platform coverage: Landlock and Seatbelt enforcement exist only on Linux and macOS. Windows enforcement is partial by its own README (`packages/sandbox/sandbox-local/README.md`). I could not test any of these on this Windows host.
10. Broad `packages/` sweeps on this Windows checkout timed out (20 s ripgrep and 120 s bash limits). Those checks were re-run on narrower paths. Results are in Section 7.
11. Prior-analysis claims not reproduced in pass 2 (not refuted; see Section 7): keyed projection channel with higher-seq-wins, self-nominating approval slot, unauthenticated plugin manifest route, mux broadcast to every session, and `session.history` creating an agent.

**Doc and code conflicts to resolve in the deep dive**
- OAuth sign-in: `packages/llm/llm-pi-ai/README.md` says supported providers can use stored OAuth with refresh locking. `docs/user/guide/providers.md` says OAuth providers such as Codex "are not supported here yet" in the Web form. Check whether the Web form or the pi-ai catalog controls this path.
- `CLAUDE.md` is a 9-byte file containing `AGENTS.md`, not a symlink, on this checkout. Any tooling that expects a symlink will see plain text here.
- Python SDK version is the placeholder `0.0.0.dev0` in both manifests. Confirm the release stamping step.
- `@yarnpkg/cli-dist` is a root devDependency whose runtime role was not located.

**Hand-off notes for the deep-dive agent (what matters most for Verevon)**
- Single-operator design: no tenant, role or org model (Section 3.11). Verevon is multi-tenant, so ownership, Space ACL and ZDR questions do not transfer directly.
- Strongest transferable patterns: the event-sourced session log with model-visible-means-logged; generation-based session migrations; the capability seam template; permission presets with approval seam and LLM-based Auto review (exp); durable goals and scheduled reminders that restore cold sessions; typed Remote contracts with a multiplexed stream; sandbox enforcement reporting (`full` or `partial`) rather than assumed isolation.
- Gaps against the target product (Verevon chat and agent page, Manus or Perplexity or ChatGPT style): no image generation, no TTS or realtime voice, no deep-research mode, no shareable links, no regenerate or edit-and-resend of sent messages, no import of prior conversations, no connectors beyond MCP, no learned memory or self-created skills, no cost meter, no file-state undo or rewind, no mobile or device pairing.
- Strongest differentiators to study: transparency surfaces (trajectory ledger, request header and context logs, `--dump-config`, plugin inventory, token and context meters); runtime programmable composition (`cordis_define`, PTC `run_code`, workflows); queue and steer with editable pending work; workspace-changes review with per-file comparisons; Ralph-style fresh-context loops with a workspace as memory.
- Read-only audit complete. No repo files were changed, no project code or app was started, and no network requests were made. All evidence is local to the checkout and its docs.

---

## 7. Delta against Verevon's prior analysis (pass 2)

Compared with `C:/dev/CoresSystem/claude-hermes-deepseek.md` (sections 2.3, 5.1 and 13.2; dated 2026-08-16 and 2026-08-17; DOC) and `C:/dev/CoresSystem/harness-adoption-execution-plan.md` (scorecard and WS-5; DOC). Their claims were checked against this checkout (HEAD `d743267`, 2026-10-09).

**Repo growth since the prior analysis (SRC).** The prior analysis counted 226 pnpm workspace packages and a 688-entry ADR ledger. This checkout has 340 `package.json` files under `packages/` and 2,454 `.md` decision notes under `.agents/notes/`. The "about 430K TypeScript lines" figure was not recounted.

**Already adopted by Verevon (DOC, not re-verified here).** From the adoption plan scorecard: parallel tool-call dispatch, the postmortem and decision ledger, the tool-call retry drift contract, and the wire-identity tenancy chokepoint (recorded as already satisfied). WS-5 items 5.1 and 5.2 (stream-client hardening; recall and truncation indicators) are marked done on 2026-08-22.

**Newly confirmed in pass 2 and missing from the first inventory (SRC).**
1. Monotonic tool guards: deny-only layer after the extensible pre-execute waterfall (Section 3.4).
2. Compaction lock semantics: `compaction/start` holds a lock until `compaction/end` (Section 3.4).
3. Conversation node registry keyed by `ConversationNodeDefinition` (Section 3.1).
4. Tool presentation `card`, `kind` and `truncated` tags (Section 3.10).
5. Connection client with generation and readiness terms (Section 3.13).
6. `cordis_define` is a row key and a process-local API, not a shipped model tool (Section 3.5).

**Prior-analysis claims not reproduced in pass 2 (open for the deep dive; not refuted).**
- Keyed projection channel `{key, value, seq}` with higher-seq-wins: no hit in `packages/client/store/src`.
- Approvals in the composer through a self-nominating chain slot: no hit in `packages/client/ui-approval`.
- Plugin manifest and chunks served without authorization: no literal hit in `packages/host/webserver/src`.
- One mux stream broadcasting every session to every browser: not traced. The mux path matched only `packages/api/gateway/src/stream-protocol.ts` and `index.ts`.
- `session.history` may create or resume an agent just to read a transcript: not checked.

**Other open items.**
- The session-history tools (`session_search` and the others) appear in the generated catalog without `[exp]`, but their owner package is under `packages/experimental/`. `apps/cli/composition.md` lists `session-query-sqlite` in the base composition and does not list `tool-session-query`. Shipped status is unresolved.
- Three cited paths in the first draft were wrong and are fixed inline: `packages/guard/tool-call-timeout-policy` (wrong) is now `packages/guard/timeout-policy`; `packages/session-query/tool-session-query` is `packages/experimental/tool-session-query`; `packages/experimental/client-ui-cordis` does not exist (the browser card is in `packages/extensions/ui-cordis`).

---

## Appendix A. Third-party dependency counts (manifest count in parentheses)

Aggregated with `jq` over all 369 manifests (`dependencies`, `optionalDependencies`, `peerDependencies`, `devDependencies`). Workspace links (`@deepseek-ai/dsh-*`) excluded. SRC.

Top entries: zod (67), @types/react (66), react (65), react-dom (37), @types/react-dom (27), @testing-library/react (25), clsx (22), js-yaml (15), @types/js-yaml (10), yaml (8), typescript (8), koffi (8), tsx (7), chokidar (7), commander (6), ws (5), picomatch (5), vitest (4), vite (4), node-addon-require-builtin (4), fflate (4), execa (4), diff (4), @agentclientprotocol/sdk (4), sharp (3), semver (3), playwright (3), @xterm/headless (3), @tanstack/react-virtual (3), @opentelemetry/api-logs (3), undici (2), papaparse (2), open (2), mermaid (2), ipaddr.js (2), extract-zip (2), esbuild (2), ajv (2).

Single-manifest capability signals: @earendil-works/pi-ai, @anthropic-ai/claude-agent-sdk, @anthropic-ai/sdk, @openai/codex, @modelcontextprotocol/sdk (and server packages), @playwright/mcp, chrome-devtools-mcp, @browserbasehq/stagehand, @puppeteer/browsers, @trycua/cua-driver, sherpa-onnx-node, node-pty, typescript-language-server, @vscode/ripgrep, @octokit/webhooks, @js-temporal/polyfill, @fortune-sheet/core, @fortune-sheet/react, xlsx, exceljs, pdfjs-dist, lexical, katex, @xterm/xterm, shiki, simple-icons, anser, dompurify, turndown, @joplin/turndown-plugin-gfm, selfsigned, compression, got, electron, electron-updater, electron-builder, @electron/notarize, cos-nodejs-sdk-v5, @yao-pkg/pkg, @yarnpkg/cli-dist, smol-toml, zustand, immer, vitepress, vue, cytoscape, @panzoom/panzoom, dayjs, big.js.

---

## Appendix B. Model-facing tool names (67) and owning packages

Source: `docs/tool-catalog.md` (generated from the live registry; SRC via DOC). Shipped aliases: `subagent_fork` (tool-subagent). Experimental owners are marked `[exp]`. Upstream computer-use and browser-provider tools (Cua Driver, Playwright, Chrome DevTools) come from their own catalogs and are not in this list; the Stagehand tools are included.

- Filesystem: `read`, `write`, `edit` (`tool-fs`); `read_image` (`tool-fs`); `str_replace_editor` (`tool-str-replace-editor`); `glob`, `grep` (`tool-fs-search`).
- Shell: `bash` (`tool-bash`, `tool-bash-persistent`); `pwsh` (`tool-pwsh`, `tool-pwsh-persistent`).
- Terminals: `terminal_open`, `terminal_send`, `terminal_read`, `terminal_signal`, `terminal_list`, `terminal_close` (`tool-terminal`, `[opt-in]`).
- Code and workflow: `run_code` (`dsh-tools`, PTC); `workflow` (`tool-workflow`); `ralph` (`tool-ralph`, `[exp]`).
- Planning: `todo_write` (`tool-todo`); `exit_plan_mode` (`plan-mode`); `create_goal`, `get_goal`, `update_goal` (`tool-goal`).
- Scheduling: `schedule_create`, `schedule_list`, `schedule_update`, `schedule_delete` (`tool-schedule`).
- Delegation: `subagent`, `subagent_fork`, `list_subagent_models` (`tool-subagent`); `send_message`, `list_agents`, `interrupt_agent` (`tool-subagent-control`); `spawn_teammate`, `wait_agent`, `team_task_create`, `team_task_get`, `team_task_list`, `team_task_update` (`tool-agent-team`, `[exp]`).
- Jobs: `job_list`, `job_output`, `job_kill` (`tool-jobs`).
- Web: `web_search`, `web_fetch` (`tool-web`).
- Session history: `session_search`, `session_trace`, `session_event_search`, `session_event_read`, `session_event_trace` (owner `tool-session-query`, located under `packages/experimental/`; shipped status open, see Section 7).
- Skills and dependencies: `skill` (`tool-skill`); `load_workspace_dependencies` (`tool-workspace-dependencies`).
- User interaction: `ask_user_question` (`tool-ask-user`); `present` (`tool-present`, deliverables).
- Code intelligence: `lsp` (`tool-lsp`).
- Workspace and worktrees: `working_directory` (`tool-working-directory`); `create_worktree` (`tool-worktree`, `[exp]`).
- Plugins and plugin authoring: `plugin_manager` (`plugin-manager`); `cordis_inspect_list`, `cordis_inspect_query` (`tool-cordis`).
- MCP resources: `list_mcp_resource_templates`, `list_mcp_resources`, `read_mcp_resource` (`mcp-resources`).
- Browser (Stagehand, `[exp]`): `stagehand_act`, `stagehand_extract`, `stagehand_navigate`, `stagehand_observe`, `stagehand_screenshot`, `stagehand_tabs`.

---

## Appendix C. Developer-visible surfaces (route, channel, env and menu names)

**HTTP routes** (SRC literal scan): `/api` (unary Remote, 12 literal hits), `/api/remote.mux` (WebSocket multiplex, `packages/api/gateway/src/stream-protocol.ts`), `/api/file` (session media, `packages/api/session-controller/src/media-references.ts`), `/open-in-app/apps`, `/open-in-app/icon`, `/open-in-app/open` (`packages/host/open-in-app`). The GitHub webhook route is configured on `ctx.webServer` (`packages/experimental/webhook-github`); its literal path was not located.

**Desktop custom scheme and window:** `dsh-app://app/` loads the packaged Web entry (`apps/desktop/src/main.ts`, `apps/desktop/README.md`).

**Desktop IPC channels** (prefix `dsh-desktop:`, `apps/desktop/src/ipc.ts`): `shortcuts-input`, `shortcuts-close-window`, `shortcuts-get`, `shortcuts-edit`, `shortcuts-changed`, `shortcuts-recording`, `boot`, `enter-workspace`, `onboarding-active`, `onboarding-api-key`, `boot-failed`, `browser-acquire`, `browser-release`, `browser-open-requested`, `directory-pick`, `device-info`, `locale-bootstrap`, `locale-changed`, `updates-status`, `updates-open`, `updates-presentation`, `native-theme-set`, `window-fullscreen`, `windows-appearance`, `windows-menu`.

**Desktop application menu labels** (`apps/desktop/src/*.ts`): "About DeepSeek Harness", "Check for Updates…", "Manage dsh Command…" (Install, Repair, Remove; installs `/usr/local/bin/dsh` on macOS).

**Environment variables** (SRC scan, counts = source references; purpose not traced for most): `DSH_HOME` (63), `DSH_ENV_PREFIX` (18), `DSH_OUTPUT_ROOT` (14), `DSH_CLIENT_VERSION` (12), `DSH_ROOT` (10), `DSH_HOME_ENV` (10), `DSH_AGENTS_HOME` (10), `DSH_PACKAGE` (8), `DSH_WEB_URL` (7), `DSH_TELEMETRY_DISABLED` (4), `DSH_SUBPROCESS_RUNNER` (3), `DSH_SPEECH_TOKEN` (3), `DSH_PTC_RUNTIME_NODE` (3), `DSH_PROFILE_KEY` (3), `DSH_WEB_SEARCH_PROVIDER` (2), `DSH_WEB_FETCH_PROVIDER` (2), `DSH_BUNDLED_SKILL_DIR` (2), `DSH_EXAMPLE_MODE` (2), `DSH_CLIENT_TITLE` (2), `DSH_PERMISSION_MODE` (1), `DSH_SHELL` (1), `DSH_CONFIG` (1), `DSH_WORKSPACE` (1). Desktop build and runtime variables include `DSH_DESKTOP_UPDATE_CHECK_INTERVAL_MS`, `DSH_DESKTOP_UPDATE_CHECK_JITTER`, `DSH_DESKTOP_UPDATE_CHECK_MAX_BACKOFF_MS`, `DSH_DESKTOP_UPDATE_HTTP_IDLE_TIMEOUT_MS`, `DSH_DESKTOP_UPDATE_JOURNAL_DIR`, `DSH_DESKTOP_MANDATORY_UPDATE_CONFIG`, `DSH_DESKTOP_WINDOWS_SIGNTOOL`, `DSH_DESKTOP_WINDOWS_CER_FILE`, `DSH_DESKTOP_WINDOWS_TOKEN_PIN`, `DSH_DESKTOP_WINDOWS_KEY_CONTAINER`, `DSH_DESKTOP_UNSIGNED`, `DSH_DESKTOP_OPEN_DEVTOOLS`, `DSH_DESKTOP_HOST_INSPECT_PORT`. Test-only: `DSH_SNAPSHOT`, `DSH_SNAPSHOT_FILE`, `DSH_SNAPSHOT_OVERRIDE`.

**Feature flags.** There is no central flag registry. Features are switched by composition: experimental packages ship off unless a row is enabled (`packages/experimental/README.md`); official bundles can be off by default (for example `dsh-skill-badge`, `dsh-auto-review`, `dsh-voice-input-bundle`, `dsh-cot-translation-bundle`); `cordis.yml` `disabled` on an entry. Per-setting switches appear in Settings cards (for example the subagent recursion depth and the parallel tool-call cap, DOC/SRC).

**CLI flag names** (`docs/cli-help.md`): launcher `--profile`, `--from-default-profile`, `--patch`, `--dump-config`, `--dump-config-schema`, `--dump-default-config`, `-V`. `web`: `--host`, `--no-open`, `--port`, `--public-url`, `--trusted-host`, `--tls-cert`, `--tls-key`. `headless`: `--json`, `--session-id`. `plugin`: `--profile` plus pnpm pass-through (`add`, `remove`, `why`).

---

## Appendix D. Negative findings (NOT FOUND, keyword sweeps excluding tests)

Each entry names the feature Verevon might expect and the evidence used. Re-check these in the deep dive; keyword searches miss synonyms.

1. Image generation or editing (`image_gen`, `generate_image`, `image generation`, `dall-e`, `gpt-image`, `text-to-image`): 0 non-test hits.
2. Text-to-speech or spoken output (`text-to-speech`, `tts`, `speechSynthesis`): no provider or UI; 2 incidental file hits were not relevant.
3. Realtime audio API (`realtime`, `openai realtime`): 0.
4. Deep research mode (`deep research`, `deep_research`, `deepResearch`): 0.
5. Public share links (`share link`, `shareLink`, `share_link`, `public link`, `shareable`): 0.
6. Import of prior conversations (`import conversation`, `import session`, `session import`): 0 (hits were module imports).
7. Regenerate a response: the only "regenerat" hits are the schedule cron builder (`packages/client/ui-schedule/src/client/TaskDetail.tsx`).
8. Edit and resend a sent message: no dedicated flow; editing applies to pending queue items only (`updateQueue`).
9. Undo or rewind of file edits to a point in time: the "undo" hits are composer text editing (`packages/client/ui-conversation`).
10. Mermaid in the Web chat: 0 hits in `packages/client`.
11. Charting library in the Web client: none. Spreadsheet preview parses Excel chart parts only (`packages/client/ui-sidebar-documentpreview/src/client/excel/xlsx-archive.ts`).
12. Mobile app, device pairing or remote-device control: 0 product hits (mobile hits are Safari skeleton CSS and an app catalog).
13. Vector embeddings or a semantic index: 0 (embedding hits are DOM and dockkit terms).
14. Inline tab completion: 0.
15. First-party SaaS connectors (Drive, Gmail, Notion, Slack, Jira, Salesforce, Linear): 0. MCP is the integration path.
16. Zero data retention (`zero data retention`, `ZDR`): 0.
17. Tenant, role-based access, multi-user, or SSO: 0 product hits (the "tenant" hits are inspector realm code).
18. Learned user model, long-term memory store, self-created skills (`self-improv`, `skill_create`, `create_skill`, `user profile`, `persistent memory`): 0 product hits. "Long-term memory" appears only in Ralph documentation.
19. Ollama, LM Studio or llama.cpp providers: 0. A vLLM scheduling option exists (`packages/llm/llm-pi-ai/src/catalog.ts`).
20. Cross-provider failover: 0. The "fallback" hits are default-selection fallbacks (`packages/acp/acp/src/session.ts`) and summarizer defaults (`packages/compaction/compaction-basic`).
21. Currency cost display: 0. Token and cache counts are shown instead; billing links go to the DeepSeek Platform.
22. Rust or Go source or build: 0 (`Cargo.toml`, `go.mod`, `*.rs`, `*.go` absent from the repo).
23. Dedicated test-runner tool: 0. Tests run through bash.
24. A "Projects" object separate from Workspaces: none. A Workspace is a directory and the project unit.
25. Multi-device or cloud sync: none found (INF from the local-first design).
26. Deploy or hosting tool: none. The Cloudflare preview workflow is repo CI only.

---

## Appendix E. Package groups and experimental packages

Manifest counts per group under `packages/` (SRC, `find -name package.json`): acp 1, api 9, attachment 2, boot 5, browser-use 1, bundle 6, client 63, compaction 5, computer-use 1, context 6, core 8, credentials 5, deliverables 2, document 1, experimental 44, extensions 4, feedback 2, fs 7, goal 4, guard 2, host 9, identity 1, interaction 5, jobs 3, llm 9, lsp 3, mcp 2, plan 1, preset 5, ptc-runtime 2, sandbox 4, schedule 2, sdk 3, session 21, session-query 3, settings 1, shell 10, skill 5, spill 3, ssh 5, storage 4, subagent 9, subprocess 3, telemetry 1, terminal 2, test-support 7, todo 1, typert 11, util 17, web 6, workflow 3, workspace 1.

Experimental group (44 packages, `packages/experimental/README.md`, DOC): tool-session-query, session-search, ralph-bundle, terminal-bundle, badge-skill-bundle, session-titles-bundle, hook-protocol, hooks-claude-code, hooks-codex, webhook, webhook-github, session-title-all-prompts-llm, tool-terminal, tool-ralph, skill-badge, translator, client-ui-cot-translation, cot-translation-bundle, speech-to-text, speech-to-text-sensevoice, api-speech-to-text, client-ui-voice-input, voice-input-bundle, agent-team-profile, agent-team, client-ui-agent-team, auto-review, claude-code-mods, client-ui-claude-code-mods, ptc-runtime-python, computer-use-cua-driver-mcp, computer-use-cua-driver-native, browser-use-playwright-mcp, browser-use-chrome-devtools-mcp, browser-use-stagehand-native, browser-use-runtime, inspector, session-inspector, inspector-profile, tool-agent-team, tool-worktree, webworker-packer, webworker-runtime, worktree.

Scratch artifacts from this run (tool name list, dependency aggregation, manifest list) are in `scratchpad/tmp/`. They are working files, not reports.
