# DeepSeek Harness (dsh): transparency and customization deep dive

Facet: transparency-and-customization. Audit 3 (Model Plane plus Verevon chat and agent pages). Source: `C:/dev/deepseek-harness`, git HEAD `d743267` (2026-10-09), version `0.2.1-alpha.2`. Read-only audit; nothing was installed, built, run or launched. Companion file: `deepseek.transparency-and-customization.features.json` (173 records). Record numbers below are written R001 to R173 and map to the id suffix `deepseek-transparency-and-customization-NNN`.

Provenance tags: SRC = read in source files; DOC = repository documentation (READMEs, docs/, generated catalogs); INF = my inference with the basis stated. REA was not used because this is an open-source repository, not a shipped binary. Paths are relative to `C:/dev/deepseek-harness/` unless absolute.

## 0. What this pass did (resume note)

A previous attempt had left a 128-record features file and no narrative. This pass:

1. Re-checked every evidence citation in the 128 records: 439 citations, 0 missing paths, 3 line ranges that overshot the file by one line (fixed). It also compared 15 records line by line against the cited source (R003, R007, R015, R019, R029, R033, R044, R045, R048, R049, R059, R063, R067, R090, R093) and read about 20 more against what was seen elsewhere (for example R005, R006, R008, R024, R032, R034, R035, R037, R046, R058, R060, R061, R066, R072, R078, R101, R104, R107, R120, R121, R124, R126); no factual error turned up.
2. Mapped which package directories no record cited, then read the uncited ones that bear on transparency and customization (user-facing sidebars, composer menus, web search, voice, analytics, desktop shell, mods bridge, plugin governance, docs site, repo skills).
3. Added 45 records (R129 to R173) with line-level evidence, validated programmatically (paths exist, line numbers in range, categories exact).
4. Wrote this narrative and cross-checked several claims of Verevon's earlier DeepSeek analysis (section 10).

The facet is largely a DOC-plus-SRC picture: most user-facing behaviour is described in per-package READMEs that the repository gates for freshness, and I verified code only where a claim was load-bearing (analytics defaults, headless stderr, mandatory update, HTML sandbox, web provider selection, terminal limits, voice hash check, plugin approval flags).

## 1. Verdict in one page

DSH is the most transparency-minded agent harness in the reference set, and the transparency is structural rather than cosmetic.

- **One invariant carries the design:** everything the model can see is reconstructable from one append-only per-session event log, and a new kind of model-visible input must come with a new event type (R001; `AGENTS.md:134`, `packages/core/session/src/known-event-types.ts`). Request headers (R002), system prompt changes (R004), routing context (R003), retries (R019), approvals (R049), hooks and compaction are all log events, so the Trajectory view is a projection, not a second source of truth.
- **Metrics are honest by rule:** timing is "unavailable" rather than invented (R007), partial cache hits never round to 100% (R015), usage totals disappear rather than show a partial sum (R013), uncertain tool outcomes are labelled as uncertain (R022), and the token meter says it is a reference, not billing (R017).
- **Disclosure is layered:** Chat hides system and context rows and offers four work-details modes (R067, R160); Trajectory shows everything with 13 inspector tabs and a timing overview (R005 to R009).
- **Customization is the architecture:** every feature is a plugin row in a layered composition (bundles, profile patch, home patch, `--patch` overlays); `dsh --dump-config` prints which layer set each row (R029); settings forms edit only fields declared volatile, mark overrides and apply live (R071, R120); a written policy forbids hard-coded tunables (R121).
- **Safety policy is shown, and its limits are stated:** permission presets (R046), fail-closed one-shot approvals (R048), sandbox enforcement reported as a fact (R051), a plain statement that sandboxes do not guarantee isolation (R064), and per-backend process-visibility notes (R169).
- **Weak spots are real:** three vendor-bound data channels with uneven user control (session-log upload default-on, feedback telemetry, desktop product analytics with no user control, R059, R058, R129), logs-only OpenTelemetry with no operator tracing (R131), no per-turn currency cost (R045), approvals without actor identity or persistent rules (R049), workflow runs shown as identity and status only (R044), and a single-user trust model (R056; arch-facet R108).
- **Engineering process is itself a transparency feature:** generated, freshness-gated catalogs (R032), per-break upgrade guides (R139), postmortems and a decision ledger (R037), user-path performance budgets (R034) and a keyless recorded-session replay corpus (R035).

