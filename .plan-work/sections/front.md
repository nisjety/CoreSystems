# Space hybrid build plan: Verevon Space as the room for people and agents

File: `docs/plans/space-hybrid-build-plan-2026-10-14.md`. State of the facts below: 2026-10-20.

## 0. How to read this plan

**Status: PLAN FOR APPROVAL. Nothing is built.** No product code, migration, proto, flag or configuration was changed to produce it. Every "exists" in this plan is a read of the live tree; every "will" is a proposal.

### 0.1 Inputs

| Input | Where | Role |
|---|---|---|
| The audit (96 items, F-t1-001..096) | `C:/dev/verevon-audit-2026-10-14/01-verevon-space-audit.md` (permanent, read-only copy; byte-identical to the working copy used while planning) | Baseline findings (audit 1.3), priorities (1.4), strengths to protect (8), waves (9.1), ADR candidates (9.3) |
| Evidence pack | `C:/dev/verevon-audit-2026-10-14/audit-evidence/t1/` | Code-verified baseline per item |
| Sibling audits | `02-quarry-and-browser-audit.md` (F-t2-nnn), `03-model-plane-chat-and-agents-audit.md` (F-t3-nnn), same folder | Touchpoints only; this plan never plans their scope |
| 20 workstream designs | one per WS01-WS20, full detail in section 3 | Slices, file lists, flags, tests, definition of done |
| Analysis | section 2 decisions (SD-00..SD-29, gates G0-G6), section 3z epics, section 4 sequencing, section 5 cross-session ownership, section 6 verification | Where a design disagrees with an analysis, the analysis decides |
| Sweeps | citation checks per workstream, live-tree delta, build and test gates | Path and line verification |

**Precedence.** Slice order, lanes, milestones, migration and proto number reservations: section 4. ADR rulings and defaults: section 2. Shared-piece ownership with the sibling sessions: section 5. Scenario ids and budgets: section 6. Where this plan changes a number a design gave (migration numbers 0046-0055 and 030-042 are placeholders; `WS06-S0` dropped; `WS18-S4a` split out; `P-*` and `M0-*` rows added), the change is stated where it happens and listed in 4.0.3.

**Section map.** 1 goal, criteria, scope, at a glance. 2 decisions first. 3 workstreams (3a-3c firm designs, 3z epics E01-E12). 4 sequencing, lanes, conflicts, migration and proto reservations, effort, first three PRs, rollout order. 5 cross-session (sibling plans). 6 verification (scenarios M0-M3, budgets, evidence).

### 0.2 STATE NOTE: which tree this plan describes (read before opening any cited file)

- The audit and the designs read the **live tree `C:/dev/CoresSystem`**: branch `main`, HEAD `afb83928`, with **326 uncommitted and untracked entries** (324 when the audit snapshot was taken; re-counted 2026-10-20). Most of that is the ZDR Z-series build and the Quarry option-B rerank work (map: `.plan-work/haiku/live-tree-delta.md`; 232 of the entries are Space-relevant, 58 are product-demo and marketing work that is out of scope).
- The **planning worktree** (`C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567`, HEAD `828d9d55`) is **61 commits behind `main`** and holds none of the uncommitted work. **Cite and verify against the live tree, not the worktree.** Lines in files tagged [M] may differ from the audit's citations (made at `afb83928`); each design gives the line it verified in the live file.
- Sibling build work lives on local, unpushed branches over `afb83928`: `model-plane` (tip `d2ea86a1`, 90 commits) and `quarryv2` / `quarryv2-next` (tips `bed81d16`, `e7c321d5`). The Z-series is committed on `model-plane`; section 4 (M0-BASE) makes that branch the Space base. The live tree's dirty copies of the Z-series are duplicates to be discarded by their owner, never committed a second time and never read by Space.

**Cited files that exist only as uncommitted work in the live tree** (state [U] new, or [M] modified; none is committed at `afb83928`):

| Area | Path (repo-relative) | State |
|---|---|---|
| ZDR refusal catalogue | `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`; `apps/Model Plane/api/contracts/zdr-refusals.json` (+ `.sha256`); SPA copy `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts` (+ test and json copy) | [U] |
| `zdr_posture` | `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs` (+ `tests/zdr_posture_matrix.rs`) | [U] |
| Retention receipt and resolver | `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs`; `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs` | [U] |
| Session-core migrations | `.../session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` (+ `revert/`); next free number in the live tree is 0046 | [U] |
| Session-core authority code | `.../session-core/src/grpc.rs`, `auth.rs`, `run_service_grpc.rs` (waiver, owner housekeeping, posture-checked run owner) | [M] |
| Model-gateway streaming and routes | `.../model-gateway/src/sse.rs`, `sse_events.rs`, `http_routes.rs` | [M] |
| Data Plane | `apps/Data Plane v2/services/retrieval-engine-rs/src/egress_attestation.rs` [U]; `src/authz/service_routes.rs` (Space-scope guard for service principals) [M] | [U]/[M] |
| SPA ZDR and notification files | `src/features/spaces/components/SpaceRoomComposer.tsx` (+ test), `src/shared/api/notifications-client.ts` (contract alignment) under `apps/Frontend Plane/verevonv3/` | [M] |

