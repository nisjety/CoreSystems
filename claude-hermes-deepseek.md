# Model Plane vs. Claude Code, DeepSeek Harness, and Hermes Agent

**A harness-capability audit, feature-parity matrix, and adoption plan.**

Date: 2026-08-16. Scope: `apps/Model Plane` (this repo) vs. three external agent
harnesses cloned locally at `/Volumes/Lagring/Triodelab/{deepseek-harness,hermes-agent,claude-code-fork}`.

> **This is not the first pass at this question.** Model Plane already has a
> license-gated external-ideas-harvest (`docs/external-ideas-harvest.md`, mined
> 2026-05-30 from `codex`/`hermes-agent`/`pi`/`daytona`/`claude-code`) and its own
> `GOAL.md` already names `claude-code-fork` and `hermes-agent` as parity/idea
> targets. This document **updates and deepens** that prior work — it adds
> `deepseek-harness` (not previously analyzed), corrects one factual error in the
> prior harvest, verifies which prior gaps have since closed, and goes to actual
> code (not just directory names) for all three external systems. Treat this as
> a supplement to `external-ideas-harvest.md` and `capability-ownership-matrix.md`,
> not a replacement.

---

## 0. TL;DR

- **Model Plane's biggest historical gap — real process sandboxing — has
  substantially closed since the last audit (2026-07-17).** `execution-core` now
  has real `bwrap` (bubblewrap) + Landlock/seccomp isolation
  (`rust/services/execution-core/src/{sandbox.rs,executor.rs}`), the same
  primitive `deepseek-harness` uses and the one Model Plane's own harvest plan
  recommended vendoring from `codex`. This was not yet true as of the last
  written audit — update `gap-analysis.md`/`GOAL.md` accordingly.