## 2. Architecture of transparency

### 2.1 The log is the product (SRC)

The session is an event-sourced JSONL log with a `seq` and time envelope. `SessionEventMap` declares 60 known event types (`packages/core/session/src/types.ts:313-330`, `known-event-types.ts:21-83`). The model's message history is derived from the log (`deriveMessages`), so a request can be rebuilt after the fact. Three kinds of record matter for transparency:

- **Surface events** that shape the conversation the model sees (messages, replacements, compaction results).
- **Log-only events** that never become messages: `request/header`, `request/context`, `approval/asked`, `approval/decided`, `hook/invoked`, `hook/result`, `llm/retry`, `compaction/*`, `image/offload`, `feedback/*`. The persistence catalog documents each one and states whether it is log-only (`docs/persistence-catalog.md`, for example `hook/*` at lines 560-610).
- **Embedded timing**: `assistant/message` carries the exact timed stream, so token timing for TTFT and decode throughput survives reload (R006; arch-facet R092).

Request headers (R002) record provider, model, reasoning effort, temperature, max tokens, which of those the adapter defaulted rather than the caller proposed, and the full tool schema list. A new header is appended only when something changed, and `reason` distinguishes `initial`, `resume`, `change` and `series`. The system prompt is surface node 0 plus positional replacements; `inspectRequestPrompt` reports whether the prompt, the tools or both changed between two requests (R004). Routing facts (context window, whether the route accepts in-history system-prompt updates) are a separate log-only record (R003) that the token meter reads for occupancy.

Every message and injected context also carries a provenance pair (R010): a source (user, model, tool, system prompt, or a plugin kind such as goal, schedule, webhook, subagent-settled) and a context form (instructions, catalog, snapshot, notice, relay, recall). The UI uses it to label injected context rather than passing it off as user text, and to explain turns that started without a human (R011: goal continuation, schedule, webhook, background job, subagent result, team message).

Design consequence (INF): because "model-visible iff logged" is a repository rule and not just a habit, a reviewer can ask of any change "where is the event?" That is a cheap, enforceable audit question. The cost is that the log stores full prompts, tool schemas and tool outputs on local disk; for a single operator that is a feature, for a multi-tenant product it is a Zero-Data-Retention problem (R001 notes).

### 2.2 Projections and meters (SRC and DOC)

Whole-log projections give figures that survive paging, reload and compaction: `session-stats` (turn and step counts, LLM time, tool time, TTFT, decode time and tokens; R014) and `token-meter` (usage, context pressure, context breakdown; R017). Key properties:

- The token meter is deterministic and replayable, uses provider-reported usage only when the request envelope matches, otherwise a fixed 4-characters-per-token estimate plus adapter-declared image pricing, and its README says the heuristic underprices CJK text and JSON schemas (R017, DOC).
- The context ring beside the composer divides projected tokens by the routed window and opens a breakdown into system prompt, tool definitions and messages (R016, SRC).
- The per-turn usage panel shows cache-hit percentage, uncached input, cached input, cache write, output and reasoning tokens, but only if every model attempt in the turn reported exact usage (R013, SRC).
- `formatCacheHitPercent` uses integer arithmetic and adds decimal places so a partial hit is never shown as 100% (R015, SRC, `packages/client/ui-chat/src/client/chat/token-format.ts:60-98`).
- A Performance and usage setting switches between compact (output speed and cache hit only) and detailed, and changes presentation only, never accounting (R068; `packages/client/ui-chat/README.md:61`).

### 2.3 Failure and recovery are visible (SRC and DOC)