Committed `main` would not build without the Z-series (build-test-gates sweep, finding 3; not re-run here). Sibling-built numbers that the live tree does not show (session-core 0046-0048, proto tags, `park_reason`, `ListToolActions`) are taken from `model-plane` by read-only git and are tagged where cited.

### 0.3 Legend of path tags

| Tag | Meaning |
|---|---|
| [C] | COMMITTED: tracked at `afb83928` and not modified in the live tree |
| [M] | MODIFIED-UNCOMMITTED: tracked but dirty; the line you see may differ from the audit's citation |
| [U] | UNCOMMITTED-NEW: untracked, exists only as uncommitted work (a `??` entry ending in `/` covers everything below it) |
| [N] | NEW: does not exist anywhere yet; this plan proposes to create it |

Paths are repo-relative with the spaces exactly as on disk (`apps/Model Plane/...`). A path marked UNVERIFIED could not be confirmed in the live tree.

### 0.4 How the plan was produced

| Step | Model tier |
|---|---|
| Per-workstream sweeps (citation checks, live-tree delta, build and test gates) | Haiku 5.5, max effort |
| Designs WS01-WS04, WS09, WS10 (the first six, written before the instruction changed) | Sonnet 5.5, xhigh |
| Designs WS05-WS08, WS11-WS20, analysis (decisions, epics, sequencing, cross-session, verification) and section writing | Sonnet 5.5, high |
| Not used | no Opus, no Fable |

### 0.5 The two sibling plans (already approved, partly built)

- **Model Plane and chat plan** (`docs/plans/model-plane-chat-build-plan-2026-10-14.md`, worktree `epic-shaw-4d613b`; its header text may still read DRAFT): building on local branch `model-plane`. Landed there: the Z-series, retention receipt, the durable agent loop (approval consume, loop checkpoints, tool-action effect ledger; session-core migrations 0046-0048), reservation ledger entry. Its numbered workstreams are written `mp-WSnn` and decisions `DEC-nn` here; "WS03" in that plan is not Space WS03.
- **Quarry v2 and browser plan** (`docs/plans/quarry-browser-build-plan-2026-10-14.md`, approved 2026-10-15 with the recommended defaults): building on local branches `quarryv2` and `quarryv2-next`. Browser-broker grants and the first grant issuer (F-t2-001) are built; live proof is not claimed.
- **Reconciliation.** This plan consumes their numbers as taken (session-core 0044-0070, their proto tags), renumbers all Space reservations above them (session-core 0071-0085, spare 0086-0090; section 4.6), adopts one vocabulary with them (grant scope, origin, run state, policy registry; SD-15 and conflicts C1-C23 in section 2), and flags every shared piece instead of planning it: Session Core, approvals and resume, ZDR, agent-work visibility, agent memory, BrowserBroker grants, the agent browser path, ZDR-safe telemetry. The two sibling branches already overlap each other in 26 files, so the hotspot rules of section 4.4 are mandatory.

## 1. Goal, success criteria and scope

### 1.1 Goal

Make Verevon Space the hybrid room for people and agents:

- **People get Teams-class groups and teams and Slack-class rooms.** Roles and a directory, direct messages, human posts and thread replies that do not invoke a model, live push, identity-bound mentions, reactions, edit and delete, attachments, search and permalinks, attention across rooms.
- **Agents get Grok-Bot-style colleague behaviour.** Personal agents with an owner-controlled access list and audience-safe memory, work the whole room can watch and stop, approvals that really resume, a computer with resource caps, and a browser the room can see and take over.
- **All of it inside Verevon's authority model.** Control Plane signs short-lived, audience-bound decisions; Space access never widens an owner-plane ACL; ZDR fails closed with typed refusals in every plane that persists or pushes content; the Model proposes and Quarry executes browser actions; no direct database crossing between planes.

