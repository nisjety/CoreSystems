# Agentic-Engineering Audit — Prompt / Context / Harness / Loop (2026-07-17)

**Scope**: verify Model Plane support for the four agentic-engineering
disciplines, rank the gaps, and specify how to close them. Method: web research
on current definitions + a read-only code sweep across `go/services/*`,
`rust/services/*`, and `proto/`. Companion change landed the same day:
resume-safe evaluator-optimizer (commit `0175a957`), which is the plane's
loop-engineering reference implementation.

**Corrections to earlier audit records** (both verified in code):

- ⚠️ "MCP not wired" is **out of date**: execution-core offers MCP tools via
  `McpGatewayClient::from_env()` (`execution-core/src/runtime_loop/agent.rs:193`,
  dispatch `mod.rs:510-521 → ProxyMcpTool`); model-gateway owns the registry
  (`mcp_gateway.rs`, `mcp_jsonrpc.rs`). It is **env-gated** — unset ⇒ built-in
  toolset only.
- ⚠️ "skills injection does not work" is **partially out of date**: the
  model-gateway SSE chat path injects matched skills as system context
  (`sse.rs:667-690`, matcher `skills.rs:148-201`). But `SkillStore` starts
  empty (no disk `.md` loader; only learned skills lazily pulled from
  session-core), and the execution-core governed loop still injects nothing
  (`agent.rs:1518-1525` unimplemented). Works for *learned* skills in the
  *interactive SSE* path only.

## The four disciplines (definitions, sourced)

Nested layers of control, each wrapping the previous:

| Discipline | One-liner | Owns |
|---|---|---|
| **Prompt engineering** | the words you send | system prompts, templates, versioning, per-model adaptation, eval-gated iteration |
| **Context engineering** | everything the model sees | history assembly, memory, retrieval-into-context, token budgeting, compaction, just-in-time retrieval, sub-agent isolation |
| **Harness engineering** | the environment the agent runs in | tool loop, validation, permissions/policy, hooks, sandboxes, HITL, observability |
| **Loop engineering** | survival of the loop itself | durable execution: every turn/tool-call a checkpoint, resume from last successful step — never restart from zero |

Key loop-engineering pattern from the field: wrap every model turn and tool
call in a durable step (DBOS `runStep`, LangGraph checkpoints, Temporal
activities); on recovery, cached step results replay and the run resumes at
the interrupted step.

## Support matrix

| Discipline | Verdict | Strongest asset | Weakest point |
|---|---|---|---|
| Prompt engineering | **PARTIAL** | injection-defense framing (`retrieval.rs:395-405`) | no registry/versioning; governed loop has no authored system prompt |
| Context engineering | **PARTIAL** | `GetContextAssembly` budgeted evidence block (`sse.rs:1310-1420`) | no in-loop window management; heuristic tokenizer |
| Harness engineering | **SUPPORTED** | gate ordering: policy → PreToolUse → execute → PostToolUse (`runtime_loop/mod.rs:244-419`) | sandbox-manager is bookkeeping only; `execute_shell` runs unsandboxed |
| Loop engineering | **PARTIAL** | Temporal workflow layer + signal-durable approval pauses | agent compute restarts from turn 0 on crash; checkpoints written but never read back |

## 1. Prompt engineering — PARTIAL

**Have**: hardcoded `const` system prompts on gateway HTTP surfaces
(`http_routes.rs:1365,1717,2600`), browser-extraction prompt assembly
(`grpc.rs:1890-1899`), data-as-evidence injection defense
(`retrieval.rs:395-405`), moderation denylist (`moderation.rs:24-39`),
per-turn skill/verbosity system context (`sse.rs:667-690`, `verbosity.rs`).
`python/eval-lab-py/` exists but is standalone.

**Missing**: (1) prompt registry/versioning — prompts are scattered inline;
(2) the governed execution-core agent loop builds its message array ad hoc
(`agent.rs:293-327`) with essentially no authored system prompt (no
role/policy/tool-usage guidance); (3) eval-lab is not wired into prompt
iteration.