Retries are durable and visible: `llm/retry` is appended before the wait, Chat shows a waiting status with attempt count and seconds, and Trajectory keeps retry counts and the last attempt error after recovery (R019). Turns end with a typed reason and a cancellation cause (R020). Failures carry stable machine codes (RATE_LIMIT, QUOTA, CONTEXT_WINDOW_EXCEEDED, EMPTY_RESPONSE and others) and the UI renders localized, actionable rows; an empty completion is a failure, not a silent end (R021). After a failed or cancelled step, unanswered tool calls get explicit synthetic results (outcome unknown, not started, aborted before dispatch) and the model is told to retry only idempotent work (R022). Output spills and truncations are announced with a pointer to the full result (R026). The repository's `dsh-error-handling` skill states the same principle for contributors: unknown outcomes must be visible and never reported as success or safe retry (`.agents/skills/dsh-error-handling/SKILL.md:20-26`; R168).

### 2.4 The format is governed (SRC and DOC)

Unknown required events refuse to load, while ignorable ones are skipped (R117). Session-format migrations are adjacent and never overwrite committed generations (R118). A persistence-change review gate and a finalization record exist (arch-facet R096). Together these make the log a durable interface, which is what makes the "log as audit record" claim credible.

## 3. The inspection surfaces

| Surface | What the user gets | Records |
|---|---|---|
| Chat, with four work-details modes | Compact, Standard, Detailed, Verbose levels of process visibility plus a collapse-timing preference; final answers never hidden | R067, R070 |
| Chat row filter | System, ordinary context and permission command rows hidden in every mode; non-human turn triggers and tool-catalog changes stay | R160, R011, R012 |
| Trajectory tab | Turn- and step-grouped ledger, timing overview with TTFT versus decode, search, fold-all, 13-tab inspector (Summary, Preview, Raw, Source, Payload, Result, Schema, Timing, Diff, System prompt, Tools, Options, Usage) | R005 to R009 |
| Composer dock | Activity and usage pills, context ring; pills are slot entries other plugins can replace | R014, R016, R078 |
| Turn footer | Expandable usage row, completion status; actions visible on hover for history | R013 |
| Tool cards | Preparing stage while arguments stream, truncation flags, spill notices, terminal cards, typed result cards | R024, R026, R163 |
| Changed-files card and deliveries | Per-turn file list from private git snapshots; explicit `present` deliveries; prose mentions linked from recorded facts | R038, R165 |
| Subagent catalog and workflow nodes | Catalog with activity and read-only history; workflow node shows identity and status only | R043, R044 |
| Experimental inspectors | Session Log inspector with a DOM-to-log element picker; Chrome DevTools hub with Host fetch capture | R027, R028 |
| Export and raw streams | Session ZIP export, raw event streams for SDK and headless | R039, R040 |
| Headless | Reasoning on stderr, final answer on stdout, exit code from the turn end reason | R132 |
| Mods band (experimental) | Token Weather context forecast, Blast Radius hold, Replay Theater stepping | R140 to R145 |
| Desktop | DevTools shortcut in packaged builds; quit and update warnings that name what would be interrupted | R136, R066 |

Chat versus Trajectory is a deliberate two-surface disclosure design (R160). Chat is for reading: it omits the system prompt and routine context so the transcript stays clean. Trajectory is for auditing: it shows request numbers, cumulative usage, the exact system prompt per request, tool schemas, request options and raw content blocks. A `Chat-to-Trajectory` deep link on a tool call connects them (R009). Neither surface fabricates data: in-flight rows show a start marker and no elapsed time (R007).

## 4. The customization architecture

### 4.1 Plugins all the way down (SRC and DOC)

The harness is built on Cordis (vendored): services, typed events and reversible effects, with nothing in the core privileged (`docs/architecture.md`; arch-facet R051). The user's customization surface is the composed plugin tree, not a settings database:

1. Bundle patches named in the profile manifest are applied first (in-box bundles: base, web-app, headless, sdk-app, sdk-minimal, acp-app).
2. Then the profile's own `cordis.patch.yml`, then the home-level `$DSH_HOME/cordis.patch.yml`, then each `--patch <path>` overlay in order. Later layers win per row (`apps/cli/reference/README.md:9-12`).
3. `dsh --profile <p> --dump-config` prints the composed tree with comments naming the source of every row and every later layer that patched it, without evaluating `!!js` expressions or mounting anything; `--dump-default-config` omits the user layers as a recovery diagnostic; `--dump-config-schema` prints a JSON Schema 2020-12 document for entries and patches (R029; `apps/cli/src/dump-config.ts`, `packages/boot/app-boot/src/index.ts:432-520`).
4. `dsh rescue --from-default-profile web` initializes a new custom profile from a shipped template under an exclusive directory claim, copying the bundle list but not dependencies or patches (R103).

