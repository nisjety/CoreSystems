# Cache + Conversation Compaction Audit — 2026-09-17

> **Execution update — 2026-09-19:** The design-only status below is historical. [Q01–Q04](PRODUCT_RECORDING_Q01_Q04_2026-09-19.md) records implemented capability selection, native summary round-trip and live threshold/restart verification. These are separate from performance certification: [Q08–Q10](PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) tracks measured latency and recovery. Do not reopen the original gaps without checking those results, or infer a speed target has passed from request acceptance alone.

Research and design pass. No code was changed as part of this document. Produced by a
5-researcher + adversarial-verify + synthesis workflow: 1 external best-practice researcher
(Anthropic/OpenAI/LangChain/Claude Code documentation) plus 1 internal architecture mapper,
feeding a grounded design proposal. All internal citations are file path + line number,
re-read fresh from `C:/dev/CoresSystem` on 2026-09-17.

## §0.0 — Original research status

Design only. Nothing below is implemented. See §7 for the priority-ordered action list.

---

## 1. External research: what the industry actually does (verified against primary sources)

- **Cache economics (Anthropic, confirmed from platform.claude.com):** cache reads cost **0.1x**
  base input price (a 10x discount); 5-minute cache writes cost **1.25x**; 1-hour writes cost
  **2x**. Up to **4 explicit breakpoints** per request (a 5th is a 400); minimum cacheable prompt
  length is model-tier-dependent (1,024 tokens for Sonnet-class, 4,096 for Haiku 4.5). Cache
  hierarchy order is **tools → system → messages**, and a lookback window of 20 blocks bounds how
  far back a breakpoint can match. `input_tokens` in a response counts only tokens *after* the
  last cache breakpoint — `cache_read_input_tokens` / `cache_creation_input_tokens` are reported
  separately.
- **OpenAI parity:** automatic caching (no explicit breakpoint needed) above 1,024 tokens, also a
  90% (0.1x) discount on cache-read tokens.
- **Compaction vs caching is a real, documented tension — not a Verevon-specific problem.**
  Anthropic's own Claude Code explicitly states that compaction "by design... invalidates the
  conversation layer, since the next request has a new, shorter history that doesn't share a
  prefix with the old one." The documented reconciliation: (a) the *summarization request itself*
  reads the pre-compaction prefix from cache, so a compaction turn "costs a fraction of what the
  context size suggests," and only the *following* turn pays to rebuild the (now much shorter)
  cache — a one-time, self-recovering cost, not a permanent regression; (b) the system prompt
  keeps its **own** cache breakpoint, independent of history, so it survives compaction untouched;
  (c) Claude Code's `/rewind` is offered specifically as a cache-preserving alternative to
  compaction when the goal is just discarding a bad path.
- **Anthropic ships two native, server-side features that do exactly what Verevon hand-rolls:**
  a **context-editing** API (`clear_tool_uses_20250919`) that clears stale tool results
  server-side (default trigger 100,000 tokens, keep last 3 tool-use pairs) — the same job as
  Verevon's own `clear_stale_tool_results`; and a **compaction API**
  (`compact_20260112`, beta) that summarizes full history server-side, defaulting to a
  150,000-token trigger (minimum 50,000), explicitly designed to coexist with system-prompt
  caching, and billed as a separate `usage.iterations` entry rather than folded into the turn.
- **No source uses a bare message count as the primary compaction trigger.** Every credible
  source (Anthropic's native compaction API, Claude Code's auto-compact, LangChain's
  `SummarizationMiddleware`) triggers on a token budget or percentage-of-context-window; a
  message count appears only as the **secondary** "how much tail stays verbatim" parameter.
- **Summaries should be LLM-authored, structured around state/decisions/next-steps — not prose.**
  Anthropic's own default compaction prompt asks for "state, next steps, learnings"; its
  engineering blog calls raw tool-result clearing "the safest, lightest-touch form of
  compaction" and recommends preserving architectural decisions/unresolved items while discarding
  redundant tool output.
- **No single authoritative "optimal ratio"** of verbatim-recent vs summarized-old content exists
  across sources; informal practitioner guidance clusters around keeping the last 2–3 turns (or
  ~20 messages) verbatim and triggering around ~75% of the context budget.

Full findings, confidence levels, and exact citations: see the workflow journal
(`wf_7cf35a68-d8d`) — available on request; condensed into the design below.

---

## 2. Internal architecture: current state, re-verified 2026-09-17

