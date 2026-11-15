# DeepSeek Harness (dsh): harness architecture deep dive

Audit 3, facet "harness-architecture". Source: `C:/dev/deepseek-harness`, git `master`, HEAD `d743267` (2026-10-09, release 0.2.1-alpha.2). Companion data: `deepseek.harness-architecture.features.json` (162 records, ids `deepseek-harness-architecture-001` to `-162`). In this document `#NNN` means the record with that suffix. Sibling facet files (`deepseek.transparency-and-customization.features.json`, 173 records) cover the user-visible transparency and customization surface; this file deliberately concentrates on the engine and platform.

Provenance tags: **SRC** read in source files, **DOC** read in repository documentation, **INF** my inference with the basis stated. **REA** is not used: the repo is open source and no DeepSeek build exists in the scratch `extract/` folder. Nothing was run (no install, build, test, server or app launch), so every behavioural statement is a reading of code or docs, not an observation.

---

## 0. How this pass was done and what changed

A previous attempt had already written a 126-record features file but no narrative. This pass:

1. Re-validated the JSON (valid array, 126 records, no duplicate ids, every record has evidence).
2. Re-read the agent loop, session repair/surface, tool registry, tool-call scheduler, inbox, runtime-context, compaction (region, summarizer, pruner), token meter, retry, checkpoint policy, timeout policy, sandbox seam and local backend, native Landlock launcher and flock addon, subprocess seam, PTC Node runtime, MCP client, skill registry, system-prompt registry, web fetch/search, SSH transport, API gateway, remote-event forwarding, Python SDK, SDK protocol, the vendor log and three of the seven dependency patches (pi-ai, node-pty, osx-sign) directly from source.
3. Checked all 674 evidence citations mechanically (file exists, line number within the file): 0 failures. A 25-citation sample was read at the cited line: about 23 point at supporting text and 2 at section headings of the relevant README.
4. Corrected, upgraded or extended 24 pass-1 records (see section 14), added 36 records (ids 127-162), and dropped 9 drafts that duplicated pass-1 records or the sibling facet.

Pass-1 quality was high. The one factual error found was `#013` (it said the compaction checkpoint template has nine sections; the source lists eight). One wording imprecision was fixed in `#002` (claiming is a splice without outcome; `agent/inbox/claimed` is the live notification, not the durable event). Fourteen DOC-only records were upgraded to SRC after the code confirmed them.

---

## 1. What this system is, in one page

DeepSeek Harness is a **single-process, single-operator agent runtime written in TypeScript** whose every capability is a plugin on one dependency-injection context (Cordis). It ships a web UI (React 18, zustand, Lexical), a CLI (`dsh`), a headless mode, an automation SDK (stdio JSON-RPC, TypeScript and Python clients), an ACP server, and an Electron desktop shell around the web app [SRC/DOC]. There is no Rust or Go in the repository; the only native code is about 460 lines of C (a static Landlock launcher and a flock Node-API binding) [SRC, `#067`, `#104`].

Three ideas organize the whole design:

1. **The session log is the program state.** Every model request is a pure function of an append-only event log; anything the model sees must be logged; compaction, pruning, image offload and prompt changes are logged replacement events, never in-place edits [SRC, `#090`, `#094`].
2. **Everything interesting is an interception point.** The loop, tool pipeline, request recovery, approvals, even the browser UI are listeners on typed waterfalls with documented ordering, so policy and UX attach without patching the core [SRC, `#001`, `#056`, `#142`].
3. **Honesty over assurance.** Sandboxes report `full` or `partial` enforcement, synthetic recovery results say "outcome unknown, do not retry blindly", every package README must state its limitations, and tests must verify the world rather than the agent's self-report [SRC, `#066`, `#005`, `#143`, `#113`].

Scale (SRC counts from the scout, spot-checked): 340 workspace packages in 52 group directories, 195 root scripts (63 `verify-*`, 18 `gen-*`), about 2,450 decision notes, 227 recorded-session snapshot scenarios, Node `^22.19 || >=24`, pnpm 11.

---

## 2. Architecture at a glance

```
                       Surfaces (all call the same Host)
  Web UI (React)   Electron shell   dsh CLI/headless   SDK (JSON-RPC)   ACP server
        |                |                 |                 |               |
        +---- Typert RPC (HTTP /api + WebSocket /api/remote.mux) ------------+
                              |  forwarded Host events (emit + 2 waterfalls)
  ============================|=====================================================
   Host process (one Node process, one Cordis context, plugin rows from YAML)
   ---------------------------------------------------------------------------
   API controllers   session | workspace | job | terminal | workspace-files | settings
   Agent plane       agent-loop (ReactLoopAgent) -> waterfalls: pre-step, request,
                     request-error, turn-stopping, tools/pre-execute|execute|post-execute
   Policy plugins    approval, permission-presets, sandbox-policy, compaction (basic,
                     pruner, image-offload), spill, retry, timeout, repeat-reminder,
                     checkpoint, goal driver, schedule, jobs
   Core services     session (event log + surface), tools registry, llm runtime,
                     system-prompt registry, token meter, projections
   Capability seams  llm | fs | subprocess | sandbox | shell | terminals | web | lsp
   (Definition +     skills | mcp | jobs | browser-use | computer-use | speech
    Provider +       providers: deepseek-native, pi-ai (multi-provider), local fs,
    Consumer)        subprocess-local, sandbox-local, ssh-*, mcp-client, ...
   Storage           JSONL session store (zstd frames, flock lease), storage hub
                     (json|sqlite), attachments (content-addressed), spill files
   ---------------------------------------------------------------------------
   Native: landlock-run (static C), flock addon, koffi FFI (Windows), node-pty
```