Starting point (audit 1.2 and 1.3, code-verified): an authority-first agent cockpit on a conversation model that is still an AI prompt box. Every room post is a model turn; there is no room push (6 s poll); only the sender sees an agent work; governance cannot be administered (no effect-policy UI, SCIM stub answers success, any org member can read all org audit events); approvals resume for three action kinds only; agent memory is not Space-aware; DMs, reactions, edit and delete, attachments and search do not exist; the agent sandbox is confined but uncapped.

### 1.2 Success criteria

Sixteen criteria. Each is checked on a rebuilt local stack (images rebuilt, stack running, flow verified end to end: the repo rule for every deployment claim), by a person other than the slice author (verification protocol 6.4). "B" is a second member of the same room. Numbers marked (proposal) are acceptance targets to be calibrated on the baseline run in M1 week 1 and then frozen (budget = measured baseline x 2 unless stated); unmarked numbers come from the designs or the audit. Scenario ids are those of section 6. Baseline = audit 1.3 finding number.

| # | Criterion and number | How measured | Proven by (milestone, scenario) |
|---|---|---|---|
| 1 | **A human post is not a model turn.** 0 `runs` rows from a post or a member reply; the agent context read of that thread contains 0 posts; `agent_memory` and thread columns unchanged. Same `client_send_id` replayed gives 1 row (`deduplicated:true`), changed content 409; viewer and non-member get 403 with 0 rows. Server acceptance p95 under 400 ms including the Control mint; optimistic row under 100 ms (both proposal). Baseline: 1.3 #1 | SQL on `messages` (`post_kind`, `author_subject_id`) and `runs`, HAR of POSTs, context read, `cargo test` and `go test` for the append gate | M1, M1-S1 |
| 2 | **Live delivery without a model.** B sees A's post or run-state change p95 under 1.5 s (p50 under 500 ms, proposal) via a pushed frame, with no 6 s `/threads` timer in the HAR (one `GET /events` per tab). Stream down: B converges within 6 s plus jitter by polling, degraded indicator within 35 s. 500 users x 3 tabs reconnecting after a gateway restart all resume by `Last-Event-ID` within 60 s. Baseline: 1.3 #2 | B's refetch time minus `space_event_log.created_at`; HAR; stop and start session-core; `space-stream-storm.mjs` | M1, M1-S2 and 6.5 budgets |
| 3 | **Revocation closes the stream.** A removed member's Space stream and run stream close p95 within 15 s of the membership event, hard bound 130 s if the event is dropped; the member's next call fails closed with 0 content bytes; a read token on the stream RPC and a stream token on `ListThreads` are both denied | Timing table (`space_revocation_close_seconds`), `space_membership_revocations` row, close frame, grpc-level cross-use test | M1, M1-S3; M2-S2 (mid-run) |
| 4 | **Zero message bodies on any push wire or notification payload.** A unique canary string posted in a room appears 0 times in the raw SSE capture, `space_event_log`, `session_audit_outbox` payloads, Convex attention rows (no text field), `notification_requests` payloads (refs only), metrics labels and logs; the nightly X3 canary job stays at 0 | Canary scan over wire captures, DB rows and logs; grep of gateway and model-gateway logs | every milestone; M1-S2, M2-S4 |
| 5 | **Every new write path refuses typed under ZDR.** 100 percent of new durable routes (post, thread create, mention wake, grant create, attachment, canvas write, browser run, personal-agent admission) return the refusal-catalogue code or `poll_only`/refs-only, with 0 rows created in `messages`, `space_event_log`, notification tables and Convex, and 0 Control calls for refused requests; the SPA shows the catalogue text. Baseline: 1.3 #8 | Route-class registry inventory test (WS06-S4, fails on an unclassified route), `SELECT count(*)` before and after, user-core log, `zdr-contract-sync.test.sh` | ZDR tour at M1-M3 (M1-S4, M3-S8) |
| 6 | **Governance is usable and the security holes are closed.** An org admin applies a preset in the effect-policy console and a member's read flips from the typed `space_policy_not_provisioned` denial to success; the change leaves 1 policy event row and 1 audit-outbox row (`published_at` set) and reaches audit-core within 10 s; a member calling the admin route gets 403 with 0 Control calls; stale revision 409; same key replays `replayed:true`; a typed 403 never shows "log in again". `GET /api/v1/audit` as a member returns own rows only and 403 `audit_scope_denied` for another user, a forged `X-User-Role` gets 403; `POST /api/v1/scim/v2/Users` returns 501 with a SCIM error body, 0 store writes, 0 NATS events; an OIDC `jobTitle=Director` no longer yields an admin role. Baseline: 1.3 #7 | HAR for admin and member calls, SQL on `space_effect_policy_events`, `user_audit_outbox`, `member`; `curl -i` for SCIM | M1, M1-S5, M1-S8 (SCIM and container limits) |
| 7 | **Enterprise identity is real.** SCIM PATCH `active:false` removes the person's org membership, sessions and Space access; the replay is a no-op; the last owner is refused; Space decisions for that user are refused within the 30 s bound of `scripts/verify-deprovision.sh`; the deprovisioned owner's personal agents are suspended within 60 s; SSO `enforce` returns `sso_required` and a break-glass owner still signs in (password plus 2FA) | `verify-deprovision.sh` propagation table, IdP log (real IdP: UNVERIFIED until run), SQL on `organization_scim_users`, `member`, `user_org_memberships` | M2.3, M2-S2 |
| 8 | **Room administration works.** Role changes obey the owner guard (a manager cannot grant manager or owner; the sole owner cannot demote, remove or leave: 409); rename and topic; archive makes writes fail and reads succeed, restore reverses it; an org-room roster follows org membership within 10 s; joining creates 0 `resource_grants`; a directory self-join (M2.2) grants the default role and an org-Y user gets 404 byte-equal to a missing room | `space_memberships`, `space_recipient_audiences`, Control roster, byte-compared 404 bodies | M1, M1-S6; M2.2 for the directory |
| 9 | **Agents work in a human room and everyone can watch.** A post that names agents by identity wakes exactly those agents in the typed order as separate runs (`turn_origin='direct'`); forged `turn_origin`, forged wake context or another user's trigger starts 0 runs; replaying `client_wake_id` yields 1 run. While A's multi-tool turn runs, B sees the card advance step by step, a reload resumes it, A sees no duplicate card; a second invocation on the busy thread is refused `thread_run_active`; a manager Stop records `cancelled_by`, a viewer's direct stop is 403. Baseline: 1.3 #1, #3 | SQL on `message_mentions` and `runs`, SSE transcript with tokens redacted, `RUN_CANCELLED` payload, HAR | M2.1, M2-S1 |
| 10 | **Approvals resume with scopes.** A gated provider write can be decided once, for this conversation (thread scope), or always (a Control standing rule); it resumes for the audited action kinds `execute_provider_action`, `book_shipment` and `tickets.create` and is verified by read-back with receipt actor = requester; the once-token is consumed exactly once under a concurrent race test; a Space `block` ceiling beats any grant; a never-auto action is never grant-passable; shell, browser and MCP continuations fail closed `invalid_continuation` with honest copy; removing the requester before approval ends the continuation `authority_changed` and nothing runs. Baseline: 1.3 #4 | Approval JSON (`resume_class`, `continuation.state`), grant rows, `consumed_at`, integration receipt, race test | M2.1 owner path, M2.3 member decide; M2-S3 |
| 11 | **Attention across rooms is correct and fast.** A mention of A creates exactly 1 refs-only attention item (replay keeps 1) visible within 5 s p95 of the post commit (proposal); a muted thread creates 0. With the room closed, A's sidebar dot appears within one visible poll (30 s) of B's post; the cross-room unread and working aggregate matches SQL ground truth in 100 percent of 60 seeded rooms, p95 under 500 ms (proposal), computed once on the server; notification budget at most 30 per recipient per hour; ZDR gets `zdr_empty`. Baseline: 1.3 #10 | `GET /api/v1/spaces/attention` vs SQL, `space_event_log` and outbox rows, Convex `spaceAttentionItems`, `nats consumer info`, k6 on the aggregate | M2.2, M2-S4 |
| 12 | **Chat basics each carry a per-route authority test.** DM: 1 `registered_spaces` row of kind `dm` per participant set, reopen returns the same `space_ref` (`created:false`). Edit writes a revision row; delete writes a tombstone for both sides, derived `agent_memory` rows are gone, the thread context shows the tombstone line; B editing A's post is 403; a moderator delete records `actor_class=moderator`; reactions counts and own state are correct, a viewer is refused; forged `actor_class` or `moderation` body fields are rejected. Every route has viewer, non-member and removed-member refusal cases. Baseline: 1.3 #6 | SQL on `message_revisions`, `registered_spaces`, context read, audit-core event, HAR of PATCH, DELETE, reaction, open-DM | M2.2, M2-S5 |
| 13 | **Search, timeline, files and canvas with per-hit authority.** Search p95 under 300 ms at `limit=20` on 50,000 messages with `idx_messages_space_fts` in the plan; every hit is re-authorized: a revoked B gets 0 hits, a ZDR credential or `zdr_waived` thread yields 0 hits and 0 stored query text; hidden, missing and foreign ids return identical not-found. A 1,500-thread room keeps under 450 DOM posts with p95 scripting under 8 ms per scroll frame. Files: a 10 MiB upload passes, 30 MB is `room_file_too_large`, B lists and previews, a non-member and a removed member get 0 files, delete removes the grant. Canvas: of 20 concurrent saves exactly 1 wins (19 get `canvas_head_moved`), restore creates a new version, 0 canvas text in logs | `EXPLAIN` and latency on the 50,000-row copy, Playwright frame-time trace, `documents` and `resource_grants` rows, wiki-store version rows | M2.3 (M2-S6, M2-S7, M2-S8); canvas M3.1 (M3-S5) |
| 14 | **A personal agent cannot read another room's memory.** A room turn creates 0 `agent_memory` rows and 0 Letta calls (containment, shipped in M1), personal memory never appears in a room turn (`memory_recall` frame count 0), a legacy leaked row is withheld from every recall and shown withheld to its owner. With labels on, an agent in room R2 recalls 0 rows labelled for R1 (resolver parity test). A personal agent is invisible to a manager not on the owner's access list; admission is 403 until the entitlement bit, then 201; removing the owner suspends admission within the request, deprovisioning suspends all of the owner's agents within 60 s; a declassified memory is recalled only in that room and stops after revocation. Baseline: 1.3 #5 | SQL on `agent_memory`, `dream_runs` (`skipped_space_scope`), `space_agent_admissions`, letta-bridge log, `memory_recall` count | containment M1 (M1 gate 4); M3.1, M3-S1..S3 |
| 15 | **The agent self-API acts only on admitted actions.** An agent can call only actions present in the registry golden and in `CONTROL_SPACE_SELF_API_ACTIONS` (first set: `space.thread.rename`, `space.thread.pin`; then `space.thread.post_card`); 1 content-free receipt per action; a human-set title is never overwritten (`title_owned_by_human`); a viewer's or removed member's run gets 0 decisions; a forged, expired or foreign token is refused by Session Core; ZDR gets no tool; the model cannot name another thread; emptying the env var removes the tool at the next run | SQL on `threads.presentation_title`, `title_source`, `run_thread_action_receipts`, decoded claims (no signature material), registry sync script | M3.1, M3-S4 |
| 16 | **A room browser run is governed and the room can take over.** An editor starts a run bound to the room; B sees the card within 6 s with host-only steps; the run grant is narrower than the session grant; a viewer or non-entitled start is 403 and no Control token reaches the browser; navigation outside the grant is refused by Quarry; a CAPTCHA raises a hand-off card; the starter takes control and gives it back, observers see `withheld` and agent steps are refused while a human holds control; the grant is revoked at run end and a mid-run revoke or member removal closes within 15 s; a third concurrent run is 429 `browser_capacity`; an IP-literal domain is 400. Baseline: 1.3 #3 | `runs` and `events` rows (hosts and ids only), broker `ValidateGrant`, Quarry control events, HAR, Playwright trace | M3.2, M3-S6 (sign-off of S6 onward needs sibling (a) F-t2-001 and F-t2-013 phase 1 in the same stack) |