- **CORRECTION (2026-08-25): the four gaps this TL;DR used to name are closed, and
  three of them were never real.** This bullet previously read "still genuinely
  missing: parallel tool-call dispatch, a populated tools/plugins/MCP registry,
  a real long-term memory backend, and any compaction-quality eval harness."
  Every one of those is now ✅ or CORRECTED in §3, most as of 2026-08-22 — and
  the registry and memory claims were **wrong when written**, not merely
  outdated: they inferred absence from the absence of an expected *type name*
  (see §3's warning box). Parallel dispatch shipped 2026-08-22; the
  compaction-recall harness shipped the same day. The TL;DR sat stale for three
  days while the matrix beneath it was correct, which is exactly the failure
  `CLAUDE.md` warns about for `ROADMAP.md`, recurring in the newer document.
- **What is *actually* the dominant gap, measured 2026-08-25: reachability, not
  absence.** A 22-claim adversarial verification of the adoption scorecard —
  each claim traced from advertisement to production caller — found **8 claims
  where the code exists, is tested, and cannot activate in production**:
  - the **context inspector** never returns data: session-core's
    `authorize_run_owner` runs *before* `get_context_assembly_inner` and has no
    empty-string guard, so every production request is rejected first;
  - the **extended-thinking dial** has zero production writers, so no request
    ever carries a budget;
  - the **per-call autonomy gate can never refuse anything** — only one
    non-test constructor of `RunAgentRequest` sets the rung;
  - `ProviderError::TooLong` is constructed at exactly one site and **nothing
    propagates it**;
  - the mid-run **queued-input** fix is real server-side while the SPA composer
    still returns silently during a stream, so the silent-drop bug it exists to
    fix is *still live in the product*;
  - **`save_memory`/`recall_memory`** are advertised and dispatched yet still
    unreachable; **memory provenance** renders only because nothing writes a
    non-zero origin on the feeding path; and `run_subagent` has **no
    first-party dispatch path at all** because no catalogue advertises a
    `subagent.*` name.
  A ✅ in this document means "the mechanism was built." It has repeatedly not
  meant "a user can reach it." Read every row with that distinction.
- **Where Model Plane now leads rather than follows: tool-argument grounding.**
  None of the four external harnesses checks that a tool argument is *grounded
  in the request*. Measured 2026-08-25, required user-held values were
  fabricated 55–70 % of the time in the agent loop and **100 %** in chat — every
  fabrication schema-valid, because shape validation cannot see an invented
  postal code. A pre-dispatch grounding gate now refuses them in both loops.
  This is a genuine lead and had no row in §3 until now — see the matrix.
- **The single most reusable idea across all three external systems** is the
  same pattern under different names: a swappable capability = one interface +
  one adapter shape that both "real OS resource" and "SDK-only/remote backend"
  implementations satisfy identically (DeepSeek's "capability seam", Hermes's
  `ProcessHandle` Protocol + `BaseEnvironment` ABC, Claude Code's `ToolDef` →
  `buildTool()` factory). Model Plane should adopt this explicitly for its own
  sandbox-backend and memory-adapter abstractions (§7.1).
- **Model Plane already has some genuinely smart engineering of its own** worth
  preserving, not just importing — most notably the 2026-08-14 decision to
  *reject* unifying its two tool-dispatch loops after measuring that the
  separation is the actual authority boundary (§6).
- **Honest read on maturity ordering for the *harness* concern specifically**
  (not the product surface as a whole): DeepSeek Harness's engineering-process
  discipline (generated/verified docs, blameless postmortems, ADR ledger) is the
  strongest of the four; Claude Code's tool/permission/sandbox/compaction design
  is the most battle-tested (it is what has actually shipped to the most users);
  Hermes has the richest plugin/backend ecosystem and the most honest security
  posture documentation (including admitting its own defaults are weak); Model
  Plane has the strongest production-readiness *discipline* (evidence states,
  coverage gates, signed release artifacts). **The "least mature harness feature
  surface of the four" clause that used to end this line is retired as of
  2026-08-25**: the 25-item adoption plan closed, and the surface is now broadly
  comparable — 22 of 34 matrix capabilities ✅, with two of the five ❌ being
  documented decisions the reference harness itself agrees with. What separates
  Model Plane from the other three is no longer feature count but the
  build-to-reachable gap described above: it ships mechanisms faster than it
  wires them to users.

---

## 1. Methodology & constraints

Each external system was explored by an independent research pass (direct
`Read`/`Grep`/`Bash` reads of source and docs, not just READMEs), cross-checked
against Model Plane's own prior research where it existed. Three constraints
were binding throughout and are enforced in this document:

1. **License gate (inherited from `external-ideas-harvest.md` §1, applied here
   too):** `deepseek-harness` is MIT. `hermes-agent` is MIT. `claude-code-fork`
   is **leaked, unlicensed, proprietary** — Anthropic's actual Claude Code CLI
   source, exposed via an npm sourcemap and squashed into one third-party
   commit by whoever archived it locally. Nothing in this document quotes or
   reproduces source code, comments, or prose from `claude-code-fork`; it is
   described structurally (file/directory names, control-flow shape, exported
   symbol names, line/byte counts) exactly as the existing harvest's own rule
   requires ("shapes only, ZERO code"). The same discipline was applied to the
   MIT-licensed repos even though it wasn't strictly required, for consistency.
2. **Verify, don't trust docs.** Every claim about Model Plane's *current* state
   was checked against source and recent git history as of 2026-08-16, not
   against the dated docs alone (`docs/core-research/*` and `MODEL_PLANE_STATUS.md`
   are from 2026-07-16/17; 153 commits have landed in `apps/Model Plane` since).
   Where a documented gap turned out to be closed (or vice versa), this is
   called out explicitly rather than silently repeating the old doc.
3. **One correction to the prior harvest's factual record:** `external-ideas-harvest.md`
   describes `hermes-agent`'s `background_review` as feeding "Wave 7 fine-tuning."
   Fresh code reading does not support this — `background_review.py`/`curator.py`
   (the actual online self-improvement loop) only ever write to local memory/skill
   files via a tool-whitelisted forked agent; the only dataset-generation code in
   the repo is `batch_runner.py`, an unrelated offline harness with no code path
   connecting it to the per-turn loop. If Model Plane's P7 learning-loop item
   (`external-ideas-harvest.md` remediation #12) is scoped assuming these are one
   system, rescope it — they are two.

---

## 2. System profiles

### 2.1 Model Plane (this repo)
Rust + Go agent platform, from-scratch rebuild. Rust owns the hot path (invoke,
streaming, session assembly, execution loop, provider routing, multimodal); Go
owns durable control (workflows, registries, policy, scheduling). North star
(`docs/GOAL.md`): product-surface parity with `claude-code-fork`, selectively
incorporating ideas from `hermes-agent`, `openai/codex`, and several
knowledge/memory projects. 153 commits landed in the 30 days before this audit;
production-readiness discipline is unusually rigorous (signed release artifacts,
evidence-state tracking, risk-based coverage targets, explicit "not
production-ready" self-labeling in `MODEL_PLANE_STATUS.md`).

### 2.2 Claude Code CLI (`claude-code-fork`, leaked source, 2026-03-31)
**Not a community fork** — a one-time unofficial leak of Anthropic's actual
proprietary CLI, exposed via an npm sourcemap and archived as a single squashed
commit with no license, no build manifest, and no test files. TypeScript on
Bun, React/Ink terminal UI, Zod v4, `@anthropic-ai/sdk`. ~30 top-level
directories organized by concern (`tools/`, `commands/`, `services/`, `utils/`
— the largest single directory at 564 files). Read-only reference material,
never a project to build or extend.

### 2.3 DeepSeek Harness (`dsh`)
DeepSeek AI's open-source (MIT) "everything is a plugin" coding-agent CLI,
developer preview. ~430K lines of TypeScript across 226 pnpm workspace
packages, built on Cordis, a vendored DI/typed-event framework. The most
process-disciplined of the four external/internal systems compared here:
generated-and-verified documentation, a 688-entry Agent Notes ADR ledger, four
numbered blameless postmortems.

### 2.4 Hermes Agent (`hermes-agent`)
Nous Research's open-source (MIT) personal AI agent. Python/`uv`-managed,
single-process multi-threaded runtime. One agent core driven through a CLI, a
~20-platform messaging gateway, a TUI, and an Electron desktop app; almost all
capability growth happens through 21 first-class plugin packages, categorized
skill packs, and 8 pluggable sandbox/memory backends each. Ships a bundled
OAuth billing gateway (Nous Portal) covering 300+ models plus web
search/image-gen/TTS/cloud-browser under one subscription.

---

## 3. Feature-parity matrix

> ### ⚠️ 2026-08-22 — five rows were materially WRONG, all from one flawed method
>
> A gap-closure pass re-verified every remaining ❌/🟡 against source before
> building against it. **Five capabilities this matrix called absent or stubbed
> turned out to be real, shipped, and in some cases hardened**: the plugin
> registry, the MCP registry (1,781 lines with an OAuth vault and SSRF guards),
> voice I/O (a 1,073-line speech provider), long-term memory (three live
> backends, already wired into the turn), and the memory-adapter seam.
>
> **The root cause is one repeated mistake, not five unlucky ones:** the
> original pass inferred absence from *the absence of an expected type name*
> — it grepped for `ToolsRegistry`/`PluginsRegistry`/`McpRegistry`-shaped types,
> found none, and wrote "stub." Model Plane's actual design is ONE `Capability`
> registry with a `Kind` discriminator (`KindPlugin`, `KindMCPServer`,
> `KindTool`, …), so those type names *should not* exist. Same pattern for
> `letta-bridge`: no `MemoryAdapter` trait, therefore assumed "in-memory stub" —
> when a `Store` interface with three implementations was sitting right there.
>
> **Read the rest of this document with that bias in mind.** A ❌ here means
> "the 2026-08-16/17 pass did not find it," which is not the same as "it does
> not exist." Every row acted on since has been re-verified against source
> first; rows not yet re-verified are still suspect. This is the same
> "correcting stale ❌/✅ claims" failure mode `CLAUDE.md` already warns about
> for `ROADMAP.md` — it recurred here, in the newer document.
>
> Genuinely absent after re-verification: **filesystem worktree isolation** and
> a **generated-docs freshness CI gate** (CI itself exists — `.github/workflows`
> has per-service pipelines; the docs-diffing gate specifically does not).

Legend: ✅ real & live · 🟡 partial/stub/opt-in · ❌ absent · — not applicable

| Capability | Model Plane | Claude Code (leaked) | DeepSeek Harness | Hermes Agent |
|---|---|---|---|---|
| Real bounded turn/tool-call loop | ✅ `execution-core` runtime_loop | ✅ `QueryEngine.submitMessage` | ✅ `core/agent-loop` | ✅ `conversation_loop.py` |
| Streaming responses | ✅ SSE `/v1/invoke/stream` | ✅ | ✅ `llm/stream` waterfall | ✅ |
| Extended-thinking / reasoning budget | ✅ **a real budget, and now with a writer** (2026-08-26) — `thinking.rs` maps the effort dial to concrete budgets (deep 4096, quick 1024) and steps down to keep `MIN_ANSWER_HEADROOM_TOKENS` for the answer, then `InferRequest.thinking_budget_tokens` → provider `thinking` block → `reasoning_delta` → the SPA's Innsikt popover. The 2026-08-25 verification found the whole READ path live and **zero production writers** — the dial existed, nothing set it. The writer existed too, unrecognised: the composer's Auto/Raskt-svar/Dyp-research selector rode the payload as `responseMode` with no reader (its "deep" arm even pushed a `"reason"` tool tag nothing consumed). Now mapped to the wire's `effort`. Not yet proven live end to end — needs one authenticated turn | ✅ configurable budgets | — (not a v1-style "thinking" mode) | — |
| **Parallel tool-call dispatch within a round** | ✅ **now live** (2026-08-22) — `run_rounds`'s 3-phase dispatch, `futures::future::join_all`, shared atomic subagent-budget claim (see §7.2) | ✅ (concurrent where safe) | ✅ ordered-or-bounded-concurrent-pool `executionMode` | 🟡 (subagent batches parallel via `ThreadPoolExecutor`; single-turn tool calls not confirmed parallel) |
| Tool-call retry on transient failure | ✅ **now live on BOTH loops** (2026-08-22) — `execution-core/runtime_loop/retry.rs` (gated on `is_risky_call`, so a write whose timeout is ambiguous is never replayed) + `model-gateway/tool_retry.rs` (read-only loop, no gate needed), with `tests/tool_retry_contract.rs` mutation-verified to catch vocabulary drift between them | 🟡 (retry/backoff at the API layer) | ✅ `tools/execute` waterfall wraps timeout/retry/metrics | 🟡 |
| Real OS-level process sandboxing | ✅ **now live** — `bwrap`+Landlock/seccomp (`sandbox.rs`) | ✅ `bwrap` (Linux) + macOS trust-daemon schema | ✅ `bwrap`+Landlock + macOS Seatbelt + Windows ACL runner | 🟡 only via configured `BaseEnvironment` backend; default `local` backend = no isolation |
| Sandbox enforcement reported as full/partial (not assumed) | 🟡 **found, not vocabulary-uniform** (2026-08-22) — `ExecOutcome.sandboxed: bool` is a genuinely probed per-call fact (not trusted), and `AllowDomains` now fails closed without a configured proxy (§7.1); `capability_profile()` exposes the live probe over HTTP but no Go consumer reads it yet | 🟡 (schema implies it, not fully confirmed) | ✅ every backend reports `full`/`partial` explicitly | ❌ |
| Fail-closed sandbox failure | ✅ policy fail-closed | 🟡 (`dangerouslyDisableSandbox` escape hatch, lockable by enterprise config) | ✅ native launcher exits 125 and refuses to exec on any confinement failure | ❌ `tirith_fail_open: True` default; `network_mode: host` shipped default |
| HITL / durable approval gate | ✅ `CreateApproval`, `AwaitingApproval` | ✅ 5+2 permission modes, role-specific handlers | ✅ `ctx.approval` one-shot, default-deny | ✅ pattern-based + auxiliary-LLM auto-approve tier |
| Single shared schema for permission UI + sandbox rules | ❌ not confirmed as unified | ✅ `sandboxTypes.ts` is the single source of truth for both | 🟡 (guards + sandbox policy are separate seams) | ❌ separate scanner + approval + network layers |
| Session/thread persistence | ✅ Postgres, append-only events, transactional outbox | ✅ | ✅ append-only `SessionEvent` log | ✅ (file/DB-backed via `hermes_state.py`) |
| Long-term / semantic memory | 🟡 **[CORRECTED 2026-08-22 — the old "in-memory stub only" claim was wrong]** `letta-bridge` has a real `Store` seam with THREE implementations (`agentmemory` semantic client, `pgstore` Postgres-lexical, `memstore` in-memory fallback), honest degraded-readiness reporting (`DEGRADED_LEXICAL_FALLBACK` / `DEGRADED_SEMANTIC_UNVERIFIED`), memory prefetch wired on the live turn path (`model-gateway/grpc.rs`), and memory read/write exposed as model-callable tools (`tool_loop.rs`). What's genuinely missing vs. Hermes is the LIFECYCLE surface — see §7.9 | ✅ `memdir/` + auto-extraction + team sync | 🟡 read-only cross-session query only, no vector memory shipped | ✅ `MemoryProvider` ABC, 8 real backends |
| Memory-adapter registry (multi-backend) | 🟡 **[CORRECTED 2026-08-22]** the seam exists (`server.Store` interface, backend-kind + semantic-capability metadata) and the deferral's own trigger ("when the second real backend lands") HAS fired — 3 implementations ship today. Missing vs. Hermes's `MemoryManager`: the one-external-provider cap, reserved-core-tool-name guard, and per-durability-class background write draining | — (single `memdir` system) | — (no shipped long-term memory system) | ✅ `MemoryManager`, one-external-provider cap + reserved core tool names |
| Context compaction | ✅ **tiered, and now in both loops (2026-08-24)** — the old "not confirmed tiered" was wrong in both directions: `model-gateway::compaction` has had three tiers (clear stale tool payloads → LLM head summary → hard drop, each leaving a notice the model can act on), while `execution-core`'s governed loop had **none at all** — the loop that accumulates the most payload, where an overflow just ended the run. Tier 1 now runs in both at an identical threshold, spared tail and notice, pinned by `tests/compaction_parity_contract.rs`. Tiers 2–3 remain chat-only by design: they compact a durable thread transcript, which an agentic turn does not have | ✅ 4-tier escalation (time-based → microcompact → provider-native → full LLM) | ✅ capability seam, lock-released-last durability | ✅ 2-owner handoff (native-first, local fallback) |
| Compaction-quality eval harness (recall, not token-count) | ✅ **now live** (2026-08-22) — `eval-lab-py`'s `seed_turns` + `cases/compaction/*.yaml`, see §7.4 | ❌ (not observed) | 🟡 (rigor via postmortems/tests, not a dedicated recall harness) | ✅ `evals/compaction/` |
| Post-compaction selective re-attachment of live file/skill state | ✅ **live for BOTH kinds of state and in BOTH loops (2026-08-24)**, need-driven rather than pre-attached. **Conversation:** `reattach_context` — compaction edits the PROMPT, never the durable thread, so the model reads compacted-away turns back on demand (20 messages / 6 000 chars, excluding what the prompt already carries); both compaction notices name it, and an unauthenticated read is a stated error, never an empty result. **Skills:** `reattach_skill` + `mp_contracts::skill_recovery` — the shared skill budget degrades before dropping and marks what it cut, which was honest and *unrecoverable*; the marker now names the tool, and a disabled skill is reported as disabled rather than as missing. **And the agent loop can now compact at all:** it had NO compaction of any kind, so an overflowing deployed-agent run simply failed on the graceful-failure sentence while chat degraded. `execution-core::compaction_budget` brings tier 1 across at the same threshold, spared-tail and notice, pinned by `tests/compaction_parity_contract.rs` (4 tests, mutation-verified); `RunAgentResponse.compaction_triggered` now reports it instead of being a hardcoded `false`. The literal Claude Code shape (pre-attach N recently-touched files per turn) still does not transfer — see §7.5 | ✅ budgeted 5-file/5-skill reattachment | 🟡 (surface-preserving cut, not confirmed selective reattachment) | ❌ |
| One-shot subagent delegation | ✅ (subagent hook exists in execution-core) | ✅ `AgentTool`/`runAgent()` | ✅ `SubagentRun` | ✅ `delegate_task` |
| Durable/continuable subagent lifecycle | ✅ **live and genuinely continuable** — lineage 2026-08-22, cold resume + replay semantics 2026-08-24. The 08-22 finding was worse than this row said: `subagent_edges`/`GetSubagentLineage` were written ONLY by the orchestration surface, so for every subagent a real user triggered the lineage endpoint returned nothing. In-loop delegation now registers a durable managed child run + edge and settles its receipt on every outcome. **Cold resume:** the child's answer is persisted on the CHILD run's own record (`RecordRunOutput` → `runs.final_output`, a column that had existed since `0001_init.sql` with no writer), read back through `list_subagent_results` (content-free, ungated) and `read_subagent_result` (approval-gated on every posture) — a resumed parent learns *that* its child finished; *what* it concluded needs the user's consent. **Continuation, and the bug closing it fixed:** `start_key` is identifiers-only so a re-driven parent step reuses its child run — which made re-running the nested loop actively wrong, because the child already carries an IMMUTABLE terminal receipt and a second `RecordTerminalOutcome` returns the FIRST outcome. A retry that succeeded handed the parent a good answer while the lineage permanently recorded the child as failed. A replay now **resumes from the recorded answer and charges zero rounds**, with distinct refusals for "finished with nothing recorded" and "still in flight"; and a delegation is excluded from in-turn retry outright, since replaying it forks the answer from the receipt. *Not adopted:* DeepSeek's FIFO inbox — our delegation is synchronous within a turn, and an inbox is a different execution model rather than a missing piece of this one | 🟡 (background-resumable agents via task types) | ✅ explicit continuation manager, FIFO inbox, cold resume | ❌ (leaf agents are disposable) |
| Fork-semantics subagent (inherit parent history) | ❌ **by decision, 2026-08-24 — not an absence** — DeepSeek considered fork and rejected it: "inherited completed turns are implicit, growing handoff state and violate the fresh-context contract". Its fresh-agent contract is a distinct session with no seed sharing only the working tree, which is what `run_subagent` already does and what `subagent_runs_a_real_nested_loop_in_an_isolated_context_and_returns_its_answer` asserts in both directions. Building fork here would adopt the thing the reference discarded | ✅ `forkSubagent.ts` | 🟡 (session fork exists at `ctx.sessions.fork`, not tool-triggered the same way) | ❌ |
| Filesystem isolation for concurrent agents (worktrees) | ❌ **by decision, 2026-08-24 — and the reference agrees** — DeepSeek does not isolate per-agent filesystems for its fresh-agent loop either (the ❌ in its column is deliberate): children share the parent's cwd *because* the working tree is the durable authority. What it isolates is CONTEXT, which `run_subagent` already does, and what it adds against runaway concurrency is a per-run child ceiling — `subagent::MAX_TOTAL_CHILDREN = 8`, independent of the round budget, since the budget bounds work and not fan-out | ✅ `EnterWorktreeTool`/`ExitWorktreeTool` | ❌ | ❌ |
| Model-written orchestration script (fan-out/pipeline as real code) | 🟡 Temporal workflows exist but aren't the production path | 🟡 (coordinator mode + task DAG, not model-authored scripts) | ✅ `workflow` (`node:worker_threads`, `agent()`/`pipeline()`/`parallel()` hooks) | ❌ |
| Hard-coded leaf-vs-orchestrator subagent role split | ✅ **now live** (2026-08-22) — `MAX_DEPTH`/`guard_depth` (pre-existing) + a new unconditional risky-tool refusal for any `depth > 0` call, reusing `permission::is_risky_call` as the blocklist (see §7.3) | 🟡 (role-specific permission handlers exist) | 🟡 (role via provider config) | ✅ `DELEGATE_BLOCKED_TOOLS` blocklist + `max_spawn_depth` |
| Plugin system | ✅ **[CORRECTED 2026-08-22 — "stub" was wrong]** real registry: `KindPlugin` capabilities seeded, `/api/v1/plugins` served by a 220-line handler over the durable capabilities store. Architecture is ONE `Capability` registry with a `Kind` discriminator, which is why grepping for a `PluginsRegistry` type found nothing | ✅ marketplace-sourced installable units | ✅ "everything is a plugin" on Cordis | ✅ 21 first-class packages, `plugin.yaml` discovery |
| Skill system | ✅ `agent_skills` registry live | ✅ bundled + user-loaded, incl. a `skillify` skill that generates new skills | ✅ `.agents/skills/`, loaded on-demand into context | ✅ `skills/` + `optional-skills/` |
| Hook/lifecycle event system | ✅ PreToolUse/PostToolUse (execution-core) | ✅ ~19+ named hook events, 4 action kinds incl. `agent`/`http` | ✅ waterfall/emit/parallel/serial typed events, wire-protocol bridge to Claude Code/Codex hooks | ✅ `invoke_hook()` sites + shell-script bridge |
| MCP client/registry | ✅ **[CORRECTED 2026-08-22 — "stub" was badly wrong]** `api/registry_apis.go` is a 1,781-line hardened implementation: Postgres-backed `mcp_servers` CRUD, `mcp_oauth_tokens` encrypted at rest via a vault, SSRF guards (`mcpForbiddenRanges` netip prefixes + host resolver), tool-allowlist validation with bounds, scope/owner/explicit-share model, plus a DNS revalidator | ✅ (`services/mcp`) | — (not a primary integration surface observed) | 🟡 (`optional-skills/mcp/`) |
| Multi-provider/model routing abstraction | ✅ **[CORRECTED 2026-08-26 — the row's own "not yet re-verified" caveat was the accurate part]** not ad-hoc: `inference-core/provider/` carries `intent.rs` (mode → model from heuristic task complexity plus the org's budget posture), `routing_policy.rs` (the complexity weights, reasoning-keyword list and mode × complexity → model table externalised from compile-time consts so they are runtime-tunable), `fallback.rs`'s provider walk with hint/model-family/ZDR/privacy-tier gates, and `policy_client.rs`. `FallbackChain` consumes `RoutingPolicy` and `intent::BudgetClient` directly | ✅ | 🟡 (LLM capability seam, single-provider-family focus) | ✅ 300+ models via Nous Portal |
| Cost/budget tracking | ✅ `cost-core`, genuinely mature, no stubs | 🟡 (`cost-tracker.ts` exists, scope unclear from structure alone) | ❌ not a primary concern | ❌ not a primary concern |
| Voice I/O | ✅ **[CORRECTED 2026-08-22 — "❌" was wrong]** `inference-core/provider/speech.rs` is 1,073 lines; `SynthesizeSpeech`/`TranscribeSpeech`/`ListSpeechVoices` RPCs; gateway surfaces `speech`/`speech_to_text`/`speech_voices`/`transcribe`/`transcription`; `cap.inference.speech` seeded. What Hermes has beyond this is the *product* layer (wakewords, a Meet bot), not the I/O capability | ✅ STT/TTS | ❌ | ✅ wakewords, streaming TTS, full Google Meet voice bot |
| Self-modification / runtime plugin mounting by the agent itself | ❌ **needs a product call, and the row should not read as a gap** (noted 2026-08-26) — three of the four systems lack this, and the one that has it labels its own mechanism *explicitly non-security-boundary*. Adopting it here means letting the agent mount code into a runtime that holds live tenant data and provider credentials, which is a posture decision rather than a missing feature. Recorded as open-with-analysis rather than silently ❌, because a bare ❌ invites someone to close it by building it | ❌ | ✅ `dsh-tool-cordis` (explicitly non-security-boundary vm mount) | ❌ |
| Generated, freshness-gated documentation as a CI gate | ✅ **exists and is now GREEN** (2026-08-26) — `scripts/gen-sse-event-taxonomy.py --check` diffs the committed `docs/sse-event-taxonomy.md` against source AND asserts every `ChatEvent` has a client `case`, wired in `.github/workflows/sse-taxonomy.yml` on push+PR. It had been **failing on a false positive**: a comment sitting between two `\|`-chained patterns in `family()` broke `parse_arms`'s unbroken-chain regex, so `Title`/`FollowUps` were reported as having no family arm when both are plainly in the `=> None` control group. A red gate stops being read, and this one had also stopped catching real drift — `queued_input` was missing from the committed doc. Comment stripping added (mutation-verified), doc regenerated, gate green at 15 events. Narrower than DeepSeek's (one taxonomy, not every plugin) |
| Blameless postmortem / rejected-design ledger | ✅ **now live** (2026-08-22) — `docs/postmortem/0001-...` + `docs/decisions/ledger.md`, seeded with HARN-1/2, the letta-bridge deferral, and the sandbox-backend-trait deferral (§7.1); see §7.6 | ❌ (not observable from source alone) | ✅ 4 numbered postmortems + 688-entry Agent Notes ledger | ❌ |
| Dedicated eval harness (general agent quality) | ✅ "eval harness MVP" recently shipped (git log, 2026-08) | ❌ (not observed) | 🟡 (snapshot-replay tests, not a scored eval harness) | ✅ `evals/readtool`, `evals/compaction` |
| **Tool-argument grounding (a required value must appear in the request)** | ✅ **live in both loops** (2026-08-25) — `mp_contracts::tool_arguments::{ungrounded_arguments, grounding_message}`, called pre-dispatch right after schema validation. Measured need: required user-held values were fabricated **55–70 %** of the time in the agent loop and **100 %** (20/20) in chat, every fabrication schema-valid. A prompt-only fix took the agent loop from 1/20 to 12/20 and stalled there; the gate makes the residual zero by construction, and 9/9 refusals produced a question rather than a second guess. Narrow by design — two shipping tools, because a required value that merely *restates the request* (`web_search.query`) or is public fact (`yr_weather.lat/lon`) must never be asked for, and a *discoverable* id (`execute_provider_action.connection_id`) is already resolved unprompted 10/10 via `list_provider_actions` | ❌ | ❌ | ❌ |
| Elicitation instruction for un-inventable arguments | ✅ **both loops** (2026-08-26) — `SNIPPET_USER_SUPPLIED_ARGS` is now byte-identical in model-gateway and execution-core, pinned by a decoded-string contract test (raw source comparison would assert formatting, not words). Agent loop +18.3 pp (p=0.016). Chat initially gained only +4/20, and the cause was findable rather than inherent: the snippet's closing clause cross-referenced `PREAMBLE_CORE`'s anti-permission rule, which resolves in the agent loop and **dangles in chat**, whose system stack carries no tool guidance at all. Rewritten self-contained (states the permission distinction inline, and names the action exclusively — "do not call the tool at all — reply with one short question"). A/B at 60 samples per arm: **26.7 % → 45.0 %, +18.3 pp, z=2.09, p=0.036** — chat now gains what the agent loop does. Zero under-calling regression in either loop (40/40 and 60/60), and the agent loop held at 85.0 % vs 83.3 % (noise). Wording stays byte-identical across loops, contract-pinned | 🟡 (per-tool description prose) | ❌ | ❌ |
| Signed/attested release artifacts | ✅ artifact-v3, uniquely rigorous among the four | ❌ n/a (leaked snapshot) | ❌ n/a (dev preview) | ❌ n/a |
| License | Internal/proprietary | **None — leaked proprietary** | MIT | MIT |

---

## 4. Deep comparison by dimension

### 4.1 Agent / turn loop design
All four converge on the same shape — a **turn** is one-or-more **steps**, a
step is one model request plus its tool calls — but differ in how explicit that
shape is as an *architectural contract* versus an implementation detail.

- **DeepSeek Harness** is the most explicit: the turn/step lifecycle is a
  documented state machine (`docs/agent-lifecycle.md`) with the invariant
  *model-visible ⟺ logged* enforced at runtime, not just by convention. The
  driver, the model adapter, and the tool registry are themselves ordinary
  plugins with no privileged core — the loop's shape is a documented extension
  point, not something you patch around.
- **Claude Code** concentrates the entire loop (streaming, tool iteration,
  extended-thinking handling, retries, token accounting) inside one ~765-line
  method (`submitMessage`) in an otherwise near-empty class — real single-point-
  of-complexity risk, independent of the inflated line-count claims in that
  repo's own `CODEBASE_ASSESSMENT.md` (see §5.4 for the correction).
- **Hermes** layers a rich per-turn prologue (`turn_context.py`) ahead of the
  actual model call, including an explicit fix for a documented race: if a
  background-review fork from the *previous* turn is still running, the new
  turn interrupts it before making its own API calls on the same
  session/credentials.
- **Model Plane** already made its most interesting loop-level decision very
  recently and it deserves top billing here, not a footnote: on 2026-08-14 a
  proposal (internally tagged HARN-1/2) to unify `execution-core`'s
  `execute_step_inner` and `model-gateway`'s `dispatch_tool` into one module
  and one turn-loop trait was **measured and explicitly withdrawn**. The two
  dispatchers share zero tools; `dispatch_tool` is an 18-arm *read-only* router
  whose first act is refusing anything side-effecting, while
  `execute_step_inner` is the full capability/hook/permission pipeline around
  sandboxed execution — the refusal boundary between them **is** the authority
  boundary, and merging them would make an accidental cross-call compile. Three
  cross-service invariants that were previously "enforced by comment only"
  (e.g., a deployed agent must never get a smaller tool-call round budget than
  plain chat) are now asserted at runtime in `cross_service_loop_contract.rs`
  and mutation-tested (perturb both sides, confirm the assertion fails
  legibly). **This is a genuinely mature call** — resisting a "collapse it into
  one module" instinct after actually measuring the coupling cost — and belongs
  in the "smart implementations" list for Model Plane itself, not just the
  three external systems.

### 4.2 Tool execution & sandboxing
This is the dimension with the most actionable, concrete convergence.

All three external systems land on the same idea from different angles: a
sandbox/execution backend should be **one minimal interface** that both a real
OS subprocess and an SDK-only/remote backend satisfy identically.
- DeepSeek: `ctx.sandbox`/`ctx.sandboxPolicy` wraps a same-world subprocess argv
  in a policy (`read-only`/`workspace-write`/`danger-full-access`); backends
  (Linux bwrap/Landlock, macOS Seatbelt, Windows ACL runner) each report their
  own enforcement completeness rather than a boolean.
- Hermes: `BaseEnvironment` ABC + `ProcessHandle` Protocol; SDK-only backends
  (Modal, Daytona) wrap their blocking call in a `_ThreadedProcessHandle`
  adapter that exposes the exact same `poll()/kill()/wait()/stdout` surface as
  a real `subprocess.Popen`, so the shared execute() machinery (snapshotting,
  CWD tracking, timeout, interrupt, output capture) is written once.
- Claude Code: sandbox config is one schema (`entrypoints/sandboxTypes.ts`,
  explicitly documented in-repo as the single source of truth) consumed by
  *both* the SDK and the interactive permission system's own allow/deny rules
  — preventing "what the user approved" and "what the OS actually allows" from
  drifting into two independently-reasoned-about surfaces.

Model Plane's own sandbox story just crossed an important threshold (§0): real
`bwrap` isolation is now source-confirmed in `execution-core`. What it does
**not** yet have is any of the three patterns above — a formal backend
interface, honest full/partial enforcement reporting, or a single schema shared
between the policy engine and the sandbox layer. `sandbox-manager`'s own
create-paths (the Go side — lease/snapshot/reconcile) remain the largest
still-open piece per the existing Tier 2 harvest plan; the Rust-side OS
isolation piece of that plan appears to have shipped ahead of the Go side.

### 4.3 Memory & context management
The three external systems each solved a different sub-problem well and no
one solved all of them:
- **Hermes** has the most rigorous *compaction-quality measurement*
  (`evals/compaction/` scores strategies by recall accuracy against gold
  questions generated from the discarded region — not token-count reduction)
  and the cleanest multi-backend memory registry discipline (one external
  provider at a time, reserved core tool names so a provider schema can never
  shadow a built-in tool).
- **Claude Code** has the most sophisticated *compaction escalation* (four
  tiers, cheapest first, cache-aware) and the most direct fix for compaction's
  worst failure mode — losing the file you were mid-edit on — via budgeted
  post-compact reattachment.
- **DeepSeek Harness** has the strongest *durability* story for the compaction
  operation itself: the start-of-operation log event is written before work
  begins and the end event only after the replacement message has landed, so a
  mid-operation crash is a mechanically detectable orphaned marker rather than
  a false "finished" record.
- **Model Plane** has real durable session/event persistence (arguably the
  most rigorously modeled of the four — single-writer-per-run, ULID-prefixed
  keys, transactional outbox) but no confirmed tiered compaction strategy and
  no compaction-quality eval harness at all. Long-term/semantic memory
  (`letta-bridge`) is correctly left as a single stub rather than
  over-engineered into a registry no one needs yet (§7.4) — this is a good
  decision already made, not a gap.

### 4.4 Multi-agent / subagent orchestration
- **DeepSeek Harness** has the most conceptually rigorous design: it names and
  separates *disposable one-shot delegation* from *durable continuable
  subagents* as genuinely different reliability/authorization problems (who
  may follow up, whether interruption reaches descendants, child-before-parent
  disposal ordering) — plus two further, distinct orchestration primitives
  (`ralph` for fresh-agent-per-round statelessness, `workflow` for a
  model-written JS script with `agent()`/`pipeline()`/`parallel()` coordination
  hooks and hard concurrency/agent-count caps).
- **Claude Code** solves a different, also-real problem well: fork-semantics
  delegation (warm-start from parent history) paired with git-worktree
  isolation as two small composable primitives, rather than one "parallel
  agent" mega-mechanism.
- **Hermes** has the most explicit blast-radius control for delegated work: a
  hard-coded leaf-vs-orchestrator role split with a literal tool blocklist for
  leaves and a configurable max recursion depth surfaced in the tool's own
  schema description (so the model can see the limit, not just be silently
  bound by it).
- **Model Plane** has a `subagent_edges` table and a subagent hook in
  `execution-core`, but nothing in the explored source confirms a continuation
  manager, a leaf/orchestrator distinction, or fork-semantics warm-start — this
  is the widest gap of the four dimensions covered here relative to all three
  external systems, and probably deserves to be its own roadmap phase rather
  than an incidental line item.

### 4.5 Engineering process & quality culture
Worth comparing on its own, since it predicts how fast a gap actually closes
once identified:
- **DeepSeek Harness** is the standout: documentation that boots real plugins
  and fails CI if it drifts from source; four numbered blameless postmortems,
  two of which are explicitly about **fully-covered, all-green code that shipped
  completely non-functional** (a stray default export dropped a DI injection; a
  config expression was evaluated in the wrong loader location) — a real,
  cited lesson that 100% coverage proves lines ran, not that the feature works.
- **Model Plane** independently proves the same lesson in its own way: the
  `MODEL_PLANE_STATUS.md`/`ROADMAP.md` correction history (2026-05-30,
  2026-07-11, 2026-07-13 reconciliations, each explicitly correcting a
  previous ❌/✅ claim that turned out wrong) and the HARN-1/2 withdrawal are
  the same discipline — measure before trusting a plan, and say so in writing
  when the plan was wrong — expressed through commit messages and dated audits
  rather than a dedicated postmortem directory. **Recommendation: give this its
  own lightweight home** (§7.6) rather than leaving it scattered across commit
  bodies and doc-correction footnotes, which is easy to lose track of at 150+
  commits/month velocity.
- **Hermes** is the most honest about its own weak defaults — its security docs
  candidly document an opt-in network-egress-isolation topology while
  disclosing the shipped default is `network_mode: host` (unrestricted), and a
  fail-open content scanner default. That kind of documented, undefended gap is
  more useful than a silent one.
- **Claude Code (leaked)** cannot be evaluated on process at all — no tests, no
  CI config, no lint config, and a squashed single-commit history exist in this
  artifact; nothing about its actual development process is recoverable from
  the leak.

---

## 5. Smart implementations, spotlighted

### 5.1 DeepSeek Harness
1. **Waterfall + monotonic-guard split for tool policy** — an open,
   plugin-extensible pre/post hook chain for ordinary policy, plus a small,
   fixed, non-reorderable layer of "monotonic guards" for identity-protecting
   allow/deny decisions that no later-registered plugin can shadow. Solves the
   classic "middleware ordering is a footgun" problem without sacrificing
   extensibility.
2. **Compaction lock released last, not first** — `compaction/start` is logged
   before work begins, `compaction/end` only after the replacement message has
   landed, turning a mid-crash into a detectable orphaned-lock state instead of
   a false completion record.
3. **Sandbox enforcement reported as a fact, not assumed** — every confinement
   backend returns an explicit `full`/`partial` value alongside its wrapped
   argv; the project's own postmortem 0004 shows exactly what breaks when this
   discipline briefly slipped.
4. **Generated, boot-verified documentation as a CI gate** — the tool catalog
   doc is produced by actually booting each tool plugin against a real context
   and reading its live schema, then failing CI if the committed doc differs
   from a fresh regeneration.
5. **Self-modification as one bounded primitive, not N structured tools** — the
   team explicitly evaluated and rejected a "one tool per capability"
   self-modification design (documented, with a real tradeoff table) in favor
   of a single vm-mount primitive plus inspect/unmount — resisting scope creep
   by identifying what actually needed solving.
6. **Blameless, numbered postmortems with root-cause-to-guardrail
   traceability** — each pairs a concrete "why did green CI ship a broken
   feature" root cause with a guardrail landed in the same fix.

### 5.2 Hermes Agent
1. **`ProcessHandle` Protocol + threaded-adapter pattern** — one minimal
   execution-handle interface that a real OS process satisfies natively and an
   SDK-only backend satisfies via a background-thread adapter, so
   session/CWD/timeout/interrupt machinery is written once, not once per
   backend.
2. **Two-owner compaction handoff, not a race** — native provider-side
   compaction gets first shot on eligible model+route combinations, clamped a
   safety margin below the local compressor's own trigger; the local
   compressor only fires if native didn't. An explicit handoff, not two
   systems racing to compact the same context.
3. **`MemoryProvider`'s one-external-provider cap + reserved core tool names**
   — a hard ceiling on simultaneously-active external memory backends, plus a
   reserved namespace of core tool names no provider registration can ever
   shadow.
4. **Leaf-vs-orchestrator subagent split with a literal tool blocklist** — a
   hard-coded `DELEGATE_BLOCKED_TOOLS` list (`delegate_task`, `clarify`,
   `memory`, `send_message`, `cronjob`) for leaf children, a separate
   orchestrator role that regains only `delegate_task`, and a configurable max
   recursion depth surfaced honestly in the tool's own model-facing schema
   description.
5. **Recall-accuracy compaction eval harness** — `evals/compaction/` scores
   compaction policies against gold questions generated from the region about
   to be summarized away, not token-count reduction — a genuinely rare piece
   of rigor for an open agent project.
6. **Thread-local approval-callback propagation fix** — subagents run in a
   `ThreadPoolExecutor`; a documented fix installs a safe non-interactive
   approval callback into every worker thread at pool-init time specifically
   to prevent a deadlock against the parent's interactive terminal prompt.

### 5.3 Claude Code (leaked source — described structurally only, no code reproduced)
1. **Declarative `ToolDef` → `buildTool()` factory** — ~40 independently
   authored tool folders each export a plain config object (schema,
   permission classification, prompt, execute function); one factory function
   turns that declaration into the runtime interface, centralizing typing,
   immutability, and permission plumbing so no individual tool can forget a
   safety check.
2. **Fork-semantics subagent spawning + worktree isolation as two separate
   primitives** — warm-starting a child from parent history and isolating
   concurrent filesystem writes are solved by two small composable tools
   (`forkSubagent` + `EnterWorktreeTool`/`ExitWorktreeTool`) instead of one
   monolithic "parallel agent" mechanism.
3. **Escalating, cache-aware compaction tiers** — time-based light clearing →
   client-side microcompact → provider-native context editing → full LLM
   summarization, cheapest tier tried first, with explicit tracking of which
   trims are safe against the prompt-cache prefix.
4. **One security schema shared by the permission UI and the OS sandbox** — a
   single documented source-of-truth type is consumed by both the SDK and the
   interactive approval system's own allow/deny rules, so "what the user
   approved" and "what the sandbox actually allows" cannot silently diverge.
5. **One pattern-matching DSL reused for both permission rules and hook
   triggers** — the same `"Bash(git *)"`-style rule syntax gates both
   interactive tool approval and whether a configured hook fires at all; one
   parser is hardened and trusted for two purposes instead of two.
6. **Post-compact selective, budgeted file/skill reattachment** — after a full
   compaction, up to 5 previously-read files and 5 previously-active skills
   are individually and jointly re-attached under hard token budgets, directly
   targeting compaction's worst failure mode (losing the file you were
   mid-edit on) without undoing the token savings.

### 5.4 Model Plane itself (a self-audit, since it's easy to only look outward)
1. **The HARN-1/2 withdrawal** (§4.1) — measuring a "collapse two loops into
   one" proposal against actual tool overlap (zero) before writing code, and
   recording why it didn't hold.
2. **`cross_service_loop_contract.rs`** — cross-service invariants that
   *cannot* be a build dependency (two independently deployed services) are
   asserted at runtime and mutation-tested (perturb both sides, confirm
   legible failure), rather than "enforced by comment" the way they used to be
   — this is directly comparable to DeepSeek's runtime-asserted
   model-visible-⟺-logged invariant and is the same quality of discipline.
3. **The `letta-bridge` memory-adapter deferral** (`capability-ownership-matrix.md`
   §4.4) — an explicit, dated design call to *not* build a multi-adapter
   registry for a single in-memory stub, and to build it "when the second
   real backend lands," recorded as a decision rather than an oversight. This
   is the correct answer to the same trap DeepSeek's own postmortems and
   Hermes's `MemoryManager` cap both gesture at from different angles
   (premature multi-backend abstraction).
4. **Cost-core's authoritative pricing fallback** — `RecordUsage` prices from
   the catalogue when `cost_usd` is absent, so the dollar ledger and budget
   posture are never silently `$0` — a small, easy-to-get-wrong detail handled
   correctly and tested.

---

## 6. Honest gaps in Model Plane (confirmed by source, 2026-08-16)

- ~~**No parallel tool-call dispatch.**~~ **Closed 2026-08-22** — see §7.2.
- **No tool retry on transient failure**, confirmed unchanged since the
  2026-07-17 audit.
- **`capability-core`'s tools/plugins/MCP registries remain stubs.** Skills,
  models, routing, and safety registries are live; tools/plugins/MCP are not,
  per both the prior harvest and a fresh grep for `ToolsRegistry`/
  `PluginsRegistry`/`McpRegistry`-shaped types (none found).
- **No compaction-quality eval harness**, and no confirmed tiered compaction
  strategy — both `hermes-agent` and `claude-code-fork` treat this as a
  first-class, multi-tier concern; Model Plane's compaction posture could not
  be confirmed to be more than "a compaction-trigger hook exists."
- **No durable/continuable subagent lifecycle, leaf/orchestrator role split,
  or fork-semantics warm-start** — the widest multi-agent gap of the four
  systems compared (§4.4).
- **Sandbox enforcement is not reported as a fact.** Now that real `bwrap`
  isolation exists, it should report `full`/`partial` per-platform the way
  DeepSeek Harness does, rather than the Go orchestration layer trusting a
  single implicit flag.
- **No shared schema between the capability-policy engine and the sandbox
  layer.** Claude Code's single-source-of-truth pattern (§4.2) directly
  addresses the failure mode of "what the policy engine approved" and "what
  the sandbox actually permits" drifting apart — worth an explicit check that
  Model Plane's `capability_policy.rs` and `sandbox.rs` share one model.
- **`orchestrator-core`'s Temporal workflows are still not the production
  dispatch path** — real, tested, but bypassed by the live
  `model-gateway → execution-core` loop for anything outside a handful of
  supervised flows. This predates this audit but is worth restating since it
  directly limits how much of the multi-agent/orchestration gap above can be
  closed by "just wire up the existing Temporal workflows."

---

## 7. What Model Plane should adopt — prioritized

Ordered by leverage (how many other gaps a single change closes) rather than
raw effort. Each item states its source and license status.

### 7.1 Formalize the sandbox-backend interface as a true capability seam (High leverage) — 🟡 RESCOPED, real gap closed (2026-08-22)
Original ask: adopt the pattern all three external systems converge on
independently — one minimal `SandboxBackend { init_session, execute, cleanup }`
trait + `SandboxHandle { poll, kill, wait, stdout }` interface (already scoped
in `external-ideas-harvest.md` #7, hermes-shaped, MIT) that a real
`bwrap`-wrapped process satisfies natively and a remote/SDK-only future backend
(Modal-equivalent) satisfies via a thread-or-task adapter.

**On inspection, the trait scaffolding itself is premature per this repo's own
rule** (`CLAUDE.md`: "a registry/interface for one implementation is
premature... build the seam when the second real backend lands" — the exact
`letta-bridge` precedent). Today there is exactly one local backend
(bwrap-or-passthrough); `MpSandboxPolicy::External` already anticipates a
remote/Daytona-style backend but hands it to `sandbox-manager` (Go), a
different service. Introducing a trait now, with nothing to poly-morph over,
would be the identical mistake this codebase already declined to make for
memory adapters.

**What the "honest full/partial enforcement" half of this item was actually
pointing at turned out to be real and unclosed**, found by reading
`sandbox.rs`/`executor.rs` directly rather than assuming the trait was the only
way to satisfy it: `ExecOutcome.sandboxed` already exists and is a genuinely
probed fact (not a trusted flag — `capability_profile()` runs a real bwrap
namespace probe), and `require_requested_isolation` already failed closed on
missing *filesystem* isolation. But `MpNetworkPolicy::AllowDomains` is
deliberately not self-enforcing in bwrap (the allowlist lives at an external
egress proxy) — and nothing checked that a proxy was actually configured.
`capability-ownership-matrix.md` §G1 had this **explicitly recorded as a
requirement since 2026-05-30** ("the G1 executor must fail closed when no
proxy is configured") and it was never implemented. **Closed 2026-08-22**:
`require_requested_isolation` now also refuses `AllowDomains` when neither
`HTTPS_PROXY` nor `ALL_PROXY` (either case) is set — a real, previously-silent
gap where `sandboxed: true` could be reported while network egress was
completely unrestricted. 4 new tests, workspace green, zero new clippy
findings. `sandbox-manager` (Go) still does not consume `capability_profile()`
at all — flagged, not built: that's a cross-plane wiring decision (does a
Go orchestrator gate scheduling on it, or is health-check-only sufficient?)
that needs a design call, not a silent default.

### 7.2 Parallel tool-call dispatch (High leverage, contained blast radius) — ✅ DONE (2026-08-22)
Replace the sequential `for call in &response.tool_calls` loop with a bounded
concurrent dispatch (DeepSeek's `executionMode`: ordered vs. bounded-concurrent-
pool is a reasonable model — not everything needs to run in parallel, but
independent read-only calls should). This is a pure latency win with no
license concerns (describe-and-reimplement, not port).

**Shipped** in `execution-core/src/runtime_loop/agent.rs`'s `run_rounds`: a
three-phase dispatch (sequential purpose-lock/dedup/step-id pre-pass →
concurrent `execute_step_with_subagent` dispatch via `futures::future::join_all`
→ sequential outcome processing in original array order) replaces the single
sequential loop, matching the model-gateway precedent already used in
`deep_research.rs`/`dataplane.rs`. Two correctness properties the naive version
would have broken, both fixed:
- **Subagent delegation budget** — `LoopSubagentDispatch` used to snapshot the
  remaining round budget per call; two concurrent siblings that both delegate
  would each see the full stale remainder and, combined, overspend it. Fixed
  with an atomic *exclusive claim* (`AtomicU32::swap(0, ..)`) shared across the
  batch, not a load-then-settle-later scheme (which a test caught failing:
  `two_concurrent_delegations_in_one_round_share_the_same_budget_pool` first
  demonstrated the double-spend, then verified the swap-based fix).
- **Audit completeness under an approval pause** — today's sequential loop
  stops at the first call needing approval, so nothing after it is even
  attempted. Concurrent dispatch means a later, independently-`Allow`ed call
  may now genuinely execute before that fact is known; the pause is deferred
  (`pending_pause`) until the full batch's outcomes are processed, so every
  call that actually ran still gets its `record_tool_step` audit write
  regardless of position relative to the pausing call
  (`a_tool_call_after_the_first_pause_is_still_recorded_not_silently_dropped`).
  A second approval-needing call in the same batch is deferred, not durably
  double-queued (`a_second_approval_needing_call_in_the_same_batch_does_not_get_a_second_durable_approval`).

No side-effecting call ever runs unapproved regardless of dispatch order: the
capability/hook/permission `Ask`/`AwaitApproval` decisions short-circuit
`execute_step_inner` before any tool body executes, confirmed by direct source
read before this landed. 4 new tests, full `execution-core` + workspace suites
green, zero new clippy findings.

### 7.3 A leaf/orchestrator subagent role split with a literal tool blocklist (Medium-high leverage) — ✅ DONE (2026-08-22), rescoped to execution-core
Adopt Hermes's pattern directly: a hard-coded blocklist of side-effecting
tools (delegate/clarify/memory-write/send-message/schedule) for any child
agent by default, a separate orchestrator role that regains only delegation,
and a configurable max recursion depth surfaced in the tool's own schema so
the model can see its own limit.

**Rescoped from Go to Rust, and the "orchestrator role" carve-out dropped as
inapplicable — both deliberate, not oversights.** `orchestrator-core` (Go)
does not run the tool-call loop; `execution-core`'s `run_rounds` does, and it
already tracks `depth` (0 = top-level run, 1 = the one delegated subagent
level `MAX_DEPTH` allows). Since nesting is capped at one level, there is no
depth-2+ "orchestrator" that would need delegation rights carved back in —
the depth-0 run already has full delegation rights, and depth-1 is the only
leaf role that exists. Implementing the Go-side depth-counter this item
originally proposed would have built a second copy of a bound this crate
already enforces (`subagent::MAX_DEPTH`/`guard_depth`).

**What shipped:** delegation itself was already blocked one level deep
(pre-existing `guard_depth`), and an approval-gated tool was already refused
inside a subagent (it cannot request human sign-off) — but under `auto` mode,
with no approval gate active at all, a subagent could run ANY tool, including
destructive ones, with strictly LESS oversight than the top-level run. Added
a new pre-dispatch check in `run_rounds`'s sequential pre-pass: `depth > 0`
and `permission::is_risky_call(...)` now refuses outright, unconditionally,
regardless of permission mode — reusing the existing name/argument-aware risk
classifier (`is_risky_tool`/`is_risky_call`) rather than a new blocklist
vocabulary, so the leaf restriction and the top-level `ask`-mode gate stay
defined by ONE list, not two that can drift. The `subagent.task` tool
definition's own description was updated to state this real constraint (the
depth-limit-in-schema half of the ask), not just "cannot delegate further."
1 new test (`a_delegated_subagent_may_never_run_a_risky_tool_even_under_auto_mode`,
proves the NEW rule fires specifically, not the pre-existing approval-refusal
which only applies under `ask`), workspace green, zero new clippy findings.

### 7.4 A compaction-quality eval harness (Medium leverage, currently zero investment) — ✅ DONE (2026-08-22), plugged into eval-lab-py
Adopt Hermes's `evals/compaction/` pattern: score any compaction/summarization
strategy against gold recall questions generated from the discarded region,
not against token-count reduction alone, and keep it as a CI-adjacent
artifact so compaction policy changes get a regression signal. Model Plane
already shipped a general "eval harness MVP" per recent git history — this
should plug into that infrastructure rather than becoming a parallel harness.

**Investigated first, not assumed**: there are TWO unrelated `compaction.rs`
files. `session-core`'s is a durable-store step-watermark (never touches
message content); `model-gateway`'s is the real conversation-quality
compaction — tier 1 clears stale tool-result payloads past a byte budget,
tier 2 (`plan_head_summary`) replaces everything before the last
`MAX_THREAD_CONTEXT_MESSAGES` (24) messages with ONE real LLM-generated
summary once a thread crosses that size. Tier 2 is the one that can
genuinely lose information, so it's the target.

**Shipped as a `seed_turns` extension to the existing harness, not a parallel
one** — exactly per the "plug in, don't duplicate" instruction:
- `CaseSpec.seed_turns: list[str]` — turns sent before `prompt`, all in ONE
  session (`eval_lab/types.py`). Empty (every existing case) is byte-for-byte
  today's single-shot behavior.
- `client.py`'s `invoke_stream` gained `content=`/`session_key=` overrides so
  one client method can drive a whole multi-turn case, threading
  model-gateway's `InvokeRequest.session_key` wire field through calls so
  history genuinely accumulates in one thread.
- `runner.py`'s new `_run_turns` sends every seed turn (each its own
  idempotency key, one shared session key), then `prompt` as the recall
  question — scored by the SAME `checks`/`judge_rubric` machinery every other
  case already uses. The "gold recall question" is just `prompt` + `checks`;
  no new metric type, no new scoring path.