- **Compaction trigger is purely message-count based**, not token-based:
  `MAX_THREAD_CONTEXT_MESSAGES = 24`, `COMPACTED_TAIL_MESSAGES = 23`
  (`model-gateway/src/sse.rs:3022-3026`), checked once per turn via
  `plan_head_summary(&messages, 24, 23)` (`sse.rs:3799-3802`). `plan_head_summary` itself
  (`compaction.rs:151-171`) does zero token counting — purely `messages.len()`.
- **The head summary IS LLM-authored** (not mechanical truncation): a bounded `direct_infer` call
  with a 6-second timeout, using `summary_prompt_with_memory` (`sse.rs:3814-3841`). Mechanical
  truncation (`DROPPED_HISTORY_NOTICE`) is correctly scoped as a *degradation path only*, on
  timeout/failure.
- **A second, separate mechanism already exists**: `clear_stale_tool_results` — char-budget based
  (32,000 chars / keep last 3), orthogonal to the message-count compaction above. This is
  Verevon's own hand-rolled version of Anthropic's native context-editing API (§1).
- **Compaction and Anthropic's cache breakpoints are completely unaware of each other.** A
  repo-wide grep for every plausible connecting term (`compaction.*cache_control`,
  `cache_control.*compact`, `apply_prompt_caching.*compact`, etc.) returned **zero hits**.
  `apply_prompt_caching` (`inference-core/src/provider/anthropic.rs:280-372`) takes only `body`
  and `req` — no signal about whether compaction ran this turn.
- **`inference-core`'s `PromptCache` never runs on the chat path.** It is wired into
  `fallback.rs`'s `infer()` (unary path only, `cache.get`/`cache.put` at lines 1111/1151);
  `infer_stream()` — the function's own comment: *"the path plain chat streams through"* — has no
  reference to `self.cache` anywhere in its body.
- **`langcache.rs` is a pure 127-line documentation tombstone, zero code.** No `DataPlaneCache`
  struct, no `from_env` function exist anywhere in the current source (a prior finding that this
  function "returns `None` unconditionally" describes a state that predates the 2026-09-16
  removal documented at the top of the file itself — that removal has since gone further and
  deleted the function entirely). The tombstone documents **why two prior cache designs failed**:
  1. A hosted third-party cache — killed for compliance (posting assembled prompts to a third
     party with no residency/TTL/delete guarantees).
  2. A local Dragonfly exact-match cache — killed because **the assembled prompt is not a pure
     function of the question**: two independent, unconditional top-N memory-recall blocks
     (gateway memory recall + session-core context assembly) drift the prompt turn to turn, so an
     identical question produces a different prompt hash and zero cache hits (measured: 17,042 vs
     17,229 bytes for the same question in two fresh threads).
- **Anthropic's breakpoint placement has no knowledge of compaction.** `apply_prompt_caching`
  places at most 3 of the 4 available breakpoints (last tool, system, last message); the 4th slot
  is reserved headroom, unused by anything compaction-related today.
- **Two inert, security-relevant env vars are still shipped.** `SEMANTIC_CACHE_URL`,
  `SEMANTIC_CACHE_TTL_SECS`, `SEMANTIC_CACHE_DATAPLANE_ENABLED` are set in both
  `docker-compose.yml:412-417` and `apps/Model Plane/deploy/docker-compose.yml:381-390`, with
  comments describing behavior that no longer exists in code. **`deploy/docker-compose.yml:381`
  interpolates a live Dragonfly password (`MODEL_DRAGONFLY_PASSWORD`) into a variable nothing
  reads, and fails the whole compose file if that secret is absent** — a real credential demanded
  for dead functionality.
- **No test anywhere exercises compaction and cache_control together.**

---

## 3. The core tension, stated precisely

Anthropic's cache is a byte-stable-prefix cache: a hit requires the bytes before the breakpoint to
be identical to a previous request. `apply_head_summary` (`compaction.rs:255-277`) **mutates the
messages array** — it deletes `head.start..head.end` and splices in a summary message as a new
`role: "system"` entry at exactly index `head.start`. Any request issued after compaction shares
no byte-identical prefix with the pre-compaction array from that index onward.

Concretely: the system-prompt breakpoint is naturally safe today, as a side effect (the system
prompt is a separate JSON field from `messages`, untouched by `plan_head_summary`) — but this is
accidental, not verified by any test. The **rolling last-message breakpoint** is the one that
actually degrades: a summary spliced into the middle of the array shifts every subsequent message
index, so the prior round's cache lookback (bounded to 20 blocks) no longer lines up.