Insight (INF): the provenance dump is the single best customization-transparency feature in the repository, because it makes "which layer changed this?" answerable without reading code. A system with a layered override model and no such dump is hard to debug.

### 4.2 Settings are forms over volatile config (SRC and DOC)

Only config fields declared `.volatile()` appear in forms. Pages stage edits and write on Save, show an Overridden badge with Reset to default, refuse a stale write when a higher layer would override it, and apply live through validate, then persist, then hot module reload (R071, R120). Secrets go through the credentials domain and are write-only in the UI (R055). Settings can open the profile `cordis.patch.yml` in the native editor on loopback browsers (R072). A legacy `settings.yaml` is imported once, renamed and never silently dropped (R124). The shell and web-search cards show the pattern in small: effective value, override badge, reset, save-or-discard (R154; `packages/client/ui-settings-shell`, `ui-settings-web-search`).

### 4.3 Modes: presets, permission presets, work details (SRC and DOC)

- Agent presets are per-session, pinned, user-visible modes such as standard, cordis, ptc and minimal; minimal fixes a one-line system prompt and mounts only the persistent shell (R105; `apps/cli/reference/README.md:133`).
- Permission presets bundle sandbox mode and approval policy (R046); new sessions in base profiles default to workspace-write, and `DSH_PERMISSION_MODE` changes the process fallback (R169).
- Work-details modes control process visibility (R067); Coding Tools is a progressive-disclosure switch for developer features (R073); tool presentation mode is `native` or `ptc` (`DSH_TOOLS_MODE`).

### 4.4 UI composition (SRC and DOC)

The web client is composed from typed slots; a plugin adds or replaces a UI element by registering the same id and order, with priority selecting the replacement (R078). Menus, composer triggers and settings pages are extension points: the unified `/` and `@` trigger menu takes sources from plugins (R151), skills resolve behind host commands (R152), preview renderers register by extension (R149), and the claimable quota notice lets a billing plugin own the top-up flow (R170). The mods band (R141) is a deliberately smaller seam: a serialized Box/Text/Button tree rather than injected components.

### 4.5 Extensibility is governed, not just open (SRC and DOC)

Plugin installation is gated like Full access and runs outside the workspace sandbox (R061). New in this pass: an install whose declared DSH peer range does not match is refused before pnpm runs, with a risk-acknowledged per-version exemption stored in the profile (R171); dependency build scripts need a named, persisted approval that the Web page shows exactly (R172). The honest weak point: the service validates the acknowledgement flag and the pending names but not that the user actually approved in conversation. Claude Code mods loaded through the bridge are not sandboxed at all (R145).

### 4.6 The written policy (SRC)

Repository rules make customization a design constraint: deployment-varying choices must be validated Config fields editable from `cordis.yml`, defaults are applied in one explicit step, misconfiguration fails at load, and protocol constants and security invariants stay fixed (R121; `AGENTS.md:138-140`). The generated config catalog (152 plugin sections) is cross-checked against the runtime schema so documentation cannot drift (R032).

### 4.7 Documented environment variables relevant to this facet (DOC and SRC)