Source anchors: composition rows `packages/bundle/base/cordis.patch.yml` (517 lines) [SRC]; seams and roles `docs/architecture.md`, `docs/capability-seams.md` [DOC, `#052`]; profiles and bundles `packages/bundle/*`, `apps/cli` [SRC, `#053`, `#101`].

### 2.1 Process and packaging model (category 13)

- **One Host process per user.** The Host owns the Cordis tree, the loop, the session log and all model transport. Jobs, terminals and subscriptions are process-local and die with the process [DOC, `#108`, `#025`].
- **Profiles** (`web`, `headless`, `sdk`, `sdk-minimal`, `acp`, plus desktop) are ordered bundle stacks plus the user's `cordis.patch.yml` and `--patch` overlays. A patch replaces a row's whole `config` by id or inserts rows; `!!js` expressions are allowed only in plugin config; `dsh --dump-config` reuses the exported `applyEntryPatches` so tooling never reimplements the algorithm [SRC `vendor/README.md` mod 11, `#053`].
- **Distribution**: single-file executables (`@yao-pkg/pkg` with a 400 KB local patch), per-platform Python wheels that bundle the runtime executable (manylinux_2_28, macosx_14, win_amd64) built by a hatch hook that rejects mixed-platform payloads, Electron installers with a bundled runtime (Python, Node, pnpm, Office libraries) installed offline [SRC `python/sdk-runtime/hatch_build.py`, `platforms.json`; DOC `#077`, `#102`, `#103`].
- **Two TypeScript programs.** Host and client code compile separately because both merge the same Cordis `Context` keys ("one program cannot see both", `tsconfig.host.json:2`). The host tsdown pass runs the Typert generator, which writes typed Remote contributions that the client pass consumes [SRC, `#158`].
- **Plugin resolution** is an in-process resolver table installed into Node's ESM and CJS resolvers, swapped as immutable generations, instead of symlinks or proxy manifests [DOC+SRC existence, `#151`].
- **Fail-loud policy**: uncaught exceptions and unhandled rejections print one `util.inspect` diagnostic, run a bounded (2 s) release hook and exit 1; the failed operation is never resumed. Desktop writes a crash report first [SRC, `#149`].

---

## 3. The data model: event-sourced sessions

### 3.1 Log, surface, projections

- **Log**: append-only typed events with a monotonic `seq`. At this checkout `KNOWN_SESSION_EVENT_TYPES` holds **60 types in 33 families** (turn, step, user/assistant/system/developer/tool messages, request/header, request/context, llm/retry, compaction, subagent, approval, permission, sandbox, goal, schedule, todo, working-directory, workspace, image, hook, team, ...) [SRC `packages/core/session/src/known-event-types.ts:22`]. The scout's figure (about 51 types, 28 families) was stale or counted a different set. Unknown records may be `ignorable`; experimental packages write only namespaced `plugin:<owner>/<record>` events [DOC, sibling `T117`, `T128`].
- **Surface**: the model-visible conversation is a derived ordered list of surface-eligible events (`system/message`, `developer/message`, `user/message`, `assistant/message`, `tool/result`). Each carries `surfaceOp: 'append'` or `{op:'replace', startSeq, endSeq}` with `sourceEventSeqs`. Replacements shadow ranges; the surface fold validates full coverage; empty system or developer nodes retain position without producing wire messages; `contentGeneration` and `replaceGeneration` counters let the loop detect a changed prefix [SRC `surface.ts:49-110, 222-262`, `#094`].
- **Role map is closed**: system, developer, user, assistant, tool. Session format V4 stores tool results as first-class `role:'tool'` messages; developer messages carry `tool-addition` and `tool-removal` blocks and require a `headerSeq` exactly when additions exist [SRC `surface.ts:166-198`, `repair.ts:168`, `#135`].
- **Request header**: `request/header` records config, adapter defaults and tool schemas with a reason (`initial`, `resume`, `change`, `series`); `request/context` records window and update modes. A "request series" is a run of requests whose prefix must stay byte-stable; the loop starts a new series when the surface generation or tool set changes on a route that cannot take incremental updates [SRC `agent.ts:616-704`, `#003`].
- **Projections**: registered fold units (`turnBoundary`, `inbox`, `llmRetry`, `permissions`, `workingDirectory`, token usage, pressure, breakdown, goals, todos, subagent catalog) fold events synchronously, are replay-deterministic, and are checkpointed by a throttled projection cache (every 200 events or 5 s) [SRC `cordis.patch.yml:191-195`, `#097`].
- **Immutability**: messages and requests are deep-frozen; tool arguments are snapshotted as lossless JSON (no BigInt, cycles, `-0`) at every trust crossing [SRC `agent.ts:686-703`, `tools/index.ts:1447-1451`].

### 3.2 Persistence and evolution

- **Store**: one JSONL file per session in checksummed zstd frames (or plain lines), fsync per batch, torn tails truncated and rewritten, immutable `session.vN.jsonl[.zstd]` generations, adjacent `vN to vN+1` migrations checked gap-free at init, and a cross-process writer lease through `flock(2)` (native addon) or a named kernel semaphore on Windows [SRC/DOC, `#095`, `#096`]. Current writer format is **4** (`SESSION_FORMAT_VERSION`, `types.ts:89`).
- **Lease as the arbiter**: blank-session reuse, resume and fork all decide ownership by acquiring the writer lease; only `session/writer-held` triggers creating a new session [SRC, `#152`, `#129`]. Reads that would trigger a format migration (sidebar fork, catalog expansion) are refused with `migration-required` instead [SRC, `#153`].
- **Governance**: finalization and release records, a persistence-change review gate and an "attribution-only" qualifier for new message source kinds keep released data readable [DOC, `#096`].

### 3.3 Fork and recovery share one mechanism