**Resolution:** the compaction boundary (the summary message itself) must become a cache
breakpoint candidate in its own right — exactly the pattern Anthropic's own compaction API
documents ("optionally cache_control the compaction summary block itself"). The summary is the new
floor of the stable prefix; every round after it is byte-identical against that floor until the
next user turn extends the tail.

---

## 4. Recommendations

### 4.1 Compaction trigger: switch to token-budget, keep message-count as a secondary floor

**Numbers:** trigger at **~12,000 estimated tokens** of assembled prompt (reusing the existing
`bytes/4` heuristic already in `anthropic.rs:235-237` — no new tokenizer dependency), compacting
only when the thread also exceeds a small message-count floor (e.g. 8 messages), so a short-but-
token-heavy thread (two turns with a giant pasted table) can still trigger, while a 24-message
thread of one-word replies is no longer force-compacted for no reason. Retain
`COMPACTED_TAIL_MESSAGES` as the minimum verbatim tail. Do **not** adopt a percentage-of-context-
window trigger (Claude Code's ~95% pattern) — Verevon's per-turn prompt budget is deliberately
independent of the model's actual context window (a cost/latency bound, not a hard-rejection
avoidance), so a percentage trigger would silently vary the threshold by model choice.

### 4.2 Anchor a cache breakpoint to the compaction boundary

- Extend `InferRequest` with an optional `compaction_boundary_index: Option<usize>`, set by
  `model-gateway` to the index of the summary message (`= head.start`, per `apply_head_summary`'s
  own splice logic) whenever compaction actually spliced content that turn; `None` otherwise. A
  boolean/index signal from the gateway is preferable to having `inference-core` content-sniff for
  a prefix marker — it keeps the provider function purely positional, and avoids duplicating a
  string constant across the two independently-deployed crates.
- In `apply_prompt_caching`, when `compaction_boundary_index` is `Some(idx)` and a 4th breakpoint
  slot is available and the accumulated prefix through `idx` clears the model's minimum-cacheable
  threshold, mark that message's content block with `cache_control`. This consumes the reserved
  4th slot only on turns where compaction actually ran.
- Add two new tests (none exist today, per §2): one asserting a breakpoint lands at the compaction
  boundary when signaled, one asserting the system-prompt breakpoint is unaffected regardless.

### 4.3 Sharpen the summary prompt for cache stability

Keep `summary_prompt_with_memory` LLM-authored and its existing structure (facts/decisions/
identifiers/unresolved items, "compact factual notes, not prose") — it already matches the
external research's guidance closely. Add one instruction: tell the summarizer its output becomes
a **new stable prefix** other turns will be cached against, so it must avoid embedding anything
that would differ on an identical repeat (e.g., a literal timestamp copied from a tool result).
Purely additive; does not change any existing test's assertions.

### 4.4 Remove the two inert env vars

Delete `SEMANTIC_CACHE_URL`, `SEMANTIC_CACHE_TTL_SECS`, `SEMANTIC_CACHE_DATAPLANE_ENABLED` from
both compose files, and the `MODEL_DRAGONFLY_PASSWORD:?required` gate that exists solely to feed
the dead `SEMANTIC_CACHE_URL` line. **Do not** wire them up to something real — re-implementing
the Dragonfly exact-match tier without first fixing the upstream prompt non-determinism (two
unconditional top-N recall blocks) would reproduce the tombstone's own measured zero-hit-rate
result a second time.

### 4.5 Should a prompt-level answer cache be resurrected?

**No, on the chat path, in any form.** Both prior designs failed for reasons still present today:
compliance (unaddressed, not re-verified this pass) and structural non-determinism (re-confirmed
this pass). Nothing in the external caching literature addresses a prompt that is unstable because
of an *upstream retrieval system re-running every turn* — that's a Verevon-specific memory
architecture property, not a caching-strategy gap.

**What already exists and covers the realistic cases (double-submit, regenerate, retry-after-
error) is `idempotency_registry`** — keyed on request identity, not a prompt hash, and already
tenant-scoped (see the security fix landed 2026-09-16). The tombstone's own conclusion, endorsed
here: don't rebuild what `idempotency_registry` already is under a different name.