**To achieve fully**:
- Introduce a versioned prompt registry (capability-core is the natural owner
  — it already versions skills): `prompt_id@semver`, org-overridable, fetched
  by execution-core/model-gateway at run start and pinned per run for
  reproducibility.
- Author a real system prompt for the governed loop (role, tool-use policy,
  ZDR posture, output contract) and thread `prompt_id` into run records so
  cost/quality can be attributed to prompt versions.
- Gate registry changes on eval-lab: a prompt version promotes only when its
  eval suite beats the incumbent (same shape as the existing
  skill-promotion gate, `CheckSkillPromotion`).

## 2. Context engineering — PARTIAL

**Have**: `GetContextAssembly` folds retrieved/wiki/graph/memory segments
under a clamped token budget (default 4096, clamp 512–32768) with
`estimated_tokens` (`sse.rs:1310-1420`); recent-thread fallback capped at 24
messages (`sse.rs:1302`); letta-bridge memory read/write real e2e (in-memory,
Postgres `pgstore`, and vector `agentmemory`); tool outputs truncated to
4000 chars before re-injection (`agent.rs:102,667-684`).

**Missing**: (1) **no in-loop window management** — the `for _round` loop
(`agent.rs:312`) grows `messages` unbounded, no token counting/truncation/
summarization of running history (`session-core/src/compaction.rs` is
checkpoint-watermark rollup, not conversation compaction); (2) history
summarization exists only offline (orchestrator `SummarizeMemoryActivity`
hard-cuts at 512 chars); (3) tokenizer is a chars/4 heuristic
(`mp-toon/src/lib.rs:263`); the assembly budget is bypassable via the
recent-messages fallback.

**To achieve fully** (per current best practice: compaction + structured
notes + just-in-time retrieval + sub-agent isolation):
- Add in-loop budget enforcement in execution-core: count tokens per round
  (real tokenizer — tiktoken-rs or the provider's count endpoint via
  inference-core), and at a high-water mark run a compaction turn: summarize
  the oldest rounds into a pinned system note, drop the raw turns, continue.
- Reuse the same budget on the SSE fallback path so recent-messages cannot
  bypass the assembly clamp.
- Promote structured note-taking: the loop already persists checkpoints —
  extend the checkpoint payload with an agent-authored scratchpad the next
  round re-reads (this also feeds loop-engineering resume, below).
- Keep retrieval just-in-time: today's evidence-block design is right; the
  gap is only that nothing manages the conversation side of the window.

## 3. Harness engineering — SUPPORTED (one hole: sandbox)

**Have**: real bounded tool loop with purpose-lock allowlist and duplicate
suppression (`agent.rs:312-403`); gate ordering enforced before every
dispatch: capability policy → PreToolUse hook → execute → PostToolUse hook
(`runtime_loop/mod.rs:244-419`), policy fail-closed
(`capability_policy.rs:122-124`); hooks real (`hook/mod.rs` Allow/Deny/Ask);
HITL approvals enforced with durable-write-before-expose and a negative test
(`grpc.rs:232-248,1001`); risk-based policy engine
(`capability-core/internal/policy/engine.go:117-172`); browser grants with
TTL/revocation fail-closed (`browser-broker/internal/grant/grant.go`); MCP
wired env-gated (see corrections); tool errors captured per step; outputs
truncated.

**Missing**: (1) **sandbox-manager is not real isolation** — in-memory
lease/snapshot stores, synthetic `ObjectKey`, and `execute_shell` runs
`Command::new` locally in-process (`mod.rs:436` → `executor.rs`);
(2) sequential tool execution only — multiple `tool_calls` in a round run
one-by-one; (3) no tool retry on transient failure; (4) MCP dark unless env
is configured.

**To achieve fully**:
- Sandbox: back sandbox-manager with real isolation (Firecracker/runc or a
  remote executor pool); route `execute_shell`/code-exec through a lease so
  the existing lease/snapshot bookkeeping becomes real. Until then,
  policy-deny `execute_shell` outside dev orgs (fail-closed default).
- Parallel tools: dispatch independent `tool_calls` of a round concurrently
  (bounded `JoinSet`), preserving per-tool gate ordering; merge results in
  call order for determinism.