`buildForkSeed` copies a source prefix, appends `session/end-seed {inherited:true}` and then the same synthetic closers used for crash recovery, with cause `forked` and different model-visible wording [SRC `fork.ts:21`, `repair.ts:53`]. `ToolCallRecovery` tracks unanswered tool requests and emits deterministic error results: `TOOL_NOT_STARTED`, `TOOL_OUTCOME_UNKNOWN` (text tells the model to retry only read-only or idempotent work, otherwise verify external state or ask) and `ABORTED_BEFORE_DISPATCH` [SRC `repair.ts:15-41, 105-198`, `#005`].

---

## 4. The agent loop

### 4.1 Structure (SRC `packages/core/agent-loop/src/agent.ts`)

A `ReactLoopAgent` has three phases: `idle`, `maintenance` (exclusive setup work such as create/resume) and `running` (turn and step counters, an `AbortController`, a wake latch). Input enters one durable **inbox** with two lists (`next-turn`, `next-step`); `followup`, `steer` and `inject` are fixed presets of `send(message, target, wakeup)`. Every inbox mutation is a normalized `agent/inbox/spliced` event; the fold rejects out-of-range splices and duplicate message ids [`#002`, `#127`]. A waking message sent into an aborted activity is reclassified as `next-turn` and latched for replay at convergence; disposal never latches, so teardown cannot start a model turn [`agent.ts:154-235`].

A **turn** appends `turn/start`, then loops steps. A **step** does: claim inbox input, assemble the system prompt, run the `agent/pre-step` waterfall (`reject` or `enter`), append `step/start`, resolve the route (`agent/request` waterfall then `llm.prepareCall`), reconcile system-prompt nodes, append user messages once, log header and context when changed, derive and freeze the request, stream the response through an `AssistantStreamAttempt`, settle `assistant/message` (or `assistant/attempt` on failure), run the `agent/request-error` waterfall (retry or fail), execute tool calls, and append `step/end`. Admission is synchronous after the single async route preparation, so a cancel during preparation commits nothing [`#003`].

**Turn endings** are one of `completed`, `max-tokens` (sticky across later steps), `blocked`, `aborted` (typed cause copied, never the abort stack), `error` (structured failure) plus `interrupted` and `forked` from synthetic closers. A tool result may carry `concludesTurn` so structured-output and plan-exit tools end a run without another model call; the serial `agent/turn-stopping` event runs before the loop breaks so a goal driver can still queue input [`agent.ts:338-398, 551-555`, `#128`].

**Cancellation preserves the prefix** the user already saw: partial text becomes an `assistant/message` with `interrupted:true` and the exact embedded timed stream; an empty prefix becomes `assistant/attempt` [`agent.ts:462-503`, `#004`].

### 4.2 Recovery and durability

- **Retry** is provider-owned policy executed by `llm-retry` on `agent/request-error`: normal mode retries `EMPTY_RESPONSE`, `RATE_LIMIT`, `SERVER`, `TIMEOUT`, `TRANSPORT` up to 5 times with 500 ms to 10 s exponential backoff and 10% jitter, honoring `Retry-After` when within the cap; `always` mode retries every failure until success, cancel or disposal. Each retry is logged (`llm/retry`) before its cancellable wait and counters live in a session projection reset on `step/start` and `turn/end` [SRC `llm-retry/src/index.ts:59-241`, `retry-policy.ts:14-35`, `#008`].
- **Checkpoints** are an 83-line plugin with three listeners: flush before the first model chunk is requested, flush before a top-level tool body runs (nested calls reuse the durable outer call), flush before each next step. A failed flush blocks the adapter or tool [SRC, `#006`].
- **Cold resume**: take the write lease first, cold-read, append `interruptedTurnClosers` through the same handle, seed the Session, flush the unstored suffix, run setup in the maintenance phase, then publish to registries [SRC `agent-loop/src/index.ts:852-926`, `#129`].
- **Lifecycle**: one memoized reverse teardown fuses caller, owner-fiber and factory abort sources, waits for any publication, cancels with `{kind:'disposed'}`, awaits idle, disposes the scope, closes and drains the write handle, leaves registries, and rejects once with an `AggregateError` [SRC `index.ts:497-676`, `#130`].

### 4.3 Higher-level behaviours built on the loop

Compaction, goals (durable same-session goals with a round driver and race fences), scheduled reminders, background jobs and wake budgets, subagents (spawn and cache-preserving fork), structured subagent results via a terminal tool, agent teams (experimental), Ralph loops (experimental), workflows in a sandboxed Node child, plan mode (guidance only; the tool catalog stays fixed for cache stability), and a model-switch notice are all plugins on the loop's waterfalls [`#011`-`#034`, `#134`].

---

## 5. The tool system (category 5)

### 5.1 Definition contract (SRC `packages/core/tools/src/index.ts`)

`defineTool` declares typed parameters (JSON Schema emitted with `required` in declaration order, which controls argument order on the wire), a **mandatory output contract** `{schema, render, presentationMeta?}`, optional `projectContent`, `finalizeContent`, `timeoutMs`, `isConcurrencySafe(args)`, and pure `presentCall`/`presentResult` for UI cards. The body returns a JSON value; the registry snapshots, validates (`INVALID_TOOL_OUTPUT`), freezes, renders and attaches presentation meta. Schemas also project to TypeScript declarations and Python stubs for PTC mode [`:214-299, 1838-1868`, `#055`, `#131`].

### 5.2 Execution pipeline

`prepare` (create execution with frozen lossless-JSON arguments, caller-cancel check, **`tools/pre-execute` waterfall** returning allow, deny, ask or cancel, approval service for ask, monotonic **guards** that can only deny) then `dispatch` (**`tools/execute` waterfall** around the body, with the caller signal fused back so a wrapper can replace but never remove cancellation; a started body is never abandoned) then `finalize` (**`tools/post-execute`** accept, replace value or content, or block with feedback, then `projectContent`/`finalizeContent`) then freeze and notify `tools/result` observers [`:1375-1826`, `#056`]. `ask` maps to four outcomes with distinct model-visible reasons (`allowed-once`, `rejected`, `cancelled`, `unavailable`); a missing approval seam degrades to deny [`:1733-1774`].