**Do not** wire `inference-core`'s existing `PromptCache` into `infer_stream` before §4.6 lands —
it would hit the exact same volatile-recall-block drift that killed the Dragonfly cache, just
relocated to a new file.

### 4.6 The higher-value, lower-risk, independent win: system-message reordering

Move the volatile recall blocks (gateway memory recall, session-core context assembly) to sit
**last** among system messages, immediately before history, instead of ahead of five stable system
messages (already recommended by the tombstone itself, `langcache.rs:109-122`). This is a pure
provider-prefix-cache win — restores a long byte-stable prefix for the existing system breakpoint,
at zero correctness cost — and is completely independent of the compaction work above. **This was
not re-verified in this pass** (the exact system-message assembly order lives in `sse.rs` code
outside what was read here) — confirming the current order is the first implementation step, not
something to take on the tombstone's word alone. Measure with the tombstone's own "ask the same
question twice" experiment before and after.

---

## 5. Priority-ordered action list

1. **Remove the two inert env vars (§4.4).** Lowest effort, lowest risk, closes a live credential-
   leak-to-nothing. Do first.
2. **System-message reordering (§4.6).** Highest value-to-risk ratio, no interaction with
   compaction at all. Do second.
3. **Compaction/cache-boundary anchoring (§4.2).** The substantive fix for the core tension in
   §3. Touches two crates; there's local precedent in this codebase for exactly this kind of
   cross-crate constant duplication (`LENGTH_MARKERS`, `compaction.rs:519-524`).
4. **Trigger change (§4.1).** Sequence after #3, not before — changing when compaction fires
   changes how often #3's new code path is exercised; validate #3 against the current trigger
   first.
5. **Summary-prompt cache-stability clause (§4.3).** Small and safe, but has no payoff until #3
   exists. Land alongside or just after it.
6. **Explicitly do not:** resurrect any prompt-hash answer cache on the chat path, or wire
   `PromptCache` into `infer_stream`, before #2 is measured and shown to fix the drift.

---

## 6. Verification — 2026-09-17 (no code changed in this pass)

### 6.1 Status of the §5 list, checked live

| # | Item | State |
|---|---|---|
| 1 | Remove the inert env vars (§4.4) | **Done and live.** Both compose files edited and re-validated; the running `model-gateway` container's environment carries **zero** `SEMANTIC_CACHE_*` variables. The `MODEL_DRAGONFLY_PASSWORD` requirement remains only where Dragonfly itself needs it. |
| 2 | System-message reordering (§4.6) | Not started — and **§4.6 is corrected below**. |
| 3 | Compaction/cache-boundary anchoring (§4.2) | Not started. Zero references to any `compaction_boundary`/`compacted_this_turn` signal in either crate. |
| 4 | Token-budget trigger (§4.1) | Not started. `MAX_THREAD_CONTEXT_MESSAGES = 24`, `COMPACTED_TAIL_MESSAGES = 23` unchanged (`sse.rs:3022-3026`). |
| 5 | Summary-prompt cache-stability clause (§4.3) | Not started. |

### 6.2 CORRECTION to §4.6 — the reorder is not a win on its own

§4.6 calls system-message reordering "a pure provider-prefix-cache win … at zero correctness
cost." That overstates it, and contradicts a finding verified earlier the same day that this
document did not carry forward:

- On Anthropic — the **default first provider** (`INFERENCE_PROVIDER_ORDER` defaults to
  `"anthropic,openai"`) — `build_request_body` joins **every** system message into **one**
  string (`anthropic.rs:477-481`, `.join("\n\n")`) and `apply_prompt_caching` places **one**
  `cache_control` at the end of that whole string (`:314-331`). Permuting the parts of a single
  concatenated string cannot create a stable prefix when the only breakpoint sits at its end:
  a volatile byte at index 4 and the same byte at index 10 both miss the whole block.
  **Reordering alone is a no-op on Anthropic.**
- It pays **only on OpenAI/Azure**, which prefix-cache automatically over the literal message
  sequence with no breakpoints.
- To realise it on Anthropic, the `system` field must be emitted as **two blocks** — stable
  (marked) and volatile (unmarked) — which needs the gateway to signal the boundary (the
  existing `ChatMessage.name` field can carry it without a proto change). The reorder is the
  prerequisite for that split; **they are one change, not two**, and §5 item 2 should be read
  that way.