- 2 new cases (`cases/compaction/13...`, `14...`): a proper-noun-shaped fact
  and a numeric constraint, each stated in seed turn 1 and asked back after
  14 padding turns (comfortably clearing the 24-message threshold — a new
  self-test enforces every compaction case seeds enough turns to actually
  force compaction, not pass vacuously on an untouched transcript, mirroring
  the existing `agentic`-feature guard for the same failure class). Case
  count assertion updated 12→14. 4 new self-tests (single-call fallback,
  multi-turn ordering/session-sharing/idempotency, fail-fast on a broken
  seed turn) plus the 2 new live cases — 18 self-tests total, all green, no
  live network needed to verify the mechanism itself.

### 7.5 Escalating, cache-aware compaction tiers + post-compact selective reattachment (Medium leverage) — 🟡 investigated, largely already true; the real remainder needs a design call (2026-08-22)
Adopt Claude Code's shape (structurally, not by code): cheapest-first
escalation (time-based light clearing → local microcompact → provider-native
context edit if the model/route supports it → full summarization), and after
any full compaction, selectively re-attach a small, token-budgeted set of the
most recently touched files — directly targeting the "lost the file I was
mid-editing" failure mode.

**Read `model-gateway/src/compaction.rs` in full rather than assuming the gap
is real.** The escalation ladder mostly already exists: tier 1
(`clear_stale_tool_results`, byte-budget-triggered, no LLM call) → tier 2
(`plan_head_summary`, one real LLM summarization call, only once a thread
outgrows `MAX_THREAD_CONTEXT_MESSAGES`) → `drop_oldest_group`, a genuinely
cheap local-truncation fallback — it just currently only fires REACTIVELY,
after a provider has already rejected the prompt for length, rather than as a
proactive intermediate rung between tiers 1 and 2. That reactive-vs-proactive
gap is real but narrow, and rescoping `drop_oldest_group` into a proactive
tier is a small, well-bounded change if it's ever worth making.

