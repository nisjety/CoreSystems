# Implementation spec: stream resume (#47) and edit-version navigation (#49p2)

**Status: SHIPPED and live-verified, 2026-08-01.** Both features' first designs
were rejected by independent adversarial review for correctness (see below for
what was wrong and how the corrected design avoids it). Once an authenticated
dev session became available, both were implemented per the corrected design,
covered by tests, and confirmed live in the browser — including a real,
previously-undiscovered duplicate-answer bug that building #49 uncovered.
Commits: `b8aea9bd` (gateway producer), `f1ab0e6b` (SPA reload merge),
`a8a82665` (version navigation + the bug it uncovered).

These are parity-backlog items #1 (resume) and #8 (edit-versions). See
`VELION_CHAT_PARITY_BACKLOG.md`.

This document is kept as the design record — the "corrected design" sections
below are exactly what shipped.

---

## #47 — Server-authoritative chat resume

### The real break (verified, not the task's framing)
More is built than the task implies. The **transport is complete**:
`model-gateway/src/stream_buffer.rs` has both backends with passing tests, and
`sse.rs`'s `invoke_resume` is a real resume endpoint. Agentic runs already resume
via `runId` through `run_events_sse`, which honours `after_event_id`.

What breaks is the **producer** and the **SPA merge**:
- On client disconnect, `tx.send` fails and `sse.rs` (~1566–1576, ~1592–1602)
  sets `cancelled = true; break`, and `~1866` **cancels the run**. So generation
  STOPS, the assistant message is never persisted, and `stream_buffers.finish`
  never runs. A reconnect then finds a half-answer marked `stopped`.
- The SPA drops the cache-only waiting turn on reload
  (`use-chat-controller.ts:391-393` → `chat-normalizers.ts:187`), so even a good
  buffer would not render.
- Six of eight answer paths never write the buffer at all.

### Why the first design was rejected
"Detach, don't cancel" naively removes the ONLY signal that currently stops a
run — so a deliberate user cancel (`/v1/invoke/{request_id}/cancel`) and an
incidental disconnect become indistinguishable, and a user who cancels keeps
paying for a run that finishes anyway.

### Corrected design
1. **Distinguish cancel from disconnect.** A disconnect is `tx.send` failing; a
   cancel is the cancel-registry token being set (`cancel_registry.rs`). Track a
   `client_connected` bool separate from the cancel signal. On `tx.send` failure:
   set `client_connected = false`, DO NOT break, DO NOT cancel — keep draining
   the gRPC stream, keep buffering to `stream_buffers`, keep heart-beating.
2. **Terminalize on the real outcome, not on the disconnect.** When the provider
   stream ends: persist the assistant message and `stream_buffers.finish` as
   normal, terminalize **Completed**. Only the cancel token terminalizes
   Cancelled. A disconnected-but-completed run is Completed and fully replayable.
3. **Prefer reusing the agentic path over a second mechanism.** Plain chat should
   resume through the SAME `run_events_sse` + `after_event_id` path agentic runs
   already use, rather than maintaining a parallel resume endpoint. That means
   every chat turn must own a durable `runId` and write its parts to the same
   store the run-events stream reads (the direct-inference run already exists —
   it just isn't the SPA's reconnect target).
4. **SPA**: on reconnect, replay from the buffer/run-events by `Last-Event-Id` /
   `after_event_id`, then re-subscribe — mirroring the agent panel's durable-run
   attach. Fix the `chat-normalizers.ts:187` drop so the waiting turn survives a
   reload.
5. **Bound the cost.** No new bound is needed: the existing `answer_token_budget`
   caps generation, so a user who never returns costs at most one capped answer —
   which is the point (the answer completes so they can reconnect to it).

### What it will still NOT cover (state it plainly)
`stream_buffers` is in-process and dies on redeploy (see landmines). Cross-deploy
resume requires the Dragonfly-backed buffer (`GATEWAY_CACHE_REDIS_URL` exists) —
land that as step 6, not the entry price.

### Verification (needs a live session)
Drive a chat turn, kill the connection mid-stream, reconnect, confirm the full
answer replays and the run is Completed (not stopped/cancelled). Then a real
`/cancel` mid-stream must still terminalize Cancelled and stop generation. The
e2e mock suite (`e2e_invoke_chain_test`) can cover the terminal-outcome branches
by dropping the receiver; the reconnect UX needs the browser.

---

## #49 part 2 — edit-version navigation (1/N siblings)

Part 1 (the regenerate/edit-resubmit dissatisfaction signal) shipped in
`fdaf4376`. This is the visible switcher.