- **The size of the prize was measured and is small:** the stable prefix is ~3,400 B ≈ 850
  tokens — below every model's cacheable floor on its own (Sonnet 1,024 / Haiku 4,096) — and only
  qualifies because `prefix_bytes` accumulates the tools block, which already has its own
  breakpoint. Net gain ≈ 765 token-equivalents per cached turn, **under 5% of input**.
- It is also **currently unmeasurable on the chat path**: the streaming adapter folds
  `cache_read_input_tokens` and `cache_creation_input_tokens` into one scalar
  (`anthropic.rs:613-617`) and `InferChunk` carries no cache fields, so a hit cannot be observed.

**Consequence for priority order:** telemetry comes first. Surface the two cache-token fields on
the streaming path (and ideally onto the usage envelope, `sse.rs:6837-6851`) *before* building
§4.2 or §4.6 — otherwise neither can be shown to have worked. `pricing.rs` has no cache tier
either, so a hit will not show as a cost reduction in the ledger until that is added.

### 6.3 What the bar-setters do that Verevon still hand-rolls

Anthropic now ships both halves of this problem server-side, with cache coexistence designed in:
the **context-editing** API (`clear_tool_uses_20250919`, default trigger 100k tokens, keep last 3
tool pairs) does what `clear_stale_tool_results` does, and the **compaction** API
(`compact_20260112`, default 150k / min 50k tokens) summarises history while keeping the system-
prompt breakpoint valid and cache-writing only the new summary block. Migrating to the native
APIs is a legitimate alternative to fixing the hand-rolled coordination in §4.2, and should be
weighed before that work starts — it removes the coordination problem rather than solving it. The
one thing the native path does not remove is the OpenAI fallback, which has no equivalent.

### 6.4 Next steps, in order

1. **Cache telemetry on the streaming path** (6.2) — prerequisite for measuring anything below.
2. **Decide native vs hand-rolled** (6.3) for compaction and tool-result clearing.
3. If hand-rolled: §4.2 boundary anchoring, then §4.1 trigger, then §4.3 — as ordered in §5.
4. §4.6 only as the combined reorder-plus-split, measured before/after with the tombstone's own
   "same question twice" experiment.

## 7. Decided and implemented — native Anthropic APIs, 2026-09-17

The user chose native over hand-rolled (§6.3), after confirming with Anthropic's own current
documentation that this does **not** require moving off the existing Azure deployment (verified
against `platform.claude.com`'s Microsoft-Foundry hosting-comparison table before the decision was
made, not after).

### 7.1 Telemetry (item 1) was already done

Re-verifying against the live repo found this had already landed in the working tree (an earlier,
unrelated session): `cache_read_input_tokens`/`cache_creation_input_tokens` on both `InferResponse`
and `InferChunk` (proto + Rust + the streaming path specifically — `anthropic.rs`'s `infer_stream`
parses them off `message_start.message.usage` and stamps the final chunk), threaded onto
model-gateway's usage envelope, and priced at the documented 10x cache-read discount in
`pricing.rs` — with the Go-side cost-core ledger extended to match. All pre-existing tests plus this
pass's own re-run: `inference-core` provider/anthropic tests (31), model-gateway pricing (4) and
usage-envelope (4) tests, cost-core's full Go suite — all pass. Nothing left to build here.

### 7.2 The exact wire format, researched and then live-verified against this org's own Azure resource

Both dated betas are real and current (`context-management-2025-06-27` header wrapping
`clear_tool_uses_20250919`; `compact-2026-01-12` header wrapping `compact_20260112`), confirmed
directly from `platform.claude.com`. Azure/Foundry support for context editing was already
confirmed at the exact-header level from Microsoft's own capability table (updated 6 days before
this check). Compaction's Azure support was genuinely uncertain from documentation alone (Microsoft's
table has no compaction row at all) — **resolved by making real calls against this org's actual
Azure Foundry resource**, not left as a guess: `clear_tool_uses_20250919` works on every tier
tested; `compact_20260112` returns the documented shape on `claude-sonnet-4-6` and
`claude-opus-4-8`, but **400s explicitly on `claude-haiku-4-5`**
(`"'claude-haiku-4-5-20251001' does not support the 'compact_20260112' context management
strategy"`). Since Haiku is this org's `DEFAULT_AZURE_ANTHROPIC_MODEL`, sending `compact`
unconditionally would have broken the default route on every over-budget turn — caught before
shipping, not after.