**Scheduling** (`tool-calls.ts`): calls form exclusive barriers or a bounded rolling pool (`maxParallelToolCalls`) of calls whose `isConcurrencySafe` returns exactly `true`; modes are re-read before each start so registry changes create barriers; pre/post policy and result commit stay in model order while bodies overlap; abort drains started calls and writes synthetic results for the rest [SRC `:60-247`, `#007`].

**Extras** not in pass-1: a tool or composite can `deferContext` extra messages that land after its result in model order [`#132`]; `concludeTurn()` ends the turn [`#128`]; in PTC mode a registry-level **collapse** denies any direct non-`run_code` call before policy runs, with a route hint, so no listener can approve a call that can only fail [`index.ts:1357-1480`, `#133`].

### 5.3 PTC mode and the runtime sandbox

The model calls tools by writing a TypeScript program (`run_code`); sub-calls re-enter the full guarded pipeline with ids `<parent>:ptc:<n>`. The Node provider starts a **fresh process per call**, strips types, confines argv through the sandbox, resets `process.env`, and talks to the host over a length-framed JSON channel on **fd 7** (a dedicated control descriptor in the subprocess seam, so program output can never forge protocol). Ceilings: 120 s default and 600 s cap wall time, 64 MiB output, 512 MB V8 old generation, 128 pending calls, 128 MiB frames; call ids must be strictly sequential and names declared. Every end state is one of eight typed failures (`exception`, `timeout`, `abort`, `worker-exit`, `invalid-output`, `output-limit`, `protocol`, `sandbox-unavailable`), returned as a field of a resolved result [SRC `ptc-runtime-node/src/index.ts:55-343`, `ptc-runtime/src/types.ts:121-162`, `#140`, `#139`]. An experimental private Python provider uses fd-3 JSON lines, rebuilds every inbound frame field by field, sets `RLIMIT_AS` and has no file confinement [DOC, `#141`]. The Python SDK and PTC Python rendering exist; only the TypeScript provider is shipped.

### 5.4 Other extensibility surfaces

- **MCP client** (stdio or Streamable HTTP): tools exposed as `mcp__<server>__<tool>` limited to 64 characters of `[A-Za-z0-9_-]` with a 12-hex SHA-256 identity suffix when normalization changes the name; two-phase refresh (build the next generation, swap on success); per-scope server-name reservation; reconnect 500 ms to 30 s, 10 attempts; instructions capped at 32 KiB; stdio children receive the scrubbed environment; only PNG/JPEG/WebP/GIF results are bridged into durable context [SRC `mcp-client/src/tools.ts:81`, `connection.ts:41-49`, `#058`].
- **Skills**: `SKILL.md` directories discovered from project, custom, user and bundled roots; registry merges providers by numeric rank (lower wins), kebab-case names, per-skill `modelInvocable`/`userInvocable`; the model sees a catalog and loads bodies on demand [SRC registry, DOC catalog, `#059`].
- **Plugin manager**: transactional install (snapshot `package.json` and lockfile, restore on failure), live apply through HMR, guided install, official on-demand bundles pinned to the running version [DOC, `#060`].
- **Creator mode**: the `cordis` agent preset gives the model `cordis_inspect_list` and `cordis_inspect_query` to read live Host and Client plugin APIs, then installs authored bundles through the plugin manager; no model tool creates process-local dynamic definitions [SRC preset, DOC runner, `#060`, sibling `T108`].
- **Compatibility bridges**: Claude Code and Codex command hooks, `AGENTS.md`/`CLAUDE.md`, `SKILL.md`, MCP servers, and Claude Code "mods" run unchanged (all experimental) [DOC, `#062`].
- **Provider request extensions**: plugins add additive `dsh_*` top-level fields to official DeepSeek requests in a prepare then accept-after-2xx transaction [SRC, `#157`].
- **SDK and ACP**: the SDK wire is tiny (six requests, four notifications) and streams raw log events; ACP is automation-only [SRC `packages/sdk/protocol/src/types.ts:88-146`, `#061`].

---

## 6. Context management and caching (the part to study hardest)

DSH treats **prefix-cache stability as a first-class invariant**. The design choices that serve it, each with its mechanism:

| Invariant | Mechanism | Record |
|---|---|---|
| Deterministic prompt bytes | Sections sort by numeric order then code-unit name (locale independent); tools sort lexicographically or by a configured `toolOrder` with a rest marker | `#063` |
| Stable and dynamic text separated | Stable sections are the system prompt; dynamic facts (cwd, sandbox, approval, delegation, time) are a runtime-context user snapshot appended **only when it differs** from the retained one | `#125` |
| Prompt changes do not rewrite history | On `in-history` routes a changed prompt is appended as a later system message in a continuing series; otherwise consolidated at node 0 on a new series | `#037` |
| Tool changes do not rewrite the tool list | Additions and removals announced by a developer message; DeepSeek `tool_addition`/`tool_removal` blocks and deferred loading | `#038` |
| Plan mode does not change tools | Tool catalog stays fixed; plan rules are prompt text that overrides tool descriptions | `#028` |
| Fork keeps route and tool definitions | `subagent_fork` omits route selection; parent and child messaging definitions match byte for byte | `#021` |
| Summarizer reuses the warm prefix | The compaction instruction is the final user message after the replayed system prompt, tools and shadowed span; call tagged `purpose:'compaction'` | `#013` |
| Images are deterministic | One request variant per route, quality ladder, 14 px patch grid, exact local token calculator; removal in 64 MiB / 10 MiB / 20-image quanta | `#041`, `#015` |
| Tool-arg order is engineered | `description` before `command` in schemas plus a prompt sentence, measured on 310 requests | `#050` |
| Time context is throttled | Clock reading appended at most every 10 minutes by default | `#018` |
| Skill catalog changes are explicit | Whole-list replacement message only when names or descriptions change | `#059` |