Applies to every criterion:

- **Kill-switch drill.** Each flag in the delivering design is flipped off once on the rebuilt stack; the feature returns to the legacy path with no data change.
- **Strengths do not regress** (audit section 8): signed effect-bound decisions, deny-by-default effect policies, no widening of owner-plane ACLs, fail-closed ZDR, agents as Control service subjects, gateway binding policy, durable approvals, per-fire re-authorisation, honest-state `unavailable[]` UX. Each design carries guard tests; a PR that weakens one is rejected.
- **Client baseline** (WS07, WS16, in M1 and M2.3; scenarios M1-S7, M2-S8): a swapped build shows the new-version banner with no lost draft, a chunk-load failure reloads exactly once then shows a card, a room without `thread_read_entitled` shows 1 restricted notice with 0 `/transcript` requests, a `javascript:` link renders as text, an agent artifact is visible to B in the Artifacts tab, a preference set in browser A is present in a cleared browser B.
- **Pass rule** (section 6): positive path green; each negative refused with the typed code and no side effect; canary scan clean; baseline diff shows no new failing test; evidence bundle attached to the PR with a dated note beside `apps/Frontend Plane/verevonv3/docs/SPACE_*`.
- **ZDR** is stated and tested per slice as typed refusal, refs-only, or local-only; the UI says what is unavailable rather than failing silently.