| Variable | Effect |
|---|---|
| `DSH_HOME` | Harness home (default `~/.dsh`), holds profiles, logs, credentials, the home patch layer |
| `DSH_AGENTS_HOME` | Shared agent configuration root (default `~/.agents`); defined in `packages/util/home-paths/src/index.ts` |
| `DSH_TOOLS_MODE` | `native` or `ptc`; other values fail at boot (`apps/cli/reference/README.md:133`) |
| `DSH_PERMISSION_MODE` | Process permission fallback (`apps/cli/reference/README.md:131`) |
| `DSH_TELEMETRY_MODE`, `DSH_TELEMETRY_DISABLED`, `DSH_TELEMETRY_OTLP_URL` | Session-telemetry sharing mode, hard opt-out, destination (R058; `docs/user/guide/network-proxy.md:70`) |
| `DSH_PRODUCT_ANALYTICS_OTLP_URL` | Product-analytics destination (R129) |
| `DSH_WEB_SEARCH_PROVIDER`, `DSH_WEB_FETCH_PROVIDER` | Feed the same fields as `searchProvider` and `fetchProvider`, "not a hidden priority chain" (`packages/web/web/src/index.ts:50-84`; R154) |
| `DSH_DESKTOP_UPDATE_CHECK_INTERVAL_MS`, `..._MAX_BACKOFF_MS`, `..._JITTER` | Update polling (`apps/desktop/README.md:462`) |

## 5. Safety policy surfaced to the user

### 5.1 What the user is shown (SRC and DOC)

- Named permission presets (Read Only, Workspace Write, Full access, experimental Auto review), pinned per session, with a risk acknowledgement before Full access or Auto review (R046, R047).
- Fail-closed one-shot approvals: no answerer, a throwing answerer or a stray return value resolves to unavailable and the action does not run; the `never` policy rejects without prompting; the approval panel takes over the composer with Enter to allow and Escape to reject (R048).
- Sandbox enforcement reported as a fact (full or partial), and denial versus runner failure attributed with visible markers (R051, R052).
- The model is told the active permission policy and changes are announced in the log (R050); per-call escalation names the narrowest wider mode with a justification (R053).
- Plain language: "sandboxing, approval prompts and permission controls can reduce risk, but they do not guarantee isolation" (`SAFETY.md:13`, R064), and the CLI reference spells out that only bwrap hides host processes while Landlock and Seatbelt leave them visible (R169).

### 5.2 What leaves the machine, and who can switch it off

| Channel | Default | User control | Records |
|---|---|---|---|
| Session log suffix on official DeepSeek API requests (up to 8 MiB) | On | Settings switch for the log; plugin inventory field has only a config toggle | R059 |
| Session-log release through OTel | Feedback only (after the user records feedback) | `DSH_TELEMETRY_DISABLED`, mode variable, redaction waterfall, anonymous install id | R058 |
| Desktop product analytics (22 typed events, device and user ids) | On in Desktop, absent in Web | None in the UI; config file only | R129, R130 |
| Update and policy checks (platform, arch, version, locale, UTC offset) | On in packaged Desktop | Documented; I found no switch for the check; polling interval is configurable by environment variable; a mandatory-update policy can block plugin changes | R133, R134 |
| Web search and fetch | Provider-dependent; fetch needs no per-call approval but rejects non-public destinations (`apps/cli/reference/README.md:137`) | Provider choice and budget in the Plugins page | R154 |
| Voice model download | Never automatic | User starts preparation and picks the source | R155 |

INF: the interesting contrast is between R058, which is consent-shaped (nothing leaves until the user submits feedback, and the sharing mode is reported), and R129, which is a default-on channel whose own package states that there is no user-facing control. The product has the machinery for good controls and uses it unevenly.

### 5.3 Gaps in the surfaced policy

- Approvals record the tool, call id, reason and outcome (allowed-once, rejected, cancelled, unavailable) but not who or what decided, and the UI offers only allow-once and reject, so there are no standing rules (R049).
- Plugin and mod trust is documented as "you would run it as a plugin": no sandbox for hooks, no signing (R145, R061).
- There is no tenant, role or SSO concept; the trust fence authenticates reachability (launch token, session cookie, Host and Origin checks), not users (R056). The webserver README is explicit that there is no server-wide authentication and that a non-loopback bind exposes unprotected routes and static assets (`packages/host/webserver/README.md:130`, DOC).

## 6. Benchmarks, snapshots and tests as transparency