**The two pieces that are genuinely missing are both real design decisions,
not gaps to mechanically close:**
- **Provider-native context edit.** Checked whether ANY provider adapter
  already exposes something in this family: `inference-core`'s Anthropic
  provider has `cache_control`/prompt caching (`anthropic.rs`), but that is a
  cost/latency optimization — the full prompt still counts against the
  context window; it is not Anthropic's separate context-*editing* API
  (server-side tool-result clearing) and would not reduce what
  `plan_head_summary` has to compact. Wiring that (or an equivalent on
  another provider) would only ever help ONE provider in the fallback chain,
  which raises exactly the kind of cross-provider-consistency question
  (`docs/capability-ownership-matrix.md`'s "one canonical owner per
  capability" lens) that deserves a product/architecture call, not a
  unilateral integration buried inside a harness-parity pass.
- **Selective reattachment of "recently touched" state** — ✅ **RESOLVED and
  built 2026-08-24**, and the product decision came out somewhere neither
  branch above anticipated. The analysis below still stands: the literal
  pattern does not transfer (`code_interpreter` is stateless — its working
  directory is emptied and deleted after every call, so there is nothing to
  reattach), and "keep the citations" was the wrong analog.

  The owner's call was **need-driven recovery, not pre-attachment**: "if there
  is something the user needs and the context has compressed it to a level that
  some info is missing, the model should reattach it". That reframes the
  problem out of existence — compaction edits the PROMPT and the durable thread
  keeps everything, so "I cannot see that" was only ever true of one request.
  No budgeted per-turn attachment, no guess about what counts as recently
  touched: `reattach_context` lets the model read the original messages back
  when it finds it is missing something, and both compaction notices tell it so.

  Original framing, kept because it is why the obvious version was not built:
  Claude Code's version targets a coding CLI's persistent workspace files;
  deciding what "recently touched" means for a retrieval-grounded chat agent is
  a product decision about THIS product, not something to invent by analogy.

Flagging both rather than building a speculative version of either, for the
same reason Track 2's `agents.tools` read-only-projection item was flagged
earlier this session: a real design call belongs to the user, not to an
unreviewed guess baked into a harness-parity pass.

### 7.6 A lightweight postmortem + rejected-design ledger for Model Plane (Medium leverage, cheap) — ✅ DONE (2026-08-22)
Model Plane already *has* the discipline (HARN-1/2 withdrawal, repeated
ROADMAP.md correction notes) — it just doesn't have a *home* for it, so the
next contributor has to grep commit messages to rediscover a settled
tradeoff. Adopt DeepSeek's shape: a small `docs/postmortem/` (numbered,
root-cause-to-guardrail) plus a `.agents/notes`-equivalent
proposed/implemented/rejected/archived ledger. Cheap, no license concerns
(a documentation practice, not code), and directly prevents "let's reconsider
X" churn in a multi-plane, multi-contributor system at this commit velocity.