### 1.3 Scope boundaries

**In scope.**

| Part | What | Depth |
|---|---|---|
| Priority items | all 23 audit Priority items (F-t1-001..024 except 005, which is Recommended after verification) | detailed designs and PR-sized slices |
| Waves W1 and W2 | the 17 Recommended items that belong to them (F-t1-005, 030, 031, 032, 033, 034, 039, 040, 041, 042, 048, 049, 052, 055, 057, 062, 063) | detailed designs and slices |
| Detail total | 20 workstreams WS01-WS20, 40 audit items, 187 firm slices plus 10 baseline and prep rows (`M0-*`, `P-*`) | built behind flags, dark first, slice by slice |
| Later waves | the other 56 audit items as 12 epics E01-E12 | goal, first slice, prerequisites, effort range, trigger only; not scheduled, not in the totals |
| Conditional slices | 14 design slices (27.0-42.5 ew) gated on a decision, a sibling delivery or a measured trigger | sized, scheduled last, excluded from firm totals; `WS08-S9..S11` outline, not sized |
| Rollout | M4: per-org flag flips, kill-switch drills, evidence filing | about 0.2 ew per flag flip, not in the totals |

**Out of scope (not built by this plan).**

| Not built | Why and where it goes |
|---|---|
| Channel Plane runtime, external channels, linked identities, cross-org rooms, chat-app push | Channel Plane is future/docs-only (repo rules). Epic E06 after ADR 10. |
| Calls, huddles, screen share, meeting intelligence, agents in calls, voice front (F-t1-029, 050, 066, 067, 068, 084, 085, 086) | Epic E05, decision-gated on ADR 7 (the owner must reverse the huddle non-goal in `apps/Frontend Plane/verevonv3/docs/space-defenition.md`); only the decision-free E05-S1 may start. |
| Desktop shell and multi-window | Epic E11, after the PWA work of E08 and a product decision. |
| Guests, teams and sub-groups, mentionable groups | Epic E06 (needs WS04-S4/S8/S10 and ADR 10); WS04 builds source-scoped membership so they can land later. |
| Persistent per-agent computer beyond confinement, egress authorisation, secrets broker | Epic E02. WS06 builds resource caps, confinement, shell lease and the egress vocabulary; the egress tunnel (`WS06-S11`, XL) is parked. |
| Local executor and Endpoint Plane (F-t1-096) | Parked; needs a registered plane and a device-identity contract first. |
| Anything the sibling sessions own | Quarry v2 and the agent browser engine (live-view transport, grants issuer, challenge handling, frame capture); Model Plane chat and agents (durable run loop and in-place resume, chat UI, memory engine internals, run stream and AG-UI contract, `ForkThread`, run-origin enum, schedule contract). This plan marks touchpoints (section 12 of each design, section 5) and assumes their contracts. |
| Any change to ownership rules | Control owns identity, authority, audit and entitlements; Data owns documents and knowledge; Model owns sessions, runs, execution, sandboxes and browser grants; Application owns realtime projections and notifications; Quarry executes browser actions; no cross-plane database access. No second source of truth: transcript, runs, revisions and the search index stay in Session Core; reactions, attention items, pins and sidebar documents in Convex; files in Data Plane; the canvas in Data Plane wiki-store. |
| Any permissive default | Every new flag defaults off or deny; effect policies stay deny-by-default; ambient agent participation is a per-binding opt-in, default off, not built until cost caps and the approval gate exist (E01, WS10-S3); unknown egress tier means `Disabled`; ZDR never weakened. |
| Retention policies, legal hold, moderation, SIEM export beyond the role-gated audit read | Epic E07 after ADR 8. |
| Non-Space product work in the live tree | Product-demo and marketing changes (58 entries) stay out of every Space change set. |