- **Performance budgets** are organised by user path (open and page a 240-turn session, first Trajectory, streaming while typing, session open, terminal I/O), run in fresh processes on synthetic data, and publish an HTML report that rescales each endpoint to modelled machines from a 2015 laptop to an M5; environment variables cannot override budgets (R034; `benchmarks/AGENTS.md`).
- **Recorded-session snapshots**: 600 committed session JSONL files across 227 scenario directories replay through shipped profiles without an API key and serve as the expected output, with ARIA snapshots for the Web UI; every model- or user-visible change must update a scenario in the same PR (R035; `snapshots/AGENTS.md`).
- **No agent-task quality benchmark ships.** `BENCHMARK.md` is two sentences pointing at the Python SDK's `jsonrpc-agent` variant and advising separate workspaces per task; the `sdk-minimal` profile exists so external benchmark harnesses can run a bare agent with no telemetry or extras (R101; arch-facet R115, SRC and DOC). For Verevon's evals purposes this is a gap, not a model.
- **Upgrade guides** (R139) and **postmortems plus decision notes** (R037) are the human-facing ledger of why behaviour changed.
- **Model Experience contract** (R033): every package README states what the model sees, the token effect and the KV-cache effect, enforced by a gate.
- **Docs for agents**: the documentation site emits raw Markdown twins and `llms.txt` (R166).

Counts for the decision ledger, from this checkout (SRC, counted with `find`): 1,224 English decision notes under `.agents/notes/` (41 proposed, 251 implemented, 924 archived, 8 rejected), plus a Chinese twin of each; the earlier inventory's "2,454" counts both languages.

## 7. Best-in-class (ranked)

1. **Log-as-truth with an enforced "model-visible iff logged" rule** (R001, R002, R004). No peer in the reference set states this as a repository invariant.
2. **Honest-by-construction metrics** (R007, R013, R015, R017, R022). Small rules with large trust effects.
3. **Two-surface disclosure plus work-details modes** (R160, R067, R005 to R009). Gives readers a clean chat and auditors a full ledger.
4. **Layered composition with a provenance dump and JSON Schema** (R029, R103). Strong customization debuggability.
5. **Fail-closed approvals and explicit sandbox facts** (R048, R051, R052).
6. **Streaming and long-history rendering**: incremental Markdown with resumable highlighting (R161), virtualized Trajectory with anchored windows (R005), local-echo reconciliation (R164), browser find that reveals text in collapsed details (R082).
7. **Process gates as product quality**: generated catalogs, upgrade guides, performance budgets, replay corpus (R032, R139, R034, R035).
8. **A bridge that runs another vendor's extension model** (R140): shows the plugin seam is general enough to host Claude Code mods with a compatibility table that lists every difference.

## 8. Weaknesses and limitations (ranked by impact for Verevon)

1. **No operator-grade observability.** OpenTelemetry is logs-only; no spans, no GenAI semantic conventions, no tenant dimension (R131). Local inspection is excellent, fleet inspection does not exist.
2. **Currency cost is absent.** Token and cache counts are shown, balances appear in Desktop settings, but no per-turn or per-session cost (R045).
3. **Uneven privacy controls.** Default-on session-log upload (R059) and analytics with no control (R129).
4. **Approval and trust model is single-user.** No actor identity, no standing rules (R049), no tenancy, no sandboxing of hooks (R145), plugins run with the host's authority (R061).
5. **Workflow runs are opaque.** The workflow node shows identity and status only, deliberately (R044).
6. **Log contents are sensitive and unredacted locally.** Full prompts and tool output are stored; startup failure reports can contain configuration or credential values and are not redacted, with a header warning (R031).
7. **Experimental surfaces are off by default and thinly proven** (mods, voice, inspectors, auto review); several limitations are documented only in package READMEs.
8. **Local-first assumptions** shape the UI: archive-only sessions, a native directory picker, open-in-app launches, a user terminal that bypasses agent sandboxing by design (R146, R150, R158).
9. **Heuristic token estimates** (4 characters per token) underprice CJK and JSON (R017).
10. **No Rust in the repository.** Rust-portable patterns exist (arch-facet R109), but nothing is reusable as code.

## 9. What Verevon should copy, adapt or avoid