### 7.3 Implemented: `services/inference-core/src/provider/anthropic.rs` + `services/model-gateway/src/compaction.rs`/`tool_loop.rs`/`sse.rs`

- `apply_context_management()` always adds `clear_tool_uses_20250919` (`keep:3`, 30k-token trigger,
  5k `clear_at_least`, mirroring the existing tool-payload-budget policy); adds `compact_20260112`
  (Anthropic's documented 150k default trigger) **only** when `model_supports_compaction(model)` —
  false for Haiku-tier models, true otherwise — pinned by a new regression test
  (`haiku_omits_the_compaction_strategy_it_does_not_support`) written directly from the live 400
  found in §7.2, not from documentation alone.
- Both beta headers are sent on `infer()` and `infer_stream()`, gated by
  `native_context_management_enabled()` (env `ANTHROPIC_NATIVE_CONTEXT_MANAGEMENT`, default **on**;
  set to `0`/`false`/`off` to fall back to the hand-rolled path if a different Azure
  resource/region ever behaves differently than the one tested here).
- The dispatch branch this whole migration turns on: `is_anthropic_family_model()` (the same
  `claude`-prefix heuristic `fallback::is_anthropic_model` already uses) gates
  `should_run_local_compaction()`/`clear_stale_tool_results_unless_native()` in `compaction.rs`,
  wired into `tool_loop.rs`'s tier-1 call site and `sse.rs`'s tier-2 head-summary block. An
  Anthropic-family turn skips the hand-rolled `plan_head_summary`/`apply_head_summary` and
  `clear_stale_tool_results` entirely, letting the native API handle both server-side.
- **The OpenAI/non-Anthropic path is untouched** — verified two ways, not one: the original
  `clear_stale_tool_results` function has zero lines changed in the diff (only additions after it),
  and a test (`a_non_anthropic_turn_still_runs_the_hand_rolled_clearing_unchanged`) diffs the new
  wrapper's output against the raw original function on identical input.
- Applied edits are logged (`info!`) rather than plumbed through the full proto/gRPC/usage-envelope
  chain — a deliberate, documented scope cut (doing it properly would mean touching every other
  provider's response construction, which conflicts with leaving the non-Anthropic path alone).
  One accepted side effect: `history_was_compacted`/`reattach_context` reads "not compacted" locally
  even when Anthropic's native compaction fired remotely, since it leaves no local marker.
- The cache-boundary-anchoring work (§4.2, `compaction_boundary_index`) and the reorder-plus-split
  (§4.6) were **not implemented** — both were designed specifically for the hand-rolled path's
  cache-invalidation problem, which no longer applies once the Anthropic path defers to native
  compaction (Anthropic's own docs state native compaction is designed to coexist with prompt
  caching server-side). They remain relevant only for the OpenAI fallback path, where the original,
  smaller-prize, harder-to-measure economics from §6.2 still apply — not picked up this pass.

### 7.4 Verified independently

`cargo check`/`cargo test` clean on both crates (`inference-core` 281/281 + 16 integration;
`model-gateway` 1182/1182 lib + 31/31 `compaction::tests`, including every new migration test).
The gating logic was confirmed correct by direct code read, not just by trusting the tests: the
Anthropic-only functions (`native_context_management_enabled`, `context_management_beta_header`,
`apply_context_management`) are referenced nowhere outside `anthropic.rs`, and Azure-Anthropic uses
the identical `AnthropicProvider` struct via `::new_azure(...)` — the same live-verified code path,
not a separate one that could silently drift.

**Scope flag, not a defect in the migration itself**: `git status` on `apps/Model Plane/rust` shows
roughly 40 modified files; about two-thirds is confirmed-unrelated, already-in-flight work from
other sessions (execution-core changes, session-core's memory-grounding feature from
`LEARNING_EDGE_FEATURES_2026-09-17.md` §6, a `tool_loop.rs` split for a subscription web-search
tier, a new conversation-core HMAC delegation integration). Three of model-gateway's integration
tests fail deterministically because of *that* other work, not this migration — traced to specific,
named causes in each case (a stale golden-text match against a refactored function, a docker-compose
text-contract test, an orchestration ownership route) — and one integration test is separately flaky
for an unrelated pre-existing timing reason. This tree is not currently isolated enough to commit
the migration alone without also resolving or excluding that other bundled work.

**Status: implemented, live-tested against a real Azure deployment, and independently verified.**
Not yet deployed to the running containers as of this writing — needs a Model Plane rebuild to take
effect live.