### 1.4 Plan at a glance

**Workstreams (firm scope).** Wave is the audit's wave; milestones are those of section 4 (M0 baseline and seams, M1 W1 foundations, M2.1 agents in a human room, M2.2 attention, DMs, lifecycle, M2.3 search, files, timeline, UI, identity, member approvals, M3.1 personal agents and canvas, M3.2 room browser). Engineer-weeks (ew) are the designers' slice ranges summed by the sequencing analysis (one engineer who knows the plane; tests, live proof and review included). Size band from the mid value: S under 6, M 6-12, L 12-18, XL over 18. Cond = conditional-slice ew, not in the firm column.

| WS | Title | Audit items | Wave / milestone | Owner plane (also) | Size | ew low-high (mid) | Cond ew |
|---|---|---|---|---|---|---|---|
| WS01 | Human posts, reliable send, thread pane | 001, 033 | W1 / M1 | Model (Control, Frontend) | M | 9.3-13.8 (11.6) | 0 |
| WS02 | Space realtime substrate | 003 | W1 / M1 | Model (Control, Application, Frontend) | M | 5.8-10.6 (8.2) | 2.0-4.0 |
| WS03 | Governance admin, phase 1 (effect-policy admin, typed denials) | 006 (phase 1) | W1 / M1 | Control (Frontend) | M | 6.5-9.8 (8.1) | 0 |
| WS04 | Room administration, membership writer, directory | 022, 048, 049 | W1-W2 / M1 (S1-S7), M2.2 (S8-S12) | Control (Application, Frontend) | L | 11.8-18.2 (15.0) | 0 |
| WS05 | Security fixes and enterprise identity | 031, 023 | W1 fixes, W2 / M1 (S1-S3), M2.3 | Control (Ingestion) | XL | 15.6-24.2 (19.9) | 0 |
| WS06 | Model Plane safety hardening (ZDR, sandbox caps) | 005, 021 | W1-W2 / M1 (S1-S5, S7), M2.3 | Model (Ingestion, Control) | M | 7.9-14.5 (11.2) | 8.5-14.5 |
| WS07 | Frontend quick wins, agent artifacts in the room | 024, 030, 063, 014 | W1 / M1 | Frontend (Model) | M | 7.1-11.1 (9.1) | 0 |
| WS08 | Addressed agent wake-up, identity-bound mentions | 002, 007 | W2 / M2.1 | Frontend (Application, Model, Control) | L | 13.5-19.5 (16.5) | 0 |
| WS09 | Agent work visible to the room, run queue, steer | 004, 040 | W2 / M1 (S1), M2.1 | Model (Control, Frontend) | L | 11.0-14.7 (12.8) | 2.0-3.0 |
| WS10 | Approvals: scopes, durable resume, quarantine | 010 | W2 / M2.1, M2.3 | Model (Control, Frontend) | XL | 17.5-27.0 (22.2) | 0 |
| WS11 | Notifications, cross-room attention, read state | 008, 009, 042 | W2 / M2.2 | Application (Model, Control, Frontend) | XL | 15.8-22.7 (19.2) | 0 |
| WS12 | Direct messages, message lifecycle, reactions | 016, 017 | W2 / M2.2 | Control (Model, Application, Frontend) | L | 12.0-17.5 (14.8) | 0 |
| WS13 | Message search, permalinks, command layer | 018, 039, 041 | W2 / M2.3 | Model (Frontend, Control) | L | 10.1-14.9 (12.5) | 5.5-7.5 |
| WS14 | Attachments, file library, artifact sharing grants | 013, 057 | W2 / M2.3 | Data (Ingestion, Control, Frontend) | XL | 15.5-21.5 (18.5) | 0 |
| WS15 | Composer for real work, scalable timeline, typed entries | 032, 034, 052 | W2 / M2.3 | Frontend (Model, Application) | L | 11.0-16.9 (13.9) | 2.5-4.5 |
| WS16 | UI quality baseline and sidebar organisation | 062, 055 | W2 / M2.3 | Frontend (Application, Control) | L | 9.5-14.5 (12.0) | 0 |
| WS17 | Personal agents and audience-safe agent memory | 011, 012 | W3 Priority / M1 (S1, S2), M3.1 | Control (Model, Data, Application, Frontend) | XL | 17.8-25.7 (21.8) | 0 |
| WS18 | Agent self-API on Space actions | 019 | W3 Priority / M3.1 | Model (Control, Frontend) | L | 11.5-17.5 (14.5) | 5.0-7.0 |
| WS19 | Room canvas | 015 | W3 Priority / M3.1 | Data (Control, Frontend, Application, Model) | L | 11.0-17.0 (14.0) | 1.5-2.0 |
| WS20 | Room-scoped agent browser | 020 | W3 Priority / M3.2 | Ingestion (Model, Control, Frontend) | XL | 19.8-28.7 (24.2) | 0 |
| | **WS01-WS20** | 40 items | | | | **240.1-360.3 (300.2)** | 27.0-42.5 |
| | M0 baseline (`M0-BASE`, `M0-T0`, `M0-CI`) | | M0 | Platform | | 1.8-3.0 (2.4) | 0 |
| | Prep seams `P-*` (7 rows: session-core, gateway, user-core, Convex, SPA x2, Control config) | | M0, M1 | all | | 5.5-8.4 (7.0) | 0 |
| | **Plan total (firm, 197 slices)** | | | | | **247.4-371.7 (309.5)** | 27.0-42.5 |