**Compaction**: threshold = `floor(min(0.8 x window, window - output - 65,536))`, retain 16% of the message budget, per-route policies, summarizer route configurable; proactive on `agent/pre-step`, reactive on `CONTEXT_WINDOW_EXCEEDED` with one maximal pass and retry. The transaction is a log bracket (`compaction/start`, summary, replacement `user/message`, `compaction/end`) with whole-surface or selected-span stability revalidation and a refusal to land a summary that does not shrink its source; the checkpoint has eight sections and merges any earlier checkpoint instead of copying it [SRC `compaction-basic/src/config.ts:150-210`, `region.ts`, `summarizer.ts:32-70`, `#011`-`#013`]. Before summarizing, a model-free **pruner** replaces over-8,192-code-point tool results with 4,096 head, marker and 1,024 tail through logged replacement events, each preceded by a shadow-price event so meters subtract it [SRC, `#014`]. Oversized tool output spills to private 0700 files above 12,500 inline tokens (spill is best-effort) [SRC, `#016`].

**Token meter**: replay-deterministic fold; provider usage anchors a measurement only when the request envelope matches and the total is at least the route-priced estimate; otherwise `ceil(chars/4)` plus 4 per block and 4 per role frame; images priced by the adapter, files by their handle text [SRC `token-meter/src/estimate.ts:13`, `index.ts:52-71`, `#017`]. The 4 chars per token heuristic underprices CJK and JSON (documented limitation).

**Other context sources**: workspace `AGENTS.md`/`CLAUDE.md` chain (65,536-byte budget, touch-driven refresh through file tools, content-duplicate suppression) [SRC, `#019`]; session references (opt-in, immutable, marked untrusted) [DOC, `#082`]; optional tmux location and time context [DOC]. A proposed "recallable compaction" design (index stubs plus a `history_read` tool) is not implemented [DOC, `#116`].

---

## 7. Provider handling (category 9)

- **Seam**: `LlmRuntime` is an adapter registry plus `stream()` interceptable by the `llm/stream` waterfall. `prepareCall(config, signal)` resolves exact model metadata (window, output default, efforts, modalities, `systemPromptUpdate`, `toolUpdate`, retry policy) bound to one adapter generation and returns a frozen one-shot call; dispatch must match the prepared config, so hot configuration changes cannot split a request across generations [SRC `llm/src/index.ts:169-340`, `#036`].
- **Two adapter families**: a native DeepSeek Messages-compatible adapter (direct fetch and SSE, 1,000,000-token default window, 256,000 default output cap, 300 s stream idle timeout, Files API pipeline for images) and a `pi-ai` multi-provider twin mounted dormant until a settings section supplies routes (OpenAI-completions, OpenAI-responses and Anthropic-messages protocols, catalog providers, endpoint discovery, OAuth login code present; docs stale) [SRC/DOC `#035`, `#047`-`#049`]. A local patch removes eager partial-JSON re-parsing of every tool-call delta in six `pi-ai` adapters because it was O(n^2) on a single event loop [SRC `patches/@earendil-works__pi-ai@1.0.2.patch`, `#105`].
- **Capabilities**: reasoning effort per model validated before I/O; replay state (thinking signatures) valid only for the same adapter and model; usage buckets disjoint (input, output, cache read, cache write, reasoning); purpose tags (`compaction`, `session-title`) become transport metadata; non-suppressible `user-agent` attribution; additive request-extension fields; a provider-neutral failure payload (`LlmFailure`) with stable codes, retry-after and request id [SRC, `#039`, `#040`, `#043`, `#044`, `#126`, `#136`, `#157`].
- **Model switch** mid-session appends a durable notice "[model changed: ...]" and snapshots the route per step [SRC `agent/src/model-selection.ts:47-125`, `#134`].
- **Deliberate gaps**: no `tool_choice`, no `response_format`, no cross-provider failover, no currency price tables, no batch or prompt-cache control vocabulary beyond provider-specific behaviour [DOC/SRC, `#046`, `#043`, `#008`].

---

## 8. Computer and environment control (category 6)

- **Execution world**: `fs` and `subprocess` together define one world shared by bash, PTY terminals, LSP and PTC; a remote provider owns only files and processes while the Host keeps the loop and log [SRC/DOC, `#070`, `#159`].
- **Subprocess seam**: no implicit defaults (every stdio mode, cwd, grace and env stated), scrubbed parent environment, bounded tail with spill and cursor-free offset reads, managed process ranges (systemd user scopes, Job Objects, process-group fallback) terminated and awaited as one unit, fd-7 control channel [SRC, `#138`, `#139`, `#069`].
- **Sandbox**: per-call policy `read-only | workspace-write | danger-full-access` (file effects only; no network or syscall policy); `confine(argv, policy)` returns the wrapped argv, an enforcement level and the backend's denial dialect and exit-gated runner-failure rules; chain Linux bwrap then Landlock, macOS Seatbelt, Windows restricted-token ACL (partial); unavailable confinement fails closed with `SANDBOX_UNAVAILABLE` [SRC `sandbox-local/src/index.ts:69-243`, `#066`, `#068`]. The Landlock launcher is a ~300-line static musl C program: `--ro`/`--rw` allow-lists, `no_new_privs`, ABI negotiation up to 5, exit 125 reserved for launcher failure, a `--probe` mode that actually enforces [SRC `native/system/packages/entry/src/main.c`, `#067`].
- **Terminals**: persistent PTY shell with a controlled prompt marker, foreground process-group inspection and three readiness tiers [SRC, `#074`]. **Jobs**: owner-fenced registry with output ring, model cursor and observer offsets [SRC, `#025`]. **Working directory** is logged state separate from the immutable workspace origin [SRC, `#146`]. **LSP** navigation (4 operations) is opt-in and not in any shipped bundle [DOC, `#147`]. **SSH** helper with versioned RPC and TLS-PSK streams [SRC, `#159`]. **Browser and computer use** (Playwright MCP, Chrome DevTools MCP, Stagehand, Cua driver) are experimental and off by default; there is no per-domain grant model between model and browser beyond approvals [DOC, `#075`].
- **File tools**: literal `edit` (unique match, version compare-and-set through an intent waterfall), `write`, `read`, `read_image` (route-gated), `glob`/`grep` (bundled ripgrep), optional `str_replace_editor`; read-before-edit enforced by an observation policy keyed weakly by session [SRC, `#072`, `#079`, `#162`].