Verevon's own baseline (`v-chat-agents-ui.baseline.md`, `v-modelplane.baseline.md`, read for calibration) already has per-answer cost, a context inspector, a durable run trace, cost dashboards, Mermaid, charts and a sandboxed HTML iframe. The items below are therefore the ones that fill documented Verevon gaps or raise the bar on existing features.

### Copy (highest value first)

| Item | Why for Verevon | Records |
|---|---|---|
| Incremental streaming Markdown (freeze completed blocks, resume highlight state) and virtualized long ledgers | Verevon lists no virtualization and a full re-parse per streamed token | R161, R005 |
| Honest-metrics rules (unavailable not invented; all-or-nothing usage; cache-hit never 100% when partial) | Cheap trust wins that apply to Verevon's turn timing and metrics badges | R007, R013, R015 |
| "Model-visible iff logged" as a rule for Model Plane runs, with a request-header event and system-prompt diff | Makes Verevon's run trace replayable and auditable; needs a ZDR-aware persist boundary | R001, R002, R004 |
| Two-surface disclosure: clean chat plus a full trajectory view | Verevon's Work panel and trace already point this way; DSH shows the split cleanly | R160, R005 |
| Provenance labels for injected context and non-human turn triggers | Helps explain schedules, watchers, goals and subagent results in chat | R010, R011 |
| Per-break upgrade guides with a verification step, gated in CI | Fits cross-plane contracts and the existing stale-doc register discipline | R139 |
| Layered config with a provenance dump and JSON Schema | Useful for org, Space and user policy layering where "who set this?" matters | R029 |
| Local echo reconciled by key with durable receipts | Needed for Verevon's mid-run queue and resume paths | R164 |
| Manual `/compact` with a tokens-saved report | Model Plane already compacts; no user control exists | R157 |
| Approval cards with computed impact (Blast Radius pattern) | Better than a bare command for computer-use and sandbox approvals | R143 |
| Per-backend sandbox exposure matrix in docs | Honest security communication | R169 |
| Docs with raw Markdown twins and `llms.txt` | Customer agents can read Verevon docs | R166 |

### Adapt

| Item | Adaptation needed | Records |
|---|---|---|
| Work-details modes (Compact to Verbose) | Map to Verevon's Work panel and "what happened underneath" telemetry rather than adding a second control | R067 |
| HTML preview with an inert default tier | Verevon already has a srcdoc iframe without same-origin; add the sanitized, no-script default for untrusted or ZDR artifacts | R148 |
| Plugin install gates and risk-acknowledged exemptions | Verify approval server-side with a token, which DSH does not | R171, R172 |
| Claude Code mods bridge idea | Offer a governed hook chain (guard, rewrite, render) with a declarative UI band, but sandboxed per tenant | R140, R141, R145 |
| Workspace and session manager (folded lists, running pinned, pending dots) | Verevon needs paging beyond 80 threads; archive-only conflicts with erasure under ZDR | R150 |
| Unified `/` and `@` trigger menu | Sources should be Data Plane documents, Spaces and threads; keep text-is-truth references | R151 |
| Local on-device voice with hash-pinned downloads | Optional local mode under ZDR; remote STT remains the default | R155 |
| Typed result cards with head-plus-tail folding | Replace free-form tool output dumps in run panels | R163 |

### Avoid

- A default-on analytics channel with no user or admin control (R129).
- A default-on upload of conversation logs to a vendor endpoint (R059); Verevon must propagate ZDR instead.
- Unredacted failure reports and local logs as the only audit trail (R031); Verevon needs tenant-scoped, redacted audit.
- Single-user trust assumptions: no actor identity in approvals, static assets without server-wide auth, mods with process authority (R049, R145).
- Archive-only sessions without delete (R150) where erasure rights apply.
- Treating a flag as consent: DSH's plugin tools accept `acceptRisk` and `approvedBuilds` without verifying conversation approval (R171, R172).

## 10. Delta against Verevon's earlier DeepSeek analysis

Reference: `C:/dev/CoresSystem/claude-hermes-deepseek.md` sections 2.3 and 13.2 (dated 2026-08-16 and 2026-08-17) and `harness-adoption-execution-plan.md`.

