# T1 / Category 9: Realtime & sync architecture (cross-source comparison)

Inputs: 171 reference records (buzz 45, grokbot 32, qm 33, slack 31, teams 30) and 15 Verevon baseline records. Output: 43 clusters, 170 records assigned, 1 excluded (Slack shell cookie rewriting). The cluster file is `09-realtime-sync-architecture.clusters.json`.

## Shared baseline (table stakes)
- **A push channel per client** (C-001, 5/5 sources): a WebSocket or server stream that delivers messages, typing, presence and agent state as soon as the server accepts them. In Slack and Teams the evidence is mostly DOC/REA. In Buzz, Grok Bot and QM it is SRC.
- **Stream health with a polling safety net** (C-006, 4/5): silence and stall watchdogs, a fallback to polling, and a relaxed safety poll even while push is healthy.
- **Local replica or paint cache** (C-010, 4/5): every desktop client has one. QM deliberately has none, and Verevon's Zero-Data-Retention (ZDR) rule argues against caching content.
- **Server-managed (non-E2EE) chat** (C-037, 4/5): no reference encrypts chat end to end. Teams does so only for calls.
- **Common (3/5):** gap-free resume (C-004), idempotent sends (C-007), a cross-room state stream (C-013), presence and typing as leases (C-014), a typed append-only room log (C-020), off-thread sync (C-024), connectivity awareness (C-027), connection hardening and rate limits (C-028), scale-out and drain (C-029), protocol evolution (C-031), and perceived-latency telemetry (C-033).

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Push channel (C-001) | Grok Bot `WatchGrokBotTranscripts` | One multiplexed stream with sequence numbers. It carries rows, deletes, `cleared{generation}`, `cursor_too_old`, heartbeats, agent state and roster changes (q5 SRC) |
| Authorization on live delivery (C-002) | Buzz | One fail-closed delivery chokepoint. Access is checked before a subscription is registered. Cache hints are kept separate from socket-kill commands across pods |
| Resume (C-004) | Grok Bot (contract) + Buzz (policy) | Resumes from a per-room `updated_seq` cursor. Resume is triggered by events, replay is paced, and the client opens live before backfilling and dedupes |
| Live agent run for all viewers (C-005) | QM | AG-UI-style SSE with text deltas keyed by offset and a snapshot fallback, so late joiners can catch up. Grok Bot adds computer-action frames |
| Idempotent send (C-007) | Grok Bot | A send journal with stages queued, prepared, dispatching and accepted-awaiting-echo |
| Cross-room aggregate (C-013) | QM `/session-state/events` | One filtered stream reports working, awaiting-approval or idle for each thread |
| Timeline paging (C-022) | Buzz NIP-CW | Top-level windows computed on the server, with reply summaries and exact composite cursors |
| Run queue (C-018) | QM | At most one running run per session, SKIP LOCKED claims, lease heartbeats, a reaper, and a tool ledger per attempt |
| Offline queue (C-009) | Teams | Ordered offline resolvers, 48 h expiry and 10 retries across sessions |
| Connectivity (C-027) | Slack | Distinguishes online, service-down and offline with a probe. Sleep, wake and lock events are forwarded |
| Perf under agent load (C-033) | Buzz + Grok Bot | Playwright typing and scroll budgets measured while simulated agents flood the room. Every send carries a trace and enter-to-ack timestamps |

## Differentiators worth carrying
- Authorization re-checked on every delivery (Buzz).
- Live agent runs that any member can watch, with computer-action frames (QM, Grok Bot).
- Mid-run steering of an agent (QM).
- A generation counter for rollback (Grok Bot).
- Paging with exact cursors (Buzz).
- Performance budgets under agent load (Buzz).
- A reverse channel to the user's own computer (Grok Bot).

## Verevon's biggest gaps (ranked)
1. **No push transport (C-001, priority, sig 5, L).** Rooms poll every 6 s while the tab is visible (`SpacePage.tsx:78-87`). Other members see agent replies only after a poll. The recommended build: Application Plane emits a seq-numbered Space event projection, the gateway serves one SSE per tab, and each delivery is checked against Control decisions (C-002, priority). Exact resume (C-004, priority) should copy the own-turn seq and Last-Event-ID scheme that already exists.
2. **Agent turns stream only to the sender (C-005, priority, sig 5, M).** The AG-UI SSE and resume routes already exist (`ag_ui.rs`). What is missing is the audience (any member with a room read decision) plus offset deltas.
3. **No cross-room unread or working aggregate (C-013, priority, sig 4, M).** The sidebar dots come from a browser-local store that only covers the open room.
4. **The full thread list plus N+1 transcript fetches run on every poll (C-022, recommended, sig 4).** There is no paging or jump-to-date.
5. **Retries are not idempotent (C-007, recommended, S).** The gateway generates `idempotency_key` itself when the composer omits it (`spaces.rs:136-137,605-613`), so a retry creates a new turn, which is billed again.
6. **The run queue and steering are unverified (C-018 unknown, C-019 partial).** Multi-member rooms need per-thread serialization and steering at step boundaries.
7. **Smaller gaps:** no typed Space event envelope (C-020), no stream watchdog (C-006), no offline-vs-down banner (C-027), and no traceparent or client latency budgets (C-033).

## Verevon strengths (keep)
- **Admission and fencing (C-015, C-042).** Control-signed decisions last 2 minutes and are bound to the audience revision, payload digest and idempotency key. This gives one authority chokepoint, stronger than QM's roster version or Buzz's membership cache.
- **Durable outboxes (C-017).** Convex outboxes with leases, retries and idempotent event ids are already shipped for lifecycle, audience and deletion.
- **Own-turn SSE resume.** It uses a monotonic seq, Last-Event-ID and refusal of duplicate frames.
- **Honest authority recheck.** A fail-closed recheck runs every 30 s, and the page renders "gap, not empty".
- **ZDR propagation (C-037).** It goes beyond every reference's encryption posture. Any push, cache or offline tier must preserve it, so send references rather than content.

## Skip or defer
- The desktop shell store (C-026), sovereign deployment profiles (C-030), sharded storage (C-021), replica routing (C-023), key-based device pairing (C-038) and multi-account global state (C-039) are premature or out of scope.
- The offline queue (C-009) and local content cache (C-010) are nice-to-have only. Content-bound decisions and ZDR make them costly.

## Caveats
- **Unknown status.** 15 clusters have status "unknown" because no baseline record covers them. Examples are the run queue, scale-out, feature flags and co-authoring. Each one says what to check in code.
- **Checks made in this step (SRC grep).** The gateway rate limiter (`rate_limit.rs`), the absence of traceparent, the append idempotency default, and the AG-UI Last-Event-ID passthrough were checked directly. These are not baseline records.
- **DOC-only evidence for Slack.** The fabric, presence, Vitess and cell evidence is DOC (q5 from engineering blogs), and the Teams limits are DOC. This category does not upgrade them.
- **Fuzzy prevalence classes.** Desktop-shell clusters (C-024, C-026, C-027) depend on the reference being a desktop app. Verevon is web-first, so they are rated lower.
- **Stale or contradictory docs.** The SPACE_PAGE_AUDIT §18 claim about sidebar dots contradicts the code. `CODEBASE_INFORMATION_SYSTEM.md` calls the Application Plane "realtime sync", but the Spaces UI uses no Convex subscription.
