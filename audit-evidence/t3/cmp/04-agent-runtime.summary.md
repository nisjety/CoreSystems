# T3 / Category 4: Agent runtime (cross-source comparison)

420 reference records from 12 sources: 415 sit in 77 capability clusters and 5 are excluded (retired features, vendor benchmarks, internal trivia). Verevon status by cluster: 15 shipped, 24 partial, 5 stub, 2 planned, 18 absent, 13 unknown. Tiers: 5 priority, 21 recommended, 23 nice-to-have, 13 skip, 15 n/a (strengths). Details are in `04-agent-runtime.clusters.json`. Provenance tags: SRC = read in source, DOC = documentation, INF = inference.

## Shared baseline (table-stakes, found in 7 or more sources)
- **Scheduled agent runs** (C-046, 10 of 12): cron/RRULE jobs that run as full agent runs and deliver results.
- **Plan mode with an approval gate** (C-024, 9 of 12).
- **Mid-turn steering and queueing** (C-003, 8 of 12).
- **Context compaction core** (C-020, 7 of 12): prune first, then summarise, or use the provider's native compaction.
- **Subagent delegation with isolated context** (C-033, 7 of 12).
- Close behind (6 sources each): persistent goals, structured clarifying questions, cache-stable prompts, provider retry and failover, multi-agent coordinators, workspace checkpoints.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Typed turn log / state machine | DeepSeek (+ Antigravity step enum) | Each request is a pure fold over an append-only log; one logged end reason per turn (SRC q5) |
| Durable execution and resume | DeepSeek, Hermes | Durability points before model calls and effects; cold resume repairs the log tail; turn lease with a watchdog |
| Interrupt and tool-history repair | DeepSeek | Keeps the text the user already saw; every unanswered tool call is closed as aborted, not started or outcome unknown |
| Steering and queue | Claude Desktop | Delivery state machine: held steers, replay after a lost connection, uuid echo |
| Compaction | Cursor, Codex, Hermes | Self-summary that keeps the cache prefix, background summarisation, reference-only summary framing, verbatim user messages |
| Cache-stable prompts | Hermes, DeepSeek, Manus | Byte-stable system prompt; changing facts appended as deltas or wire-only sidecars |
| Prompt assembly | Cursor | System prompt built as a component tree with per-model gating, plus a typed RequestContext with hydration flags |
| Clarifying questions | Manus, Codex, Cursor | Typed question cards; sync and async asks; answers delivered durably after the turn paused |
| Goals | Codex | Persistent ThreadGoal with a token budget and an audited completion check |
| Scheduled runs | Hermes, Claude, Manus | Full agent runs, an at-most-once execution ledger, multi-surface delivery |
| Event triggers | Cursor, Manus | Trigger catalog, LLM filters, revisions, dry runs, run history |
| Loop hygiene | Hermes, Cursor | Detects repeats, cycles and no-progress; detects repetition in the streamed text |
| Code mode | Codex, Manus | Model writes JS that runs in a V8 isolate, with per-cell budgets |
| Exactly-once effects | Manus | Action ledger keyed by (session, actionId) with a payload digest |
| Permission policy | Claude Desktop | Host-side callback; forged approval cards rejected; reduced tools when no one is watching |

## Differentiators worth watching
Coordinator, "project" and swarm modes (Cursor, Antigravity, Perplexity Computer). Model-authored workflows (Manus Wide Research). Best-of-N runs (Antigravity). Monitor-mode cron that skips the model when the source has not changed (Hermes). Persistent named agents with their own email or computer (Manus, OpenAI aeon/Dots, Hermes bots). A post-turn learning fork (Hermes). An automated reviewer model (ChatGPT, DOC only).

## Verevon's biggest gaps
1. **Do-mode context seeding** (C-018, priority, sig 5): a Do run starts with only the preamble and the goal. It gets no thread history, persona, org or Space instructions, and no memory (agent.rs:1185-1200). Fix it by reusing Session Core GetContextAssembly.
2. **Continuation after approval or crash** (C-005, priority, sig 5): after approval, the LLM loop does not resume. Only 3 effect types are re-executed. The descriptor key is empty on the stack. Interrupted runs do not resume.
3. **Clarifying-question tool** (C-029, priority): absent (grep found no ask_user tool). The approval-pause plumbing can be reused for it.
4. **Persistent goals and long runs** (C-032, priority): absent. Long runs are also capped by the 300 s delegated bearer (OD8) and 4 default rounds.
5. **Scheduled runs are a stub** (C-046, priority): a scheduled run makes one no-tool, 1,024-token call. The owner ruling makes them draft-only, and no result is delivered.

Recommended next tier: plan approve-then-continue; durable queue, cancel and grant registries (all in memory today); a loop guard; output spill for every tool; hooks fed into Do mode; todo recitation; conversation fork and rewind; custom agent roles and a persistent-agent model for the Agents pages; event triggers through Ingestion (integration-corev2, never polled by the Model Plane); code mode through a gated sandbox bridge.

## Verevon strengths (keep)
- **Server-side plan-mode enforcement**: ahead of the prompt-only plan modes in Claude Code, Cursor and DeepSeek.
- **Capability gate**: every call needs Ed25519-signed evidence and unknown tools are refused (fail closed); the autonomy ladder is checked per call.
- **Deterministic postcondition verification**: produces a Proof Bundle.
- **Argument checks**: arguments are grounded, validated against the schema, and MCP arguments are repaired.
- **Compaction**: tiered, with need-driven reattach and a recall eval running in CI.
- **Durable subagent lineage**: with cold-resume read-back and atomic budgets.
- **Run records**: typed SSE taxonomy, terminalization receipts, drain on redeploy.
- **Server-side runs**: they continue after the client closes.

## Caveats
- 4 of the 12 sources are web-only benchmarks backed mainly by DOC records. Claude/web-claudeai and Manus/web-manus are the same vendors seen twice, so prevalence counts overstate breadth a little.
- C-031 (auto-review) and C-077 (record and replay) are backed only by DOC records. Their confidence is low.
- 13 clusters are "unknown" (no baseline record). Several conclusions rest on quick greps: no repetition guard, no goal primitive, no ask-user tool, Anthropic cache_control present. A later code check should confirm C-007 (provider failover), C-012 (streaming tool exec), C-004 (partial-text persistence) and C-028 (per-turn overrides).
- Some baseline evidence paths are written relative to `apps/Model Plane` or `apps/Frontend Plane/verevonv3`, or as `C:/dev/CoresSystem/...`, as recorded in the base file.
- The DeepSeek fork-subagent record (C-037) conflicts with Verevon's ledger note that DeepSeek rejected forks. This needs a check.
- Memory, skills and extensibility overlap with other categories. C-066 (learning passes) and C-015 (tool search, which is blocked because the capability gate denies MCP calls) are scored only on their runtime aspect.