---

## 9. Platform plumbing worth knowing (category 13)

- **Typert RPC**: `@Remote` methods are analyzed from the host `ts.Program` and generate strict descriptors, codecs and typed client methods with declaration maps; lookup providers turn wire ids into live Host objects; streams multiplex over `/api/remote.mux` with half-close, uplink and cancellation; byte results travel as multipart attachments instead of base64 [DOC/SRC, `#098`, `#144`].
- **UI as a hook participant**: 27 allowlisted Host events are forwarded (25 emits, 2 waterfalls: `approval/request`, `user-questions/request`). The Gateway offers every scoped waterfall to every connected client; the first `result` wins and cancel frames retract the rest; `next` from all clients lets the Host chain continue; a request raised while no client is attached stays pending and is replayed to the next client. Emits are broadcast to every client [SRC `api/remotes/src/remote-events.ts:20`, `api/gateway/src/index.ts:475-668`, `#142`, `#099`].
- **Client**: a Cordis application in the browser with typed slots, a lazy module table and combo bundle URLs; connection generations become visible only after the Host attaches listeners and sends `ready`; session `follow()` sends a baseline then increments with gap repair [DOC, `#099`, `#100`].
- **State outside the log**: a storage hub of schema-validated KV domains over JSON or SQLite backends (schedules, workspaces, projection cache) [DOC, `#154`]; attachments content-addressed under `DSH_HOME` [DOC, `#122`]; credentials by reference with write-only UI [DOC, `#085`].
- **Exposure**: concrete bind address only (no wildcards), own bind IP trusted as Host, optional native TLS with scheme-bound cookies, launch token plus Host/Origin fence [SRC/DOC, `#150`, sibling `T056`].
- **Workspaces**: projects are ordered directories with sessions; archive asks every owner (agent turn, jobs, subagent descendants, schedule reminders) what still runs and stops them without waking later [SRC, `#156`].
- **Desktop**: Electron around the full web host, task-aware quit, update journal, crash reports [DOC/SRC, `#102`, `#149`].

---

## 10. Testing, evaluation and engineering governance (category 4 "evals", 13)

- **Tiers**: per-file 100% line coverage on `packages/*/*/src`; real-API e2e that self-skips without keys; keyless **recorded-session snapshot** scenarios (227: 136 session, 59 web, 26 SDK, 10 ACP) replayed through the real shipped profiles by `llm-replay` with a `replay.override.json` sidecar for failures the log cannot encode; a scriptable mock Messages server with fault injection and a seeded stress mode; user-path **performance budgets** (reconnect, agent continuation, conversation fold, long session, session open, corpus listing) run in fresh child processes under plain Node with source-constant budgets and memory-capped reruns [DOC/SRC, `#111`-`#114`].
- **Rules**: mock only the expensive or nondeterministic boundary; verify the world (re-run the command, re-read the file) not the agent's self-report; test the published entry path (built `lib`, not tsx); every registry gets an HMR-safety test [DOC, `#113`].
- **No agent-quality benchmark ships**: `BENCHMARK.md` only tells external harnesses to use the Python SDK and the `sdk-minimal` profile (shell-only, no runtime context) as a control arm [DOC, `#115`].
- **Architecture fitness gates** are AST-level scripts: no bare undici dispatcher (proxy bypass), restricted `appendPluginRecord` callers, an `as unknown` ratchet baseline, default-product isolation, runtime closure of shipped presets, product-use reachability, mandatory limitations sections, plus generated and freshness-gated catalogs (tool, config, CLI help, persistence, module graph) [SRC, `#143`, `#093`].
- **Process**: about 2,450 decision notes (implemented, archived, proposed, rejected), four postmortems, and an ownership-weighted PR approval check computed from `git blame` [DOC, `#110`, `#148`].

---

## 11. Focus items called out in the brief