Planning figure: mid plus 15 percent for integration and rebases on two moving sibling branches = **356 ew**; the high column is a pessimistic bound, not a probability. Designer sums are adopted over the audit's size labels (the audit-equivalent was 76-152 ew for WS01-WS20; 2.6x on mid, reasons in 4.7.2). Not in any figure: the epics, conditional slices, M4 rollout, audit items outside WS01-WS20, the Support Plane.

**Milestones and release lines** (ew firm, mid; calendar weeks at mid, milestone-ordered, 0.8 productivity; 3 / 5 / 8 engineers).

| Release | Content | Firm ew low-high | Weeks, 3 / 5 / 8 eng |
|---|---|---|---|
| R1 = M0 + M1 | people post, live room, governance console, room admin, security fixes, contained memory | 50.4-80.1 | 34 / 21 / 17 |
| R2 = + M2.1 | agents in a human room: addressed wake, live run view, approvals (owner path) | 83.9-128.8 | 54 / 39 / 29 |
| R3 = + M2.2 | attention, DMs, edit and delete, membership depth | 116.9-177.2 | 83 / 50 / 44 |
| R4 = + M2.3 | search, files, timeline, UI baseline, SSO and SCIM, member approvals (M2 complete) | 189.5-286.5 | 128 / 89 / 46 |
| R5 = + M3.1 | personal agents, labels, self-API, canvas | 227.5-343.0 | 147 / 104 / 53 |
| R6 = + M3.2 | room browser | 247.3-371.7 | 168 / 124 / 67 |