- Add a transient-retry envelope (single retry, jittered) around tool
  execution for network-class errors only; surface permanent errors to the
  model unchanged.
- Ship a default MCP env preset in deploy compose so the path is exercised
  (it is currently correct but dark).

## 4. Loop engineering — PARTIAL (workflow layer YES, agent compute NO)

**Have**: Temporal workflow layer replays history — approval pauses are
signal-durable (`interactive_run.go:102-127`); SSE stream resume via
Last-Event-Id replay buffer, Redis-backed multi-replica
(`orchestration_grpc.rs:38-214`); **per-step checkpoints are persisted** on
every `ExecuteStep` (`execution-core/src/grpc.rs:441-471` →
`session-core/src/grpc.rs:1644-1697`, idempotent `INSERT INTO checkpoints`);
evaluator-optimizer now checkpoint-per-leg durable (`0175a957`).

**Missing** (ranked):
1. `ExecuteStepLoopActivity` runs its whole MaxTurns loop inside one Temporal
   activity (`orchestrator-core/cmd/activities/activities.go:185-218`) — a
   worker crash retries from turn 0. No heartbeat/HeartbeatDetails.
2. execution-core's `for _round` loop state (`messages`, partial answer) is
   in-memory only; checkpoints are **write-only** — no LoadCheckpoint/restore
   consumer exists anywhere (only the proto enum `CheckpointRestored=111`).
3. `resume_run` is status-only (Gated→Running, `state.rs:105-112`); there is
   no "resume from turn N / last checkpoint" continuation descriptor (noted
   explicitly at gateway `grpc.rs:1315`).
4. Non-Redis deployments lose stream-resume across replica restart
   (in-memory replay buffer).

**To achieve fully** — the reference pattern is now in-tree
(evaluator-optimizer, `0175a957`): *the workflow owns the loop; every model/
tool leg is one activity = one durable checkpoint; leg request IDs are
deterministic so retries dedupe upstream; pure deterministic steppers shared
by both drivers with an equivalence test.* Apply it outward:

- **ExecuteStepLoopActivity** (follow-up task filed): add a single-step
  `ExecuteStepActivity`; move the MaxTurns loop into a shared workflow
  helper; migrate the five callers behind `workflow.GetVersion` so in-flight
  runs stay replay-compatible; keep the old activity registered for old
  histories; verify `replay_test.go`.
- **execution-core compute resume**: implement the missing checkpoint
  *consumer* — on `ExecuteStep` for a run with prior checkpoints, rebuild the
  message array from the last checkpoint (plus the compaction scratchpad from
  §2) instead of starting the round loop cold; emit `CheckpointRestored`.
  This converts the existing write-only durability investment into real
  resume.
- **Continuation descriptor**: extend `ResumeRun` with
  `{last_checkpoint_id, next_round}` so the gateway can resume compute, not
  just flip status.
- Require Redis replay buffer in production profiles (compose default).

## Priority order

1. Checkpoint **restore** consumer in execution-core (turns existing writes
   into actual resume; unblocks "resume no matter what").
2. Per-turn durable step loop in orchestrator-core (task chip filed;
   `GetVersion`-gated).
3. In-loop context compaction + real tokenizer (context engineering's only
   structural hole; also shrinks checkpoint payloads).
4. Sandbox realization or fail-closed shell denial (harness's one hole).
5. Prompt registry + eval-gated promotion (quality flywheel; composes with
   the evaluator-optimizer and eval-lab).

## Sources

- Atlan — Prompt vs Context vs Harness Engineering:
  https://atlan.com/know/harness-engineering-vs-prompt-engineering/
- Tosea — What Is Loop Engineering (prompt→harness progression):
  https://tosea.ai/blog/loop-engineering-ai-agents-complete-guide-2026
- MongoDB — The Agent Harness (durable execution, checkpoint-resume):
  https://www.mongodb.com/company/blog/technical/agent-harness-why-llm-is-smallest-part-of-your-agent-system
- Anthropic — Effective context engineering for AI agents (compaction,
  structured notes, just-in-time retrieval, sub-agents):
  https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- Faros — Harness Engineering: https://www.faros.ai/blog/harness-engineering