**Missed by that analysis (confirmed here from source or docs):**

- The Desktop product-analytics channel with default-on collection and no user control (R129, R130), and the update-check fingerprint and server-driven mandatory update block (R133, R134).
- The Claude Code mods bridge with its above-prompt band and three ready-made mods (R140 to R145), a direct bridge to the Claude desktop and CLI extension model.
- Per-break upgrade guides as a CI-gated artifact (R139) and the repo-owned agent skills (R168).
- The headless reasoning-to-stderr contract (R132).
- Plugin governance details: compatibility gate, risk-acknowledged exemptions, named build-script approvals (R171, R172).
- Two-surface disclosure (R160), the HTML preview's two tiers (R148), the user terminal that is deliberately outside agent permissions (R146), and the sidebar tools generally (R147 to R149).
- Docs for agents: raw Markdown twins and `llms.txt` (R166).
- Scale: the decision ledger now holds 1,224 English notes (the earlier analysis cited 688) and `packages/` has 340 `package.json` files (earlier analysis: 226 workspace packages), per the scout inventory (SRC, not re-counted here).

**Confirmed from the earlier analysis:**

- Approvals take over the conversation composer: `packages/client/ui-approval/README.md:11` (DOC).
- Static assets and routes are not protected by a server-wide authentication policy; route owners enforce their own: `packages/host/webserver/README.md:130` (DOC). This matches the "served unauthorized" concern but is stated by the package itself as a documented limitation.

**Not reproduced:**

- "`session.history` may create or resume an agent just to read a transcript": the current session-controller README says history pages, log following, search and attachment reads can inspect persistence without activating an Agent, and only prompt, queue, model, rename and file-reference operations may resolve or resume an ordinary Session (`packages/api/session-controller/README.md:34`, DOC). I did not trace the code, so the earlier claim may describe an older revision. Confidence: medium.
- The keyed projection channel `{key, value, seq}` with higher-seq-wins was not found in `packages/client/store` (the store README is 46 lines). It may live under session projection packages; not traced.

## 11. Open questions

1. Is product analytics' `enabled: true` ever surfaced to the user in the Desktop onboarding or welcome flow (the README says "no user-facing control"; an archived note records that a first-run internal-test notice about session-log upload was removed on 2026-08-13)? Needs a runtime look at the welcome pages, which this audit could not run.
2. Does the Plugin inventory field (`dsh_plugin_packages`) have a UI switch? The previous pass found none; the plugin-inventory settings tab is read-only (R030).
3. How complete is Trajectory for sessions with paged history when the system prompt page is not loaded? The README says the prompt stays unavailable until its page arrives (R160 notes).
4. Which of the experimental packages are intended to graduate (mods, voice, inspectors, auto review, session search)? The `proposed/` and `implemented/` notes under `.agents/notes/` would answer this; not read in depth.
5. Runtime logging: the Cordis console logger supports per-logger levels, but I found it mounted only in a test fixture; how operators change log levels in shipped profiles is unresolved.

## 12. Pointers

- Features: `C:/Users/ImaFernandesDaCosta/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-AppData-Roaming-Claude-scratch-workspaces-5e23d905-dd91-4518-8ff4-252efcbe0520-b5ff8b3a-437c-4499-9a7f-469de994f319-scratch-2026-10-12-69a4ee/1e4c8548-5aa8-429f-9d17-693211694c40/scratchpad/findings/t3/deepseek.transparency-and-customization.features.json`
- Scout inventory: `.../findings/t3/deepseek.inventory.md`; architecture facet: `.../findings/t3/deepseek.harness-architecture.features.json`.
- Key source anchors: `AGENTS.md:134-140`, `packages/core/session/src/known-event-types.ts`, `packages/client/ui-trajectory/README.md`, `packages/client/ui-chat/README.md`, `apps/cli/reference/README.md`, `packages/boot/app-boot/src/index.ts:432-520`, `packages/client/product-analytics/`, `apps/desktop/README.md`, `packages/experimental/claude-code-mods/`, `packages/boot/plugin-manager/README.md`.