**Shipped, seeded with real (not invented) content:**
- `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md` — the one
  genuinely incident-shaped item in this repo's history, migrated from
  `git show ac2be529`'s commit message into a structured, discoverable
  root-cause-to-guardrail write-up.
- `docs/decisions/ledger.md` — a lighter-weight running ledger, seeded with
  three real, verified decisions: HARN-1/2 (pointing at the postmortem
  above), the pre-existing letta-bridge memory-adapter deferral
  (`capability-ownership-matrix.md` §4.4), and TODAY's sandbox-backend-trait
  deferral (§7.1) — which is itself a live demonstration of the ledger doing
  its job: the same "defer until a second real backend exists" reasoning,
  applied a second time, now has a precedent to point back to instead of
  being re-derived from scratch.
- `CLAUDE.md` updated to point at both (Code Style + Project Structure +
  Conventions sections) instead of "see git history" / "grep commit
  messages" — the whole point of giving this a home.

Deliberately did NOT pad the ledger with commits that narrate a
documentation *correction* (the ROADMAP.md "❌-era baseline" pattern) as if
they were rejected-design postmortems — those are a real, different, already
partly documented failure class (stale docs vs. live code), not "we proposed
X and measured it away." Padding the ledger with the wrong shape of entry
would undermine the exact "find a real settled decision here" trust the
ledger exists to build.

### 7.7 Do NOT over-correct on memory-adapter registries (a "don't adopt" call)
Both DeepSeek's and Hermes's memory stories are more mature than Model
Plane's — but Model Plane's own 2026-05-30 decision to defer a
`MemoryAdapter` registry until a second real backend exists is *already the
right call*, not a gap to rush-close by copying Hermes's `MemoryProvider` ABC
today. Revisit only when `letta-bridge` gets a real upstream or a second
backend is actually planned.

### 7.8 Continue treating `claude-code-fork` as shapes-only (a compliance reminder, not a new item)
Every item above sourced from `claude-code-fork` is described and adopted as a
*pattern*, never as ported code — consistent with the binding license gate
already in `external-ideas-harvest.md` §1. This audit did not find any reason
to relax that gate.

### 7.9 Hermes memory parity — the verified gap list (added 2026-08-22)

**First, the correction this section exists because of.** Two matrix rows above
described `letta-bridge` as "an in-memory stub" with "no seam" — repeated from
the 2026-05-30 harvest and never re-verified. Reading the source found the
opposite: a `server.Store` interface (`Put`/`Search`/`List`/`Delete`) with
**three** live implementations (`internal/agentmemory` semantic client,
`internal/pgstore` Postgres-lexical, `internal/memstore` in-memory fallback),
backend-capability metadata, and a genuinely honest readiness model that
refuses to report ready for semantic retrieval until a real search has
succeeded (`DEGRADED_SEMANTIC_UNVERIFIED`) — a posture Hermes does not have.
Memory is also already **wired into the live turn**: prefetch at
`model-gateway/grpc.rs`, and read/write exposed as model-callable tools in
`tool_loop.rs`. Anyone planning memory work off the old rows would have
rebuilt what exists.

**What Hermes genuinely has that we do not.** Its `MemoryProvider` ABC
(`agent/memory_provider.py`) is ~20 members against our 4-method `Store`; the
difference is almost entirely **turn/session lifecycle**, not storage:

> **⚠️ Second correction, same day — this section's own first draft was also
> wrong, in the opposite direction.** It said memory was "much further along
> than documented." That is true of *storage* and false of *wiring*. Tracing
> every call site found that memory is **storage without a product**: the main
> browser chat surface has never had memory at all, and the model has no way to
> write memory anywhere. Corrected table below; the earlier optimistic
> paragraph is left standing above only because the storage half of it is
> accurate.

**Verified call-site truth (2026-08-22), not inferred from type names:**

| Aspect | Reality | Evidence |
|---|---|---|
| Storage backends | ✅ real, 3 behind a `Store` seam | `letta-bridge/internal/{agentmemory,pgstore,memstore}` |
| Degraded-readiness honesty | ✅ stronger than Hermes | `DEGRADED_SEMANTIC_UNVERIFIED` — refuses "ready" until a real search succeeds |
| `MemoryService` RPCs | ✅ Search / Index / List / Delete | `memory.proto`, served by letta-bridge *and* session-core |
| Read on the **gRPC Invoke** path | ✅ wired | `fetch_memory_context`, `grpc.rs:710` / `:917` |
| Read on the **SSE chat path** (the browser product) | ❌ **never wired** | `sse.rs` had zero `memory_client` uses before today |
| Memory as a context-assembly segment | ❌ **no `"memory"` segment is produced anywhere** | repo-wide grep for a memory segment kind: zero hits |
| **Model-callable memory write** | ❌ **unreachable** | the `"save_memory"` arm at `tool_loop.rs:2078` sits behind `inline_tool_allowed`'s hard early-return at `:1636`; `execution-core` has no memory tool at all |
| Lifecycle hooks | 🟡 1 of 6 (`on_pre_compress`, added today) | `compaction::summary_prompt_with_memory` + `sse::fetch_compaction_memory_directive` |