Critical path about 19 ew at mid (WS20 chain, roughly 24 calendar weeks with unlimited engineers). A 3-engineer team needs about 3 years for everything and should stop at a release line; M1 is the only path-bound milestone (about 14 weeks at best).

**Later-wave epics** (outline only; effort in ew, low confidence, plus or minus 40 to 50 percent; none is in the totals above). Start order is section 3z's.

| Epic | Title | Audit items | ew | Earliest start |
|---|---|---|---|---|
| E01 | Agent operations and multi-agent rooms (cost, guardrails, delegation, task tracker, allowlist residual of 006) | 027, 045, 025, 056 | 26-42 | cost slice after W1; multi-agent after WS08 and WS10-S3 |
| E08 | Push, PWA, DND, digest, catch-up | 035, 059, 060, 061, 092 | 18-30 | attention slice after WS11 |
| E03 | Room automations and webhooks | 036, 047, 074 | 16-25 | after E01-S1 and WS10-S3; ruling C9 |
| E04 | Agent tools, forms, history answers, file knowledge | 026, 037, 038, 046 | 20-32 | after WS13, WS14, WS15 |
| E07 | Retention, moderation, compliance, assurance | 043, 051, 077, 094 | 35-59 | after ADR 8 and WS04-S4 |
| E02 | Agent computer, egress, credential custody | 028, 044, 096 (parked) | 20-32 (+12-20 parked) | after WS06, E01-S1, WS17 |
| E06 | Guests, teams, external surfaces | 053, 054, 075, 087, 093 | 34-58 | after WS04-S4/S8/S10 and ADR 10 |
| E09 | Lists, records, room templates | 058, 064 | 8-16 | after WS14 grants phase 1 |
| E10 | Skills, plugins, public API, federation | 065, 071, 072, 076, 081, 088, 083 | 38-68 | API tokens any time; skills after S3.5 |
| E05 | Calls, meetings, voice (decision-gated) | 029, 050, 066, 067, 068, 084, 085, 086 | 43-72 | only after ADR 7 (E05-S1 is decision-free) |
| E11 | Desktop shell and multi-window | 073, 090, 089 | 12-21 | after E08 and a product decision |
| E12 | Client long tail | 069, 070, 078, 079, 080, 082, 091, 095 | 22-40 | on demand |
| | **E01-E12** | 56 items | **292-495** | excluding parked F-t1-096 |

**What the owner must settle first** (full ranked list in section 2.6; "ok" accepts the recommendation): SD-11 implementation base branch, integrator and the single reservation ledger entry (gate G0, before the first non-quick-win slice); SD-00 product stance, recommended 0-C hybrid (chat basics in, ambient agents off by default, calls behind ADR 7); SD-01 Session Core owns human posts; SD-12 ZDR product behaviour for Space; SD-15 one vocabulary with the siblings; SD-02 realtime tier; SD-13 who writes room membership; SD-05 non-owner agent invocation. WS03 (read side), WS05-S1..S3, WS06, WS07, WS01-S7a and WS04-S1..S3 proceed under any answer.

<!-- SECTION COMPLETE -->