### Why the first design was rejected (both reviewers)
1. A flat single `(versionGroupId, versionIndex)` marker pair **cannot express
   nesting**. Edit is offered on every user message, so editing turn *i* while a
   non-newest version is selected re-stamps turns that already belong to a later
   alternative — two alternatives merge into one segment, rendered stacked and
   persisted by the next snapshot. Irreversible.
2. `SupersedeLatestExchange(thread_id)` is wrong when editing a **non-final**
   message: for `[u1,a1,u2,a2]`, editing `u1` supersedes only `u2,a2`, leaving
   `u1,a1` live in `load_thread_messages` forever while the client hides them —
   the prompt and the view diverge permanently and silently.
3. `ListConversation` (session-core) also feeds **capability-core's skill
   learning** (`sessionreview/transcript.go:43`), so filtering only Dreaming
   would still author skills from discarded answers.

### Corrected design — client-only v1, guarded to the last exchange
This sidesteps every rejected flaw by not persisting and not nesting:
- **Client-only session state.** No proto, no session-core change, no supersede
  RPC — so flaws #2 and #3 cannot occur. On reload versions are gone (documented,
  same as `followUps`/`branchCount` today).
- **Guard to the newest exchange only.** `regenerateLatest` already targets the
  last exchange. Restrict versioning to that: editing/regenerating the FINAL
  exchange snapshots the outgoing pair into a sibling array before replacing;
  editing an EARLIER turn keeps today's truncate-and-resend (that is what
  `branchAt` is for). Guarding to the final exchange makes flaw #1 (nesting)
  structurally impossible.
- **Data model.** A dedicated, unit-tested `chat-versions.ts` module holding, for
  the last user-message node, `versions: ExchangeSnapshot[]` and `activeIndex`.
  A derived `visibleTurns` applies the active version. Every transition
  (snapshot, switch, new-version-on-regenerate) is covered by vitest — this is
  the part that must be provably correct, and it can be without a browser.
- **Render.** `prev/next` arrows + `n/N` on the last exchange, reading the derived
  state. Selecting a sibling swaps the displayed snapshot; it does NOT
  regenerate.
- **Bonus already in place.** Edit-resubmit is already a tracked dissatisfaction
  signal — persisting the sibling instead of discarding it enriches that signal
  for free.

### Path to server-persisted versions (later, not v1)
When #1 (server-authoritative sessions) lands, promote the client sibling array
to server state with an **ordinal anchor** (`keep_leading_live_messages`, not
"the last exchange"), and add `include_superseded` to `ListConversationRequest`
so ONLY model-gateway's chat read path sees superseded turns and skill-learning
never does. Not before #1.

### Verification — DONE, live, 2026-08-01
vitest covers the state module exhaustively (17 cases: snapshot, round-trip,
boundary disable, thread/anchor invalidation — `chat-versions.test.ts`). Live in
the browser: regenerated a turn twice and edited the final turn once, producing
a real 3-version exchange (verified with a non-deterministic prompt — "pick a
random animal" — specifically to rule out a cache hit making two different
slots look coincidentally identical, which a first pass with a short factual
prompt could not rule out). Paged 1/3 → 2/3 → 3/3 and back; each version
restored its exact question+answer pair and follow-up chips, boundaries
correctly disabled the arrows. Also confirmed: this uncovered `regenerateLatest`
was appending a duplicate answer instead of replacing it (present since the
feature shipped, independent of versioning) — fixed in the same commit
(`a8a82665`) since the truncation is what makes both the switcher and plain
regenerate correct.

---

## #47 verification — DONE
Two independent proofs, since a live browser reload alone couldn't establish
whether the exact disconnect-detection code path fired (this dev topology's
BFF-to-model-gateway proxy may not propagate a browser disconnect to
model-gateway within an observed generation window — the browser test could
only prove the *outcome*, not the mechanism):

- **Deterministic Rust test** (`invoke_stream_completes_and_persists_after_the_client_disconnects_mid_stream`,
  `e2e_invoke_chain_test.rs`): drops the SSE response body's data stream mid-run,
  which drops the channel's `Receiver` so every subsequent `tx.send` genuinely
  fails — no wall-clock race. Asserts the run still persists both messages,
  terminalizes `Completed`, and finishes a full, replayable buffer. This test
  fails against the pre-fix code (old code would see only the user message
  persisted and a `Cancelled` terminal outcome).
- **Live**: sent a 300-word request, reloaded mid-stream (Stopp svar still
  showing), and the reloaded thread showed the complete answer with no stuck
  spinner or error — proving the end-to-end outcome the fix exists for.

## Why this took a live session
Both are interactive chat-path features whose first designs were shown to
corrupt data, and needed either a live reconnect (#47) or a live version
switcher (#49) to verify safely — not something a static review can settle.
The corrected designs above turned out to be exactly what shipped once a real
session was available.