So the honest gap is **not** "six lifecycle hooks on a working memory system."
It is: memory is a well-built store that the product barely calls. Hermes
parity therefore starts one level lower than this section first claimed —
prefetch on the real chat surface, and a write path the model can actually
reach — before any of the remaining hooks are worth adding.

**A dead-code hazard worth its own line.** The `save_memory` arm is ~60 lines
of live-looking dispatch code that cannot execute. It is why this document
(and an earlier pass) recorded "memory tools are wired." That is exactly the
anti-pattern §13.3 flags in Claude Code — "UI compiled out of the build but
still reading as wired" — occurring here. Either delete it or gate it behind
the same governed path `browser_agent` uses; leaving it is an invitation to
misread the system a third time.

**Remaining Hermes surface, once the wiring above exists:**
`initialize` · `system_prompt_block` · async/timeout-bounded `prefetch` ·
`recall_status` (also feeds the DeepSeek-style observability workstream) ·
turn-shaped `sync_turn` · `on_turn_start` / `on_session_end` /
`on_session_switch` / `on_delegation` / `on_memory_write` · `backup_paths` ·
and the manager-level governance (one-external-provider cap,
reserved-core-tool-name guard, `is_trivial_prompt` skip, durability-classed
background writes with `flush_pending`).

**Does NOT transfer: Hermes's `StreamingContextScrubber.** It strips echoed
`<memory-context>` tags out of streamed output. Model Plane injects memory as
a plain `"Relevant memory:\n…"` system message with no tags, so there is no
marker to echo and nothing for a scrubber to strip. Porting it would be
building a defence against a mechanism we do not have. (The related question
that *is* real — whether untrusted content can launder itself into a
system-role message via a memory round-trip — is currently moot precisely
because the model cannot write memory. It stops being moot the moment the
write path above is built, and must be designed in then, not after.)

Sequencing note: `on_pre_compress` was done first because §7.4's
compaction-recall eval harness already exists to measure whether it works.

---

## 8. What NOT to change in Model Plane
Two decisions already made deserve explicit protection from well-meaning
"simplification" during any adoption work above:

- **Keep the two tool-dispatch loops separate** (`dispatch_tool` vs.
  `execute_step_inner`, §4.1/§5.4). The HARN-1/2 withdrawal already settled
  this with evidence; don't relitigate it while wiring in parallel dispatch or
  a new sandbox interface — both can be added to `execute_step_inner` without
  touching the plain-chat router.
- **Keep `letta-bridge` as a single stub** until a second real memory backend
  exists (§7.7).

---

## 9. Onboarding artifacts produced alongside this audit
As part of this pass, onboarding material was generated for all four
codebases (per the `codebase-onboarding` skill format):

| Repo | `CLAUDE.md` | `ONBOARDING.md` |
|---|---|---|
| `apps/Model Plane` (this repo) | **written** (none existed) | **written** |
| `deepseek-harness` | *left untouched* — an excellent, actively-maintained `CLAUDE.md`/`AGENTS.md` already exists upstream | **written** (new, additive) |
| `hermes-agent` | **written** (none existed) | **written** |
| `claude-code-fork` | **written** (none existed) | **written** |

`deepseek-harness/CLAUDE.md` is a real, upstream-maintained file (literally
documented as a symlink to `AGENTS.md`) — it was deliberately not modified;
only a new, additive `ONBOARDING.md` was added there.

---

## 11. Addendum (2026-08-16, later pass): Verevon v3 + Model Plane vs. the "browser-web-app-first" bar

Prompted by a follow-up question: since Verevon v3 (`apps/Frontend Plane/verevonv3`)
is explicitly meant to be a **browser-web-app-first** surface (unlike Claude
Code/Hermes, which are terminal-first with secondary surfaces, or DeepSeek
Harness, which treats `dsh-web-app` and `dsh-headless` as equal siblings of one
core), this addendum checks how far the actual frontend + gateway + Model
Plane's SSE exposure layer are from that goal, verified against source.