**native/**: `native/system` (`@deepseek-ai/node-addon-system`) ships `landlock-run` and a Node-API v8 `flock` binding as per-platform prebuilds (darwin and linux, x64 and arm64; Alpine tested for musl; **no Windows prebuild**, Windows uses koffi FFI and Job Objects). Installs never compile; a prepack check and an offline packed-install rehearsal verify the bytes. `main.c` is fail-closed by design (cannot open a rule path, ruleset not enforced, or exec failure all exit 125) [SRC, `#067`, `#104`].

**python/**: `python/sdk` is a synchronous, thread-based JSON-RPC client over stdio that launches `dsh --profile sdk`; it **never uses `~/.dsh` implicitly** (explicit `DSH_HOME` required), filters notifications to a session tree using `subagent.started` edges, and `Session.run` waits for the `agent/inbox/spliced` receipt of its own message before collecting events until the whole agent is idle. `python/sdk-runtime` builds platform wheels carrying the runtime executable. Versions are the placeholder `0.0.0.dev0` in the tree [SRC, `#061`, `#103`].

**patches/**: seven pnpm patches: pi-ai (stop O(n^2) re-parse of streamed tool arguments), node-pty (embedded-runtime helper path via `DSH_NODE_PTY_SPAWN_HELPER`, ConPTY drain fix), `@yao-pkg/pkg` (about 400 KB, SEA/packaging), exceljs (OOXML prefix tolerance), two fortune-sheet patches, osx-sign (`lstat` instead of `stat`) [SRC for pi-ai, node-pty and osx-sign read in full; others by size and DOC, `#105`].

**vendor/**: Cordis and eight helper packages vendored under the `@deepseek-ai` scope with 23 numbered local modifications, notably fiber lifecycle hardening (reentrant disposal gaps), lazy config resolution, **volatile config references** (hot-applied settings without remount), durable debounced config writes and Node-loader shape detection [SRC `vendor/README.md`, `#105`, `#120`].

**snapshots/**: 600 recorded session logs under `acp/`, `sdk/`, `session/`, `web/`; a top-level scenario's highest parent generation supplies user input, model replay and expected persisted result; mutating scenarios compare a `workspace.expected/` tree that record and refresh never rewrite [DOC, `#111`].

---

## 12. Assessment

### 12.1 Best in class (relative to peers I can compare from the other audits)

1. **Event-sourced loop with model-visible equals logged** and replacement-only history edits (`#090`, `#094`, `#012`). Replay, fork, audit and compaction safety come from one mechanism.
2. **Crash and cancel repair semantics** (`#004`, `#005`, `#129`): preserved visible prefix, explicit unknown-outcome results, repair through the log.
3. **Prefix-cache engineering** across prompt, tools, forks, compaction and images (section 6).
4. **Tool pipeline** with monotonic deny-only guards, around-dispatch cancellation fusion and a mandatory typed output contract (`#056`, `#131`).
5. **Honest sandbox reporting** and a documented denial/failure classification (`#066`, `#067`).
6. **Recorded-session replay corpus** plus verify-the-world testing (`#111`, `#113`).
7. **AST-level fitness gates** and generated, freshness-gated docs (`#143`, `#093`).
8. **UI as an ordinary hook listener** for approvals and questions (`#142`).

### 12.2 Weaknesses and limits (what DSH does not do or does poorly)

- **Single-operator, single-process**: no tenant, role, org, ZDR or per-user addressing; every connected browser receives every event and every pending approval (`#142`); jobs, terminals and scheduling die with the process [SRC/DOC].
- **Plugins have full Host authority**; `SAFETY.md` says the project is unaudited and not production-ready; the `node:vm` realm for dynamic definitions is not a security boundary (`#089`).
- **Sandbox covers file effects only** (no network, process or credential policy), Windows is partial, and denial detection is stderr text matching (`#066`).
- **Default-on upload** of the full session log (up to 8 MiB per request, enabled `true` by default) and plugin inventory to the official API, superseding an earlier opt-in (`#083`, verified in `session-log-deepseek/src/index.ts:52`).
- **Neutral request vocabulary is minimal**: no `tool_choice`, `response_format`, failover or currency cost (`#046`, `#043`).
- **No product features beyond text agents**: no image generation or editing, TTS or realtime voice, deep research, shareable links, conversation import, learned memory, connectors beyond MCP, mobile or device pairing (scout hand-off, INF).
- **Heavy process and framework risk**: one upstream DI framework vendored with 23 local fixes; 195 root scripts; a 4-phase build; a React/zustand UI that Verevon's SolidJS shell cannot reuse (`#105`, `#158`).
- **Approvals are one-shot only**: no persistent grants, no argument inspection by the answerer, no actor identity in the audit pair (`#086`, sibling `T049`).

### 12.3 Quality notes on records

Records tagged quality 5 (27) are the engine mechanisms above. Quality 2 records are experimental or proposed designs (agent teams, Ralph, webhooks, recallable compaction) and the explicit limitation records.

---

## 13. What Verevon should copy, adapt or avoid

Mapped to Verevon's Model Plane and the chat and agent pages. Rust-portability noted where it changes the shape.

**Copy (high value, low regret)**
1. **Log-first run model**: append-only typed events, a derived model-visible surface, replacement ops with `sourceEventSeqs`, deep-frozen derived requests, `request/header` series reasons. In Rust: enum-per-event with exhaustive `match`, newtype ids, `Arc<[Message]>` snapshots (`#094`, `#003`, `#109`).
2. **Repair-through-the-log**: three synthetic tool results with the "unknown outcome, do not retry blindly" wording, a preserved cancelled prefix, and resume ordering (lease, read, append closers, publish) (`#004`, `#005`, `#129`).
3. **Cache-stability invariants** as a written checklist and tests: deterministic ordering, runtime-context snapshots only on change, additive tool-change messages, fork keeps route and tools, summarizer reuses the live prefix and puts the instruction last, tag auxiliary calls by purpose (`#037`, `#038`, `#013`, `#021`, `#044`).
4. **Compaction as a log transaction** with whole-surface stability revalidation, no-shrink refusal, shadow-price events and a model-free pruner tier; threshold formula that reserves output and headroom before the ratio (`#011`-`#014`).
5. **Tool contract**: mandatory output schema and pure render, monotonic deny-only guards, fused cancellation, `concludeTurn`, `deferContext`, registry-enforced PTC collapse (`#131`-`#133`, `#056`).
6. **Code-execution runner design**: dedicated control fd, hostile-frame rebuild, strictly sequential call ids, explicit ceilings, typed failure taxonomy, sandbox facts in every result (`#139`, `#140`).
7. **Sandbox honesty**: per-run `full|partial` enforcement, denial vs runner-failure classification, fail-closed on unavailable confinement (`#066`).
8. **Eval assets**: recorded-session replay with a failure-injection sidecar, a fault-injecting mock provider, user-path performance budgets, "verify the world" assertions (`#111`-`#114`).
9. **Fitness gates**: encode Verevon's cross-plane rules (no cross-plane DB access, ZDR propagation, Space ACL never widening an owner ACL, "Model proposes, Quarry executes") as AST or dependency-graph checks with baseline ratchets (`#143`).
10. **Human-in-the-loop as awaited event waterfalls** with a fail-closed default and replay of pending requests to reconnecting clients, but addressed per user and per run (`#142`).

**Adapt**
- **Approvals**: add persistent scoped grants with expiry, argument visibility and actor identity; keep the closed outcome vocabulary and `never` policy (`#086`).
- **Permission bundles**: express Space and org policy as named bundles over independent enforcement knobs, logged as events (sibling `T046`).
- **Web seam and search tool**: normalized `WebSearchResult` plus a multi-query tier with citation instruction, but add iterative deep research (`#045`, `#161`).
- **Request extensions** for billing, audit and ZDR flags on outbound inference (`#157`), keeping the accept-after-2xx at-least-once semantics.
- **Skill and MCP compatibility**: accept `SKILL.md` and stdio/HTTP MCP unchanged, with per-scope attachment mapped to Spaces (`#058`, `#059`).
- **Workspace archive handshake** for chats that own agent runs and jobs (`#156`).
- **Reasoning translation** for multilingual (for example Norwegian) users with durable, tenant-scoped records and explicit disclosure (`#155`).

**Avoid**
- Default-on log upload to a model vendor (`#083`); Verevon's ZDR rule requires opt-in, tenant-controlled export.
- Broadcast of every event and pending request to all clients (`#142`); whole-config replacement semantics for overrides (`#053`); one-shot-only approvals; full-authority third-party plugins (`#089`); in-process dynamic code realms as a security boundary; reliance on process-local jobs and terminals.

**Do not port**: the Cordis DI framework or the React/zustand/Lexical client. Keep the ideas (registrations return disposers; services by key; waterfall middleware) as Rust traits plus RAII guards and a plane-owned event contract.

---

## 14. Corrections, upgrades and resolved open items

Corrections to pass 1: `#013` nine to eight sections; `#002` claim wording. Upgrades from DOC to SRC after reading code: `#006`, `#010`, `#014`, `#017`, `#019`, `#025`, `#026`, `#031`, `#058`, `#059`, `#063`, `#074`, `#081`, `#071`. Evidence added without status change: `#005`, `#016`, `#045`, `#060`, `#061`, `#066`, `#099`, `#109`.

Resolved from the scout's open list:
- **Mux broadcast**: confirmed, emits go to every client generation and scoped waterfalls are offered to every client (`api/gateway/src/index.ts:539-633`).
- **Approval "chain slot"**: it is the UI's own `ctx.remote.$on('approval/request', ...)` listener, which answers or delegates (`client/ui-approval/src/client/index.ts:104`).
- **Approval with no browser attached**: stays pending and is replayed to the next client (not fail-closed in the web profile); headless profiles use policy `never` or have no answerer, which yields `unavailable`.
- **Event count**: 60 types in 33 families at HEAD.
- **Default-on upload**: confirmed in source.
- **Python PTC provider**: confirmed to exist as a private experimental package using fd 3; it is not in the shipped profiles, so the shipped programmatic mode is TypeScript only.

---

## 15. Open questions and limits of this audit

1. **No runtime observation.** Nothing was executed; throughput, latency, memory and the behaviour of the replay corpus were not measured.
2. **Prior-analysis claims not reproduced**: the keyed projection channel with higher-sequence-wins in the client store; unauthenticated plugin manifest route; `session.history` creating an agent (the resolver can resume cold sessions for some calls, while the workspace-file scope lookup deliberately never activates an agent; which Remote methods do which was not enumerated).
3. **Experimental packages** (44) were read mostly through READMEs; the Python PTC provider, browser-use and computer-use drivers, Ralph and webhook packages were not read in source.
4. **OAuth sign-in**: `login.ts` builds OAuth and interactive-key methods for pi-ai providers while `docs/user/guide/providers.md` says OAuth providers are unsupported in the Web form; whether the Models page surfaces them was not verified (`#048`).
5. **Which preset new sessions default to** (standard vs cordis) was not verified; `standard` is order-first in the shipped presets (`#054`).
6. **Windows and macOS behaviour** of sandboxes and terminals could not be tested from this host.
7. **Absence claims** (no failover, no cost tables, no image generation) come from the scout's keyword sweeps and tool catalog and are tagged INF where used.
8. **Cross-facet overlap**: nine drafts were dropped as duplicates of pass-1 or sibling records; residual overlap with the sibling facet exists where the same feature is described from the user-visible side (for example permission presets, session titles, export).

---

## 16. Record index by category

Counts after this pass: Agent runtime 45, Platform architecture 28, Tools/plugins/extensibility 22, Models and routing 21, Computer and environment control 18, Safety/permissions/governance 10, Transparency 4, Chat experience 4, Coding capability 4, Multimodal and creation 3, Research and web 2, Memory/learning/personalization 1. Total 162. Provenance: SRC 115, DOC 45, INF 2. Quality distribution: 5 (27), 4 (79), 3 (44), 2 (12). Uniqueness: unique 43, uncommon 85, common 34.

New records in this pass (ids 127-162): loop phase machine, turn-end vocabulary, cold-resume ordering, scoped lifecycle teardown, mandatory output contract, deferred context, PTC collapse, model-switch notice, closed role map, attribution, untrusted web label, subprocess seam, fd-7 control channel, PTC host-child protocol, Python PTC provider, UI as hook participant, AST fitness gates, binary RPC results, workspace file authority, working directory, LSP tool, weighted PR approval, fail-loud policy, web exposure rules, runtime resolution generations, blank-session lock arbitration, migration-aware reads, storage hub, reasoning translation, workspace registry and archive admission, request-extension registry, dual-face build, SSH transport, web_search tool, read_image tool, edit tool.