### 11.1 Verdict
**The runtime/observability half of a browser-first agent web-app is real and
substantially more complete than a skeptical read expects. The authoring half
does not exist yet, and is honestly labeled as not existing.** Three
independent research passes (frontend SPA, the Rust gateway's domain wiring,
and Model Plane's SSE/`InvokeRequest` exposure layer) found:

- Chat streaming, tool-call visibility (humanized, expandable cards — not raw
  JSON), artifact rendering (including a deliberately sandboxed HTML iframe),
  HITL approval (in **two** places: chat and the Agent Run Console, both
  hitting the same real backend route), subagent-attach/stop visibility (also
  in two places), a real Agent Run Console with run history + proof bundles +
  resume/cancel, live memory/skills CRUD (tucked into Settings, not the Agents
  area), and both a dedicated cost dashboard and inline per-turn cost/token
  display — are all **live-wired end-to-end**, not mocked, not decorative.
- The gateway's SSE proxying is genuinely well-hardened: true byte-level
  passthrough (no buffering/re-encoding), server-derived (not client-forgeable)
  org scoping, five independently-audience-bound delegated tokens minted
  per-request and never echoed back to the client.
- **Configuring or authoring a new agent/workflow from the browser and having
  it persist or execute is not possible today, on purpose.** `WorkflowBuilder`
  is a static design preview whose own code comment states the reason:
  "the model plane is an LLM agent loop, not an n8n DAG runner" — there is no
  workflow/DAG concept in the backend to bind a visual builder to. Similarly,
  `ChatbotStudio`'s preset-agent configuration UI is mostly a disclosed mockup
  because, per its own code comment, "there is no per-org agent-config store
  yet." Both use a consistent, deliberate `DesignPreviewBadge` "honesty
  contract" pattern rather than silently faking a save button.

  **Correction (same day, follow-up dig):** "no agent-config store" is too
  broad a reading of that comment. A real, mature per-org agent-config store
  already exists — Convex's `agents` table (`apps/Application Plane/convex-core/convex/agents.ts`),
  with full CRUD (`create`/`update`/`getById`/`listByOrg`), a public-embed-widget
  security model (rotating secret, 403 without it), and fields for
  model/temperature/systemPrompt/tone/greeting/tools/knowledgeSources. What's
  actually missing is narrower: (1) `ChatbotStudio.tsx` simply isn't wired to
  that store yet (zero data-fetching imports found), and (2) the specific thing
  `AgentsPage.tsx`'s "Blueprint" badge names as Phase-5-deferred is an
  *activation pathway* — turning a role template (support/sales/marketing/...)
  into a real, capability-granted, deployed `agents` row — which is a
  genuinely new, smaller, and more precisely scoped piece of work than "build
  a config store from nothing." See the chat discussion for the full breakdown.

### 11.2 Backend landmines that cap what the frontend can ever show
Two findings from the SSE/`InvokeRequest` pass matter specifically for the
browser-first goal, because no amount of frontend work can surface a
capability the backend never actually emits:

- **`ReasoningDelta` is dead.** The SSE event is defined, feature-gated, and
  already has a client-side dispatch case (`chat-client.ts`) — but is never
  constructed anywhere in `model-gateway` outside its own unit tests, because
  `inference-core` has no reasoning/thinking-token channel at all. A browser
  user will never see a live "thinking" stream today, even though the wiring
  for it already exists on both ends.
- **Two structurally different `InvokeRequest` types share one name.** The
  protobuf message (with the chat-parity audit's added `content_parts`/
  `tools`/`attachments`/`features` fields) is consumed only by the gRPC
  transport on `:9090`, which never reads those four fields. The browser-facing
  HTTP path (`/v1/invoke/stream`, what the SPA actually calls) uses a
  completely separate hand-written struct where `tools`/`attachments`/
  `features` *are* real, but `content_parts` has no equivalent at all —
  multimodal image input goes through a narrower, differently-shaped
  `attachments` + `vision.rs` path instead. Anyone reading only the proto file
  would wrongly conclude multimodal input is fully wired; anyone reading only
  `grpc.rs` would wrongly conclude the chat-parity fields are unimplemented.
  Worth a docs correction and, longer-term, collapsing to one `InvokeRequest`
  shape.
- A built-but-unused surface: `ag_ui.rs` (an AG-UI/CopilotKit-protocol SSE
  adapter) omits the org/user headers and delegated tokens the sibling chat
  route treats as mandatory — but has zero callers anywhere in the shipped SPA,
  so it's an inconsistency to fix or delete, not an active exposure.

### 11.3 How this stacks up against the three external systems' web surfaces
None of the three external systems are a fair apples-to-apples comparison for
a *browser* agent web-app specifically:
- **DeepSeek Harness**'s `dsh-web-app` bundle is real but its depth wasn't
  profiled in this audit (out of scope — the harness comparison focused on the
  core/loop/sandbox packages, not the web-app bundle's UI).
- **Hermes** doesn't clearly have a browser-based *agent* UI as a primary
  surface (its `web/`/`website/` pair read as a docs site in the exploration
  done for §2.4, not the agent interface itself) — Electron desktop + CLI +
  TUI + messaging gateway are its actual UI surfaces.
- **Claude Code (leaked)** is terminal-first with an IDE/remote bridge, not
  browser-first at all.

So Verevon v3 isn't behind any of the three on browser-based agent-runtime
observability — if anything, the combination of a real Agent Run Console with
proof bundles, dual-surface HITL approval, and dual-surface subagent
visibility is a *more* complete browser agent-observability surface than
anything confirmed in the other three systems' web/GUI layers. The gap is
narrower and more specific than "we're behind": it's "authoring has no backend
yet," which is a Model Plane backend-scope decision (§6, §7), not a frontend
shortfall.

### 11.4 What this means for prioritization
Items already in §7 gain sharper justification from this pass:
- **§7.2 (parallel tool-call dispatch)** and **§7.3 (leaf/orchestrator subagent
  split)** now have a concrete frontend consumer waiting for them — the Agent
  Run Console and `ChatLiveRunPanel` already render subagent-attach events and
  would show richer orchestration immediately once the backend supports it.
- **New, frontend-driven item: give `sandbox-manager`/`execution-core` a
  workflow/DAG-and-agent-config persistence layer before investing further in
  `WorkflowBuilder`/`ChatbotStudio` UI.** Building more UI on top of these two
  features would be pure UI investment with no backend to bind to — the
  current `DesignPreviewBadge` discipline is the right call until that
  changes, not a shortcut to remove prematurely.
- **New item: fix or delete `ag_ui.rs`.** It's a real, distinct code path with
  a security-hygiene gap (missing org/delegated-token headers) and zero live
  callers — cheap to close either direction, but leaving it as-is is the worst
  of both options (attack surface with no product value).
- **New item: resolve the two-`InvokeRequest` naming collision** — at minimum
  a doc correction so `content_parts`'s proto-only status doesn't get
  mis-cited as "multimodal is wired" in a future audit; ideally collapse to one
  shape so the gRPC and HTTP transports stop drifting independently.

---

## 13. Second pass (2026-08-17) — four harnesses, new lenses, plus `pi`

Commissioned with an explicit priority order and a lens per system. `pi`
(`/Volumes/Lagring/Triodelab/pi`, earendil-works, **MIT**) joins as a fourth
harness — previously named in `external-ideas-harvest.md` but never actually
read. Chat-UI-specific conclusions live in `VEREVON_CHAT_DESIGN.md`; this
section covers the harness/architecture findings.

### 13.1 `pi` — the versatility is real, and so is the trap

**~256K LOC TypeScript, 12 packages, MIT.** The founder's read ("most versatile,
steep learning curve, a Claude Code competitor for expert users") is accurate,
and the mechanism is one architectural bet: **keep the core tiny and make the
extension API almost as powerful as the core**.

pi ships **exactly seven built-in tools** (`read`, `bash`, `edit`, `write`,
`grep`, `find`, `ls`) and **no MCP, no subagents, no permission prompts, no plan
mode, no todo list**. A grep across every package confirms zero MCP
implementation. Everything competitors ship as a built-in, pi ships as an
example extension you copy — 60+ working examples covering plan mode, subagents,
permission gating, sandboxing, git checkpointing. (The documented extension table
was diffed against the filesystem: every documented example exists. No doc drift.)

**The self-extension loop is the genuinely novel part**, and it's a packaging
decision plus a prompt trick: `package.json` ships `docs` and `examples` in the
npm `files` array, so an installed pi carries its own reference material on
disk; the system prompt then hands the model absolute paths to that material
with a topic-routing table. "Write me an extension that does X" makes the agent
read its own docs with its own `read` tool, write a `.ts` file, and `/reload`.
**The agent extends itself using only the file tools it already has.** There is
no plugin SDK ceremony because the plugin API *is* the internal API.

**The steep learning curve is not config and not a DSL — it's that you write
TypeScript against a large, unforgiving API with no guardrails.** Four things:
a ~40-event typed `ExtensionAPI`; an unusual mental model (the session is a
*tree*, not a log, with fork/branch/compaction-boundary reasoning and an
explicit `invalidate()` that poisons a stale captured context); no safety rails
to lean on (no permission system, no core sandbox, `bash` has **no default
timeout**); and undifferentiated power — a project's `.pi/extensions/*.ts` is
arbitrary code executed on session start, so *cloning a repo and running pi in
it is a code-execution event*. The only mitigation is a binary per-directory
trust store.

**Best portable assets** (all reimplementable freely — MIT, and architecture
isn't copyrightable anyway):
- **The no-throw stream envelope** (`lazyStream`) — confirmed and *understated*
  in our prior note. Every provider call returns a stream synchronously; unknown
  provider, unconfigured auth, OAuth refresh failure and transport failure all
  become a terminal in-band error event carrying a well-formed zero-usage
  message. One error path instead of two; cost accounting never has a hole.
- **The context-overflow detection table** — 25+ provider-specific regexes *plus*
  a rate-limit exclusion list (so Bedrock throttling isn't misread as overflow)
  *plus* two silent-overflow heuristics (one provider accepts overflow silently;
  another truncates and returns a length-stop with zero output). This is months
  of operational scar tissue available for free.
- **Session-as-tree with append-only compaction** — record the summary and the
  first-kept-entry id; never delete summarized entries. Branch/fork becomes a
  real feature *and* the untruncated transcript stays available for audit,
  export and erasure after the model's view is compressed. Directly serves our
  GDPR posture.
- **Fail every tool call in an assistant message whose stopReason is `length`,
  without executing any.** Streamed tool arguments are finalized by a salvage
  parser, so a truncated message can yield calls that parse and schema-validate
  while being silently incomplete. This is a real data-corruption class.
- **Two distinct queued-input semantics** (`steer` = inject after current tool
  calls, before next model call; `followUp` = wait until the agent would stop,
  then restart) with sending-during-stream-without-specifying treated as an
  **error, not a silent default**.

  ✅ **Adopted 2026-08-24, as a THIRD semantic rather than these two.** The
  owner's call: deliver at the tool-round boundary **as a pause** and let the
  model classify — redirect now, or finish the current task first. pi makes the
  *caller* choose, and the caller cannot know: whether a message redirects the
  work or merely follows it is a property of what it says. The premise was also
  worse than "silent default" — the SPA's send path *discarded* anything typed
  while streaming (`if (!content || state.status === 'streaming') return`), so
  the message was not queued, not refused, and not shown as rejected. See
  `model-gateway/queued_input.rs` and the plan's 4.3.
- **Compositional system prompt** — each tool carries its own snippet and
  guideline bullets; the builder assembles from whatever tools are active. The
  only maintainable approach once tool availability varies per tenant/plan/Space.

**⚠️ Major skeptical finding: pi contains a large, well-specified, well-tested
second architecture that is almost entirely non-functional.** `AgentHarness`
(`packages/agent/src/harness/`) has a 2,941-line spec, ~10K LOC of substrate and
~5.7K LOC of tests — and **every lane operation returns a promise rejected with
`HarnessNotImplemented`**; only `getLeafId()` works. `packages/server` is
imported by no source file. `pi server`/`pi client` subcommands are fully
implemented and unit-tested but **unreachable** (never referenced by the arg
parser). The 667-line crash-recovery reducer has zero callers. The telemetry
package is consumed only by the unwired harness, so **pi as it actually runs
emits no spans**. Its top-level test file is literally named
`agent-harness-scaffold.test.ts`.

Treat `harness.md` as an excellent *design reference* — its three-store
durability split, single-transaction primitive, "the durable program counter is
one register holding complete total state", and the effect-sandwich commit
protocol are a markedly simpler crash-recovery story than event-log replay, and
answer "what happens when the pod restarts mid-tool-call". But it is **not**
evidence of a shipping capability. Also: `tui-plan.md`, despite 36KB at repo
root, is a terminal layout-engine handoff with no bearing on a web product.

### 13.2 DeepSeek Harness — the web-app layer (closing pass 1's gap)

Pass 1 explicitly skipped the web-app bundle. It is the most directly applicable
material of the four systems, and the strongest single idea is:

**`ConversationNodeDefinition` + keyed renderer registry** — a plugin contributes
a new *renderable message type* to the browser chat UI, rather than growing a
central switch. Their acceptance gate is a three-path equivalence test (full
replace / prepend older page / live append must produce identical state) for
every new node type.

**Tool presentation as a pure, non-persisted, card-tagged intent owned by the
tool** — `generic` / `terminal` / `diff` / `read` / `search`, carried as an
optional sidecar on the tool event, with a documented degradation to flattened
text. The rule worth copying verbatim: **never persist the view; recompute at
emission, so improving a card retroactively improves all history.**
`truncated`/`total` on search results is non-negotiable — it stops the UI
presenting a capped result as complete.

Also: one **generic keyed projection channel** (`{key, value, seq}`,
higher-seq-wins, absence-of-key means "feature not composed → render nothing")
replacing bespoke live-state channels; a **connection-generation + strict
readiness handshake + gap-repair triad** that directly addresses our recorded
"stale result lands on the wrong thread" bug class; **approvals in the composer
via a self-nominating chain slot**; and a **wire-identity lookup seam as the
single tenancy chokepoint** — one central resolver where org-scoping lives,
instead of every handler. Given we have found cross-tenant IDOR twice, that last
one is the highest security-value item on the list.

**⚠️ Do not port their security posture.** `userId`, `tenant` and `principal`
appear nowhere in the web packages; the README says outright the fence is a
*reachability* policy, not authentication. One mux stream broadcasts every
session in the process to every connected browser (fine for one user, a
cross-tenant leak shape for us). The `Host` header is used as a boundary. The
plugin manifest and chunks are served **unauthorized**. `--host 0.0.0.0` is
hard-refused at parse, which means **nothing in the repo has ever been exercised
as a network-exposed multi-client server** — no TLS, no auth, no origin policy,
no load testing of the mux fan-out. Also: `session.history` may *create or
resume an agent just to read a transcript* (a trivial resource-exhaustion vector
for us — a read must never allocate a runtime), and boot is all-or-nothing (one
bad feature bundle = full outage; our shell must render core chat with degraded
features).

### 13.3 Claude Code — the industry-standard patterns, and where they're theatre

The most-copied patterns, with the honest caveats:

**Plan mode is enforced only by prompt text.** There is no plan-mode write block
in the permission engine — read-only-ness is a re-injected system reminder plus
ordinary asking behaviour. Worse: **when plan mode is entered from a
bypass-capable context, the permission check auto-allows every tool while still
telling the model it is read-only.** For a multi-tenant product exposed to
untrusted content that is a prompt-injection hole, not a safety mode. *Enforce
plan mode server-side with its own action allowlist; keep the reminder text only
as a redundant hint.*

**Two live task systems with an inverted enablement flag** — `TodoWrite` is
registered but its `isEnabled()` is the negation of the Task-v2 gate, which is
on for every interactive session. The famous tool is **dead in the interactive
product**. Skip TodoWrite v1 entirely; go straight to a durable task store with
`blocks`/`blockedBy` edges and a live subscription, and never render task updates
as chat messages.

**UI compiled out of the build but still reading as wired** — the
skill-improvement survey and the classifier-reviewed approval option sit behind
build-time constants that are false externally. Grepping finds a
complete-looking implementation no external user can reach. A discipline
warning for our own codebase, and the same failure class as our `ReasoningDelta`.

Patterns worth taking — the first three are now **taken** (2026-08-24), see the
note after this paragraph: the **compound plan-approval control** (approve +
choose post-approval autonomy level + choose fresh-context, with context-used %
printed in the option label; rejection requires free text fed back to the
model) — though *split it into a primary action plus secondary toggles in a
browser, where you have layout room*; **approvals as a durable addressable
queue, not modals**, routed to per-action-type components rendering the real
diff/payload; **"don't ask again" that shows and lets the user edit the exact
rule** with its scope named, org-scope being an audited admin-gated policy
change; a **context inspector** itemising the window by category computed
against what the model will actually see post-compaction; **skill discovery
capped at ~1% of the window** with description caps and degradation before
dropping entries; **`context: 'fork'` skills** that run in a sub-agent with
their own budget; and **@-mentions resolved to typed attachments at send time
under current permissions** rather than inlined at type time (which is what
lets you re-check tenant ACLs) — with resolution failures **visible on the
chip**, since silent drops make the model answer about content it never
received.

> **Status 2026-08-24 — three of these are now built, one deliberately not.**
>
> - **Compound plan-approval control ✅.** `mp_contracts::autonomy` holds the
>   ladder (`read_only | workspace_write | danger_full_access`, strictly
>   ordered), the strictly-wider escalation table, and the required
>   justification; `POST /v1/runs/:run_id/plan-approval` → BFF → SPA
>   `PlanApprovalControl` is the reachable surface. The premise was the same
>   failure class this section warns about: `ExitPlanMode` was served over gRPC
>   with **no production caller**, and `ApprovalKind::Plan` was only ever mapped
>   in conversion helpers, never produced — plan mode could be entered from the
>   composer and never left. Enforced **per call at execution** with the call's
>   arguments, never in a tool schema, because "schemas are registry-global
>   while the effective mode is per-call truth".
> - **Context inspector ✅** (2026-08-22) — `GetContextAssembly` had itemised the
>   window all along and model-gateway already called it to BUILD prompts; it
>   was simply never exposed.
> - **Skill discovery capped at ~1% of the window ✅** (2026-08-22) —
>   `fit_skill_blocks`, 8 000 chars, in BOTH loops, with an explicit truncation
>   marker because a silently truncated *instruction* is worse than a dropped
>   one.
> - **"Choose fresh-context" — deliberately NOT part of the approval control.**
>   The autonomy level is a graded grant a person makes; fresh-context is a
>   property of an *iterative* loop, and ours runs a delegated child once to
>   completion. Bundling them would put a toggle in the approval dialog that
>   changes nothing. Revisit with the typed handoff report — see the plan's 3.3.
>
> The rest of the paragraph above (durable approval queue, editable "don't ask
> again" rules, `context: 'fork'` skills, @-mention resolution at send time)
> remains open and is NOT claimed here.

Anti-patterns: an **LLM call on the permission hot path** (deriving the reusable
"don't ask again" prefix needs a model round-trip; the code itself warns when it
exceeds 10s); **reminder-injection as the universal steering tool** (plan mode,
auto mode, todos, tasks and memory each inject hidden per-turn messages with
their own throttles and mutual suppression — build one arbitrated attachment
pipeline with priority and a per-turn budget instead); **command surface
sprawl** (~101 command files, many one-off internal tooling — keep the palette
small, push the rest into skills); and **plan artifacts keyed by a random word
slug on a local filesystem path**, which survives nothing about a multi-tenant
multi-device product.

### 13.4 Hermes — how the agent learns, and why we must measure it

The learning design is genuinely thoughtful and the **governance patterns are
the real prize**:

- **A DO-NOT-CAPTURE list that is visibly an incident log** — no
  environment-dependent failures, no negative capability claims (which harden
  into refusals the agent later cites against itself), no self-resolved transient
  errors, no one-off task narratives, and never write up a sequence of failed
  attempts as a validated workflow. *Each of those should have become a test the
  moment it was learned.*
- **A strict preference ladder for writes** — patch the item loaded this session,
  else patch an existing umbrella, else add a typed support file, and only then
  create a new top-level item, with an explicit ban on names that only make
  sense for today's task. Without this a self-writing library fragments into
  hundreds of unfindable single-session entries.
- **Three-tier library with capture always in the private tier** —
  vendor/global (read-only, shipped by us) → org (proposed, approved) →
  user/workspace (private draft). **Nothing is ever born org-visible**;
  promotion is a separate human act and auto-propose defaults OFF.
- **Fail-loud name collisions** — when an org item and a personal item share a
  name, neither silently wins; load-by-bare-name refuses as ambiguous.
- **Telemetry in a sidecar, never inside authored content** — and for a
  regulated tenant, extend the field set with `source_conversation_id`, `org_id`,
  author, approval chain and an explicit retention class **at write time**,
  because GDPR erasure over free-text learned items cannot be retrofitted.
- **Full reversibility envelope for autonomous curation** — snapshot before,
  archive-never-delete as the maximum destructive action, a dry-run producing an
  identical report, and a per-run before/after diff with a rename map.
- **Zero use count is absence of evidence, not staleness**; and exempt anything a
  scheduled job depends on from usage-based aging (the scheduler only bumps
  usage when a job fires).

**⚠️ The flagship learning feature ships unmeasured.** The evals directory holds
exactly two harnesses (compaction, readtool); neither touches skills or the
review loop. Nothing anywhere answers *"does the learned library make the agent
better."* The entire acceptance bar is a ~4,000-character prompt string —
unversioned, untestable, un-A/B-able. **We must not build an org learning loop
we cannot defend with a number in a renewal conversation.**

Two further warnings that apply directly to us: **meter and attribute the
learning loop per tenant before shipping** (Hermes's review fork silently
doubled per-turn main-model spend and was invisible to usage analytics because
it ran with persistence disabled); and **there is no safe automatic path from
one tenant's learned item into another's context** — even ostensibly anonymized
procedural knowledge leaks (a workflow naming a specific ERP module, an approval
chain, a counterparty). Cross-tenant benefit must be a curated, human-authored,
vendor-owned template library merely *inspired* by aggregate telemetry.

On **community**: split *install* from *load* as the governance boundary (an org
admin decides which packs are installed; the agent decides per-conversation
which to load), adopt the **hash-the-declared-set consent pattern** (record a
hash of what the user saw when granting; an update declaring a different set
leaves additions ungranted until re-consent on a visible diff), and **apply the
content scanner to agent-authored items, not just imported ones** — a learned
item is executable instruction, so injection into the library is injection into
every future session in that tenant. On **gamification**: build a per-**org**
capability-adoption map, keep only the "what counts" disclosure, the pure
read-model derived from existing telemetry, and secret-until-first-signal;
discard tiers, leaderboards, share cards and any per-user activity score — in a
Norwegian works-council context, an artifact ranking one employee's activity
against another's is a concrete labour-relations problem.

### 13.5 Revised priority for Model Plane after pass 2

Pass 1's §7 stands. Pass 2 adds, in order:

1. **The no-throw stream envelope + overflow-detection table** (pi) — small,
   self-contained, immediately reduces a whole class of streaming failure. —
   🟡 PARTIALLY DONE (2026-08-22): investigated with a dedicated read of
   inference-core's streaming path before assuming the gap was real (it
   mostly wasn't the "no-throw" half — model-gateway's `sse.rs` already
   converts every inference-core failure into an honest in-band `error`
   event; the docstring workaround at `sse.rs:1470` proves this was already
   deliberately handled). What WAS genuinely silent: a mid-stream provider
   disconnect produced a well-formed `done: true` chunk **indistinguishable
   from a natural completion** — `InferChunk` had no finish/stop-reason field
   at all, so a truncated streamed answer looked identical to a complete
   one. Closed by adding `InferChunk.stop_reason` (proto + both provider
   adapters + the gateway's own `InvokeChunk` mirror), populated from each
   provider's real streaming finish-reason field (verified against
   developers.openai.com's and platform.claude.com's current streaming
   references via `ctx7`, not assumed) and set to a new `"stream_incomplete"`
   value specifically when the connection broke before any proper
   termination signal arrived. Both call sites that consume it
   (`grpc.rs`/`sse.rs`) now log a warning on that value, so an incomplete
   answer is at least discoverable instead of silent. 2 new unit tests (one
   per provider) verify the JSON-path extraction directly. **Deliberately
   NOT done**: threading `stop_reason` all the way into the client-facing
   `SseChunk` JSON (an ~18-construction-site shared struct spanning several
   unrelated streaming features — chat, deep research, image gen — a
   separate, larger change, not a silent gap in a security/correctness
   sense since the signal is now captured and logged server-side). Also NOT
   done: pi's 25+-regex overflow-detection table stayed at `compaction.rs`'s
   existing ~18-pattern version (already the right shape, just smaller) and
   a typed `ProviderError::TooLong` variant — the string-matching approach
   works today and widening it is a incremental, lower-urgency follow-up,
   not blocked on anything.
2. **Tool-presentation intent + `ConversationNodeDefinition`** (DeepSeek) — the
   prerequisite for the chat redesign in `VEREVON_CHAT_DESIGN.md`. — ⏸️
   correctly deferred (2026-08-22), not built.

   Checked `VEREVON_CHAT_DESIGN.md` before touching anything: it already
   specs this exact pattern (`ConversationNodeDefinition` → `src/shared/
   chat-nodes`, table row citing DeepSeek by name) as ONE piece of a larger,
   still **"Status: Proposed"** redesign spanning the chat feature, the
   gateway, and Model Plane exposure. Its own I2 invariant ("a thread created
   outside the chat page never appears in the chat thread list... enforced by
   a first-class `origin` dimension in session-core") is exactly what this
   session's Track 1 work built. Building `ConversationNodeDefinition` in
   isolation, ahead of a decision to actually greenlight the rest of that
   redesign, risks producing a frontend architecture piece shaped by guesswork
   rather than by the redesign's own eventual specifics — and it's a
   SolidJS/TypeScript frontend-architecture change, a different domain from
   this pass's Rust/Go work. Left for whenever `VEREVON_CHAT_DESIGN.md` itself
   moves from Proposed to underway.
3. **Server-side plan-mode enforcement** (avoiding Claude Code's hole) — we
   already have a capability-policy gate; plan mode should be an allowlist over
   it, not a prompt. — ✅ DONE (2026-08-22)

   **Investigated first: Model Plane had the EXACT same hole**, just not yet
   exploited. `plan_mode` existed only as a durable STATUS flag
   (`model-gateway`'s `PlanModeStore`/session-core's `run.mode`, queryable via
   an `IsPlanMode` RPC) so a UI could show "this run is planning" — but
   `coordinator.rs`'s own doc comment already promised more: "write-class
   tools are **expected** to gate via `is_plan_mode`." No code ever did.
   `execution-core` (the deployed-agent loop `plan_mode` actually needs to
   restrict) had ZERO references to it at all — `RunAgentRequest` didn't even
   have a field for it, so there was no wire path for the flag to reach the
   loop that dispatches tools. A run a human believed was "planning" could
   still execute a real destructive action if the run's own `mode` was
   `auto`. (Model-gateway's plain-chat path was accidentally safe regardless —
   `dispatch_tool` already refuses every side-effecting tool unconditionally,
   with or without plan_mode — so the live exposure was narrower than Claude
   Code's, but the missing enforcement itself was identical.)

   **Closed** by adding `RunAgentRequest.plan_mode` (proto), threading it from
   the original chat request through `model-gateway`'s `agentic_run_stream` →
   `spawn_run_dispatch` → execution-core, and enforcing it in `run_rounds`'s
   sequential pre-pass — reusing the EXACT same pattern and classifier
   (`permission::is_risky_call`) as §7.3's leaf/orchestrator blocklist, added
   the same session: plan mode refuses any risky/side-effecting tool
   outright regardless of the run's own permission mode; read-only tools
   still run; `subagent.*` is exempt (delegating an investigation is not an
   action); and because a delegated loop inherits `req` verbatim, plan mode
   propagates to every subagent automatically with zero extra wiring. 1 new
   test (top-level, `mode: "auto"`, deliberately NOT `"ask"` so the
   pre-existing approval gate can't mask whether the new check actually
   fired). Full workspace green, no new clippy findings.
4. **Session-as-tree + append-only compaction** (pi) — unlocks branch/fork *and*
   strengthens the GDPR/audit story. — 🟡 the audit half is already true;
   branch/fork is a net-new product feature, not built (2026-08-22).

   The "append-only, nothing summarized is ever destructively lost" half of
   this pattern is **already Model Plane's architecture, independently of
   this recommendation**: `session-core`'s `events` table is append-only by
   design, and `compaction.rs::compact_once` is INSERT-only (a PG-verified
   regression test, `compaction_is_additive_never_deletes_events`, locks this
   in). `model-gateway`'s own compaction (§7.5) only ever rewrites the
   OUTGOING prompt assembled for a given inference call — it never touches
   the underlying stored events. So "the untruncated transcript stays
   available for audit, export and erasure after the model's view is
   compressed" is already true today, achieved by a different mechanism
   (durable append-only event log) than pi's (an explicit tree with
   first-kept-entry pointers) — same property, already satisfied.

   **What pi's pattern would actually ADD is branch/fork itself** — a user
   returning to an earlier point in a conversation and continuing down a
   different path as a first-class, addressable feature. That is a genuine
   net-new product capability, not a gap in the current architecture, and
   deciding whether/how Verevon should expose branching (a UI concept, a
   thread-relationship model, how it interacts with the `origin`
   dimension Track 1 built this session) is a product decision on the same
   footing as §13.5 item 2's chat redesign — left there rather than invented
   here.
5. **The wire-identity tenancy chokepoint** (DeepSeek) — one resolver to audit
   instead of every handler. — ✅ VERIFIED ALREADY SATISFIED (2026-08-22), no
   new code needed.

   The highest security-value item on the list deserved the most rigorous
   verification, not the most code. Traced both gateways end-to-end rather
   than assuming a gap: **model-gateway** decodes the ingress JWT exactly
   once (`auth.rs::require_auth`), and all seven `Verified*Bearer` types are
   thin wrappers over the one `decode_delegated_bearer` primitive, all
   funneling through the one `claims.org_id != model_claims.org_id` check
   (`auth.rs::verify_delegated_user_bearer`); every one of the 48 real gRPC
   RPC handlers calls the same generic `authorize_rpc<T: TenantScopedRequest>`
   (checked programmatically against every `impl ModelGateway` method — 48/48,
   the only 3 non-callers are internal helpers and the healthcheck).
   **Frontend Plane's gateway** shows the identical shape: one
   `require_session` → one `authorize_request` → one `authorized_org_id`
   resolver, with the two apparent "duplicates" confirmed to be one-line
   pass-through wrappers, not reimplementations.

   This is not a coincidence: this repo's own two historical cross-tenant
   IDOR fixes (`ff2a3f59` — six gateway domains each had their own
   `org_id_from_headers` reading a client-controlled header, deleted in favor
   of the one `authorized_org_id` resolver; `e26c724f` — an approval RPC
   family carrying no `org_id` at all, fixed by binding scoping to the
   verified bearer) are the exact incidents that already drove this codebase
   onto the chokepoint architecture DeepSeek's pattern recommends. The
   pattern is already load-bearing production code, not a documentation
   aspiration — a genuinely different outcome from §7.1's and §13.5 item 3's
   "documented intent, never wired" findings, and worth stating precisely so
   a future security review doesn't re-flag it as a gap.
6. **Learning loop: governance first, feature second** (Hermes) — the three-tier
   library, sidecar telemetry with retention class, and a measurement harness
   must exist *before* the learner does. — 🟡 investigated, one real gap
   confirmed and flagged, not unilaterally built (2026-08-22)

   G7 (the closed learning loop: `RUN_COMPLETED` → LLM review → persist →
   read → `MatchSkills`) already exists and already has real, working
   governance pieces: a confidence floor (`MinConfidence = 0.5`), DB-level
   provenance protection (a `background_review` candidate can never overwrite
   a `user`-authored skill — `skipped_protected`), and content-hash dedup.
   Also confirmed: a PRIOR session already investigated and correctly
   *removed* capability-core's `PromoteSkill` RPC as dead theater (per
   `QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md` SKILL-2) — it mutated a
   `Capability.Scope` rollout label that `policy.Engine.EvaluateCapability`
   never actually read, so it was never a real gate to begin with. Removing
   a fake gate was the right call, not a regression.

   **The real gap Hermes's governance pattern surfaces**: nothing replaced
   it. `session-core::list_agent_skills` returns every row for an org
   unconditionally — `origin` distinguishes provenance for overwrite
   protection, but nothing filters `MatchSkills`'s surface by it. A
   `background_review` candidate that clears the confidence floor is
   **immediately org-wide-matchable with zero human review** — precisely
   the anti-pattern Hermes's own governance write-up warns against most
   directly ("nothing is ever born org-visible; promotion is a separate
   human act and auto-propose defaults OFF").

   **Not built, deliberately**: a real replacement promotion gate needs a
   private/draft tier, a review surface a human actually uses, and a default
   posture (auto-propose off unless an org opts in) — genuine product and UX
   decisions (what does "review a learned skill" look like? who has that
   role?), not a backend patch. Building one unilaterally here would risk
   repeating exactly the mistake the removed `PromoteSkill` RPC was: a gate
   that exists in code but was never actually reasoned through as a real
   control. Also confirmed still true: no measurement harness exists for
   whether the learned library makes the agent measurably better (the audit's
   own "we must not build a learning loop we cannot defend with a number"
   concern) — `eval-lab-py` (§7.4) has no skills-quality case type yet, and
   adding one meaningfully depends on the promotion-gate design above (what
   are we measuring — raw candidates, or only promoted ones?).

---

## 14. Sources
- This repo: `docs/GOAL.md`, `docs/ROADMAP.md`, `docs/gap-analysis.md`,
  `docs/gap-model.md`, `docs/external-ideas-harvest.md`,
  `docs/capability-ownership-matrix.md`, `docs/chat-parity-audit.md`,
  `docs/core-research/*.md`, `MODEL_PLANE_STATUS.md`, `MODEL_PLANE_ROADMAP.md`,
  plus direct source reads (`rust/services/execution-core/src/{sandbox.rs,
  executor.rs,runtime_loop/agent.rs}`, `go/services/*`) and git history
  (`git log` for `apps/Model Plane`, commit `ac2be529` in full).
- `/Volumes/Lagring/Triodelab/deepseek-harness` — `README.md`, `AGENTS.md`/
  `CLAUDE.md`, `docs/architecture.md`, `docs/agent-lifecycle.md`,
  `docs/tool-execution-pipeline.md`, `docs/tool-catalog.md`,
  `docs/defensive-patterns.md`, `docs/postmortem/*`, `.agents/notes/*`, and
  direct package source reads.
- `/Volumes/Lagring/Triodelab/hermes-agent` — `README.md`, `pyproject.toml`,
  `AGENTS.md`, `docker/SOUL.md`, `docs/security/network-egress-isolation.md`,
  and direct reads of `agent/*.py`, `tools/*.py`, `plugins/*`.
- `/Volumes/Lagring/Triodelab/claude-code-fork` — `README.md`,
  `CODEBASE_ASSESSMENT.md`, and structural analysis only (directory listings,
  exported symbol names, import scans, line/byte counts) per the binding
  no-reproduction constraint (§1).
