# 2. Decisions first (Space hybrid build plan, section 2)

State as of 2026-10-18. English, no emojis. Slice ids are the designs' (`WSnn-Sk`); the Model Plane plan's workstreams are written `mp-WSnn` and its decisions `DEC-nn`; the Quarry plan's are `q-WSn.X`, `ADR-n`, `OQ-n`, `X-n`. Space decisions get ids `SD-nn` (SD-00 = Decision 0, SD-01..SD-10 = audit ADR 1..10, SD-11 and up = additional) so they never collide with the siblings' ADR-0003/0005/0007/0008/0011 series.

## 2.0 Reading guide and the eight decisions that gate most work

**Deciders.** USER = the product/system owner must answer (recommendation given, "ok" accepts it). PLAN = this plan resolves it by default; the user may veto, cost of reversal stated. SIBLING = already decided in a sibling plan; Space adopts and amends its designs (listed in 2.3.2). Where only a sibling DRAFT exists it is marked.

**Gate codes** (no sequencing document exists yet in `.plan-work/`, so gates are stated against slice order and the audit waves):
G0 before the first Space code PR; G1 before the first merge of any slice that edits `session-core` (`grpc.rs`, `auth.rs`), a `.proto` or adds a migration; G2 before WS01-S3 (member append) and WS02-S3 (stream frames) start; G3 W1 exit / first staging enablement in a real org; G4 W2 start (WS08-WS15 main slices); G5 W2 flag-on in an org; G6 W3 start (WS17-WS20); EPIC = a later epic (E01-E12 in `analysis/03z-epics.md`).

**State of the siblings (verified in git, read-only).**
- Model Plane and chat plan: file `docs/plans/model-plane-chat-build-plan-2026-10-14.md` in worktree `epic-shaw-4d613b` (untracked). Its header still says "DRAFT FOR APPROVAL", but build is under way on local branch `model-plane` (tip `e8f3561c`, 43 commits over `afb83928`, no remote contains it). Landed: the ZDR Z-series wave (`5ae7fe13`..`b67bb7f0`, session-core migrations 0044 and 0045), CI and buf gates (`d4045003`, `e45caa71`), run-context contract (`8cff6738`, `0736d692`), reservation ledger entry 2026-10-15 in `apps/Model Plane/docs/decisions/ledger.md` (`22090c3b`), and mp-WS03 S1-S5 (session-core migrations 0046 `approval_consume_and_mode`, 0047 `run_loop_checkpoints`, 0048 `tool_action_effect_ledger`). Treat its DEC-01..DEC-40 as "adopted for building, not yet approved".
- Quarry v2 and browser plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md` on local branches `quarryv2` and `quarryv2-next` (56 commits over `afb83928`); status line says approved 2026-10-15 with the recommended defaults of all 13 questions accepted. Treat as decided.
- The live tree `C:/dev/CoresSystem` (`main` at `afb83928`) still has 326 uncommitted entries; the Space-relevant Z-series part is now duplicated by committed work on `model-plane`.

**The eight decisions that gate the most Space work** (full text below):

| # | Id | Decision | Decider | Gates (slices) |
|---|---|---|---|---|
| 1 | SD-00 | Product stance: does the hybrid brief override "no chat parity / no auto-responding agents / huddles" | USER | WS01-S3..S7b, WS12 all, WS13-S5..S7, WS14-S8/S9, WS15-S3/S4, WS16-S7..S11, WS11-S4+, E05 |
| 2 | SD-11 | Implementation base branch, integrator, shared reservation ledger, who lands leftover live work | USER | every slice with a migration, a proto tag or an edit to `grpc.rs`/`auth.rs`/`sessions.proto` (about 35 slices) |
| 3 | SD-01 | ADR 1: Session Core owns human posts; member append reuses `thread:append` | USER (accept) | WS01-S2, S3; WS08-S2; WS12-S4..S7; WS13-S10/S11; WS15-S9 |
| 4 | SD-12 | ZDR product behaviour for Space (what ZDR orgs get; observe vs enforce) | USER | WS01-S4/S5, WS02-S4, WS04-S2, WS06-S2/S6b, WS09-S4/S5, WS10, WS11-S6, WS12-S8, WS14, WS16-S8, WS19, WS20 |
| 5 | SD-15 | One vocabulary with the siblings: grant scope, origin, run state, policy registry, run-action table, revisions, queue/steer (C2-C7, C11, C17) | USER (ratify) | WS10-S2, WS08-S2/S4, WS11-S5/S6, WS18-S1/S2, WS03 phase 2, WS09-S9, WS12-S4 |
| 6 | SD-02 | ADR 2: ids-and-kinds stream, authority at subscribe and per delivery | USER (accept) | WS02-S3..S5, WS09-S4, WS11-S5.., WS20-S6, E08 |
| 7 | SD-13 | Who writes room membership (audit 9.4 #4) | USER | WS04-S4 enablement, S6, S8; WS12-S3; WS17-S4; WS05-S9; E06 |
| 8 | SD-05 | ADR 5 second half: may a non-owner member invoke an agent turn in a shared thread | USER | WS08-S10, WS09-S9, WS10-S6/S7, E01 |

ADR 3 and 4 (SD-03, SD-04) gate W3 only (WS17-S3..S6) and are next in size; ADR 6 to 10 gate epics or nothing in WS01-WS20.

**Count.** 1 stance decision, 10 ADRs, 19 additional decisions (SD-11..SD-29), 23 sibling conflicts (C1-C23) of which 15 are decided by a sibling and 8 still need a user ruling or a joint ruling, and 20 per-workstream default sets (PD-WS01..PD-WS20). USER must answer 17 items (2.6); everything else is PLAN default or SIBLING-decided.

---

## 2.1 (A) Decision 0 (SD-00): product stance

**Question.** `apps/Frontend Plane/verevonv3/docs/space-defenition.md` (COMMITTED, last commit `aae03af5`) says in its non-goals (L349-L356) "No auto-responding agents in rooms by default" and "No chat-feature parity race with Slack or Teams (presence, emoji, huddles, notification polish)", and its acceptance criteria (L363) "Agents answer only when explicitly invoked". The brief for this plan is a Teams + Slack + Grok Bot hybrid. Which wins?

**What actually conflicts.** Most of the brief does not.

| Brief element | Doc lines in tension | Verdict |
|---|---|---|
| Human posts, replies, DMs, edit/delete, attachments, search, read state | L24-L27 ("not building a better Slack or Teams"), L52-L54 ("not chat parity"), L352-L353 | Conflicts with the strategy text; not named as non-goals except emoji and notification polish |
| Addressed agent wake-up, several agents per message | none: L162-L164 "A mention invokes; it never grants", L363 | Consistent. Note L294 ("A mention is a signal, not a trigger") contradicts L163 inside the doc; L294 is about gated actions |
| Ambient agent participation (answers unaddressed) | L349, L159 ("No passive or unsolicited responses"), L161 | Conflicts |
| Calls and huddles | L25, L45, L352-L353 | Conflicts (audit ADR 7) |
| Personal agents with own PC and browser | L350 "no global agent available everywhere", L360 binding rule | Consistent if every presence in a room is an explicit binding |
| Delegation re-resolves authority per hop, initiating human recorded | L365-L368 | Already decided by the doc: this settles most of ADR 5 (SD-05) |

**Options.**

| Option | What it is | Unlocks | Blocks / costs |
|---|---|---|---|
| 0-A Hold the line | Keep the non-goals verbatim; build only agent-in-room depth | Smallest scope; WS03, WS05, WS06, WS07, WS09, WS10, WS17-S1/S2, WS18-WS20 proceed | The brief is not met: no human-only posts (WS01-S3..S6), no WS12 (DMs, reactions, edits), WS13 search UI, WS14 files, WS15 composer/timeline, WS16 sidebar, WS11 attention; the room stays an AI prompt box (audit 1.3 #1) |
| 0-B Brief overrides everything | Chat parity, ambient agents default on, calls now | Full parity story | Reverses the doc's safety posture; ambient default multiplies paid turns (audit risk 6; cost caps E01-S1/S3 do not exist yet); E05 calls cost 43-72 engineer-weeks and need ADR 7, an SFU purchase and a Control shared-room import decision first |
| 0-C Hybrid (recommended) | Accept chat parity for the Space chat core (posts, replies, DMs, edit/delete, reactions, attachments, search, read state, notifications); addressed wake-up is the default agent behaviour; ambient participation is a per-binding opt-in, default off, built only after cost caps and the approval gate (E01, WS10-S3); calls and huddles deferred behind ADR 7 with the upload-based meeting MVP (F-t1-050 minimal) as the fallback | All of WS01-WS16 and WS17-WS20; keeps every security principle of the doc (L268-L334 unchanged) | Ambient wake (WS08-S9 outline, E01) and E05 stay parked; "no chat parity race" is rewritten, not deleted |

**Recommendation: 0-C.** Why: the doc's own acceptance criteria (agents answer only when invoked, delegation per hop, no client-authored authority) are untouched by chat basics; the competitive claim ("agent in the room") survives; and without human posts the Slack-class half of the brief cannot exist. Ambient default-off keeps the one real safety line.

**Exact lines to amend in `apps/Frontend Plane/verevonv3/docs/space-defenition.md`** (owner: the doc owner; amend in the first Space PR):

| Line(s) | Today | Amend to (gist) |
|---|---|---|
| 24-27 | "We are **not** building a better Slack or Microsoft Teams ... Fighting there is unwinnable" | "A room must be a real multi-person room (people post and reply without invoking a model; DMs; reactions; search); we still compete on the agent in the room, not on chat polish." |
| 52-54 | "Build that depth, not chat parity." | "Build that depth first; chat basics ship because a room without them is not a room." |
| 159, 161 | "No passive or unsolicited responses." / "No always-on presence by default." | Keep; add "(agent presence; a binding may opt in to ambient participation, default off)" |
| 294 | "A mention is a signal, not a trigger." | Add "for anything gated (approvals, moderation); an @mention of a bound agent is an explicit invocation (L163)" |
| 349 | "No auto-responding agents in rooms by default." | Keep; append "Ambient participation exists only as a per-binding opt-in, default off, rate-limited and cost-capped." |
| 352-353 | "No chat-feature parity race ... (presence, emoji, huddles, notification polish)" | "No chat-parity race beyond the Space chat core (posts, replies, DMs, edit/delete, reactions, attachments, search, read state, notifications). Calls and huddles are not planned until ADR 7 is accepted." |
| 363 | "Agents answer only when explicitly invoked." | "...invoked (an @mention or an Ask control used by a person with wake rights), or when the binding's ambient policy is on (default off). Human-only posts never invoke an agent." |
| 365-368 | delegation and initiating-human criteria | Unchanged; cite as the accepted answer to ADR 5 |
| 376+ (Open questions) | personal-Space agent question "still open as of 2026-09-07" | Re-date after WS17-S4 decision |

**Sibling status.** Not addressed by either sibling. Quarry plan Q7 (approved) already states "Space members cannot view or drive another member's browser"; Model plan DEC-26 keeps `@` for Space agents. Neither conflicts with 0-C.
**Decider:** USER. **Latest step:** G0 (before WS01-S3 and any W2 chat-basics slice). **Meanwhile:** WS01-S1, WS01-S7a, WS02-S1/S2, WS03, WS04-S1..S3, WS05-S1..S3, WS06, WS07 proceed under any option.

---

## 2.2 (B) ADR 1 to ADR 10 (SD-01..SD-10)

### SD-01 / ADR 1: ownership of human room messages and shared-thread append authority
- **Question.** Which plane holds human posts, and who may append to a shared thread?
- **Options.** A: Session Core stays the single transcript owner; reuse the member-capable Control `thread:append` decision (`apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go`); add `post_kind` and DM kinds. B: human posts in an Application store (two write paths on one transcript).
- **Recommendation: A.** Why: `AppendMessage` already carries the Space decision fields; the owner-only check is at `session-core/src/grpc.rs:4504`; ZDR callers are already refused durable writes; Model plan section 5.1 row 1 names Session Core "the single conversation writer; Convex has no message table".
- **Two amendments forced by the siblings.** (1) Space adds only `post_kind`; it does not add a `turn_origin` to the append (DEC-17, SD-15). (2) Send dedupe is not built by WS01: mp-WS04-S6 already plans `AppendMessageRequest.client_message_id = 18`, `AppendMessageResponse.message_id = 2`, `deduplicated = 3` and a unique `(thread_id, client_message_id)` index. WS01-S1's `client_message_key = 19` and `message_send_dedupe` table are dropped; `post_kind` takes the next free tag (19 provisional).
- **Gates.** WS01-S2, WS01-S3 (should wait for acceptance); WS08-S2; WS10-S6, S7; WS12-S4..S7; WS13-S10, S11; WS15-S9.
- **Can proceed without.** WS01-S1 (as amended), WS01-S7a, WS01-S8; WS02-S1..S5 (written for A); WS10-S1..S5; WS12-S1..S3.
- **Sibling status.** Consistent with Model plan 5.1 row 1; the "first to land owns `append_message_in_tx`" rule (2.6 row 12) applies to WS01-S3, mp-WS04-S6, mp-WS17-S8.
- **Decider:** USER (accept). Latest G2.

### SD-02 / ADR 2: realtime delivery tier under ZDR and Space authority
- **Options.** A: frames carry content, authority checked at subscribe only. B: frames carry ids and kinds only; authority checked at subscribe and at every delivery; push is a content-free wake; ZDR orgs get no content cache.
- **Recommendation: B.** Why: matches WS02 (Session Core log, gRPC tail, new Control audience `model-plane-space-stream`), WS09 (per-viewer re-authorised stream) and Quarry plan ADR-8 (replay ring live-only, nothing published for ZDR runs). Verified in the audit: current Control audiences are closed to Session Core, so a new audience is needed (WS02-S2).
- **Amendments.** Run-state kinds in frames come from mp-WS15/WS16's closed enum through one mapping function (C17, DEC-11), not a Space-private vocabulary; the per-user attention hub is Model's (DEC-38, SD-17).
- **Gates.** WS02-S3..S5, WS09-S4, WS10-S6 (live updates), WS11-S5.., WS20-S6; epics E08 (push), F-t1-080.
- **Can proceed without.** WS02-S1, S2; WS09-S1..S3, S7; WS11-S1..S4; all slices poll at 6 s until S4.
- **Hard prerequisite outside Space.** mp-WS01 S2 (audit outbox publisher fix, deployed) before WS02 goes live (Model 2.6 row 20; only `VEREVON_AUDIT` and `mp.v1.run.%.event` subjects are admitted by the outbox today).
- **Decider:** USER (accept). Latest G2.

### SD-03 / ADR 3: agent memory ownership, audience labels, declassification
- **Options.** A: Model Plane (session-core) owns learned memory and the resolver, Data v2 only authored documents; label at write, fold at read, Letta gets the same filter. B: exclude personal memory from shared rooms until labels exist.
- **Recommendation: A, with B as the interim.** Containment first (WS17-S1, S2 in W1, overriding the audit's W3 placement per cross-audit O3), then labels (S3).
- **Why.** Verified leak vector today: a Space message becomes the thread owner's user-level memory with no Space tag.
- **Sibling status: decided by the Model plan.** DEC-06 (memory skipped on Space-scoped Do runs, `SKIPPED_BY_POLICY(space_scope)`); Model 2.6 row 13: one migration `(scope_kind, scope_id)` plus audience predicate `(audience_hash, audience_revision, source_label)`, owner session-core, WS17-S1 = F-t3-060 step 1, `mp-WS07` ships the shared helper first, Space applies it to Dreaming and the Letta bridge. WS17-S3's label schema must adopt `(scope_kind, scope_id)` (C12, C13, C23).
- **Gates.** WS17-S3 (waits for acceptance); WS07-D2 and WS09-D1 (how much step detail non-owners see); WS13 D-2 (index assistant text until labels exist); E02.
- **Can proceed without.** WS17-S1, S2.
- **Decider:** USER (ratify the sibling's answer). Latest G6 for S3; S1/S2 need nothing.

### SD-04 / ADR 4: personal agents as service subjects
- **Options.** A: owner-derived admission, the agent-to-owner link recorded as a Control fact, cascade on ban/suspension. B: Application-only add policy (advisory).
- **Recommendation: A.** Why: every cascade (ban, SCIM suspension, WS05-S9 probe) needs the Control link; B would be advisory.
- **Sibling status.** None. Model DEC-28 (Spaces as chat project) does not touch it.
- **Gates.** WS17-S4, S5, S6; the agent half of WS05-S9; E06 moderation.
- **Can proceed without.** WS17-S1..S3; all of WS05 except the agent cascade.
- **Decider:** USER (ratify). Latest G6.

### SD-05 / ADR 5: delegation and whose authority an agent acts under
- **Already decided in the doc:** L365-L368 (delegate only inside one Space, each hop re-resolves its own authority, initiating human recorded, approvals belong to that person). Recommendation: re-resolve per hop, record the initiating human; formalise, do not reopen.
- **The open half (new, decide now).** A non-owner member who replies in someone else's thread cannot run an agent there: the run belongs to the thread starter (WS08-D-1, WS09-D4). Slack-class rooms normally allow it.
  - Option 1 (assumed, WS08): only the thread owner's own post wakes; members "ask in your own post". Honest UI copy required.
  - Option 2 (WS08-S10, WS09-S9): member-invoked runs owned by the invoking member in the same thread; needs a Session Core member start gate, owner-bound read changes, quarantine (WS10-S8) and the Control run-steer decision.
- **Recommendation:** Option 1 for W2, fund Option 2 after W2 exit (it is the largest single UX gap left in a "multi-person room").
- **Sibling status.** Model DEC-07: member steering waits for a Control "run steer" decision (WS09-S7 is exactly that deliverable); mp-WS04 bus will accept an `authorized_actor`. Model DEC-16 covers unattended runs only.
- **Gates.** WS08-S10; WS09-S9; WS10-S6, S7; WS18 D-4 (pin meaning); E01 multi-agent.
- **Can proceed without.** WS08-S1..S8 (Option 1), WS09-S1..S8, WS10-S1..S5.
- **Decider:** USER (fund Option 2 or not). Latest G4 for the statement; G5 for funding.

### SD-06 / ADR 6: agent computer network boundary, persistence, local execution
- **Options.** 1: keep "Model proposes, Quarry executes" and define an equivalent policy point for any new executor class. 2: sandboxes reach the network directly.
- **Recommendation: 1; park enforcement.** Verified: no Space agent path has network today; gaps are resource caps (WS06-S7..S10, no decision needed), a Quarry egress route (WS06-S11, XL, parked) and ZDR handling in sandbox-manager.
- **Sibling status: partly decided.** DEC-22: sandbox only, Endpoint Plane stays out (F-t1-096 stays parked, C20), SandboxBackend trait only with a second backend; mp-WS22-S1 already adds `AcquireLeaseRequest.zdr = 8` and sandbox-manager migration 0005, so WS06-S6b merges into mp-WS22-S1. Enforcement point (C8: Quarry tunnel vs netns/firewall in execution-core) is open in all three plans.
- **Gates.** WS06-S11; E02.
- **Can proceed without.** WS06-S1..S10 (S5 closed egress vocabulary lands in Control claims in lockstep, schema `space-capability-profile-v2`).
- **Decider:** PLAN (defer); USER only when a feature needs network. Latest EPIC.

### SD-07 / ADR 7: calls and meetings
- **Options.** Bought SFU with Control-authorised join and an Application call record keyed by `spaceRef`; child Space kind (amends ADR-0001); keep the non-goal and build the upload-based meeting MVP only.
- **Recommendation:** follows SD-00. Under 0-C: defer all of it, keep the upload-based MVP as fallback; if later wanted: sub-resource plus bought SFU, and the shared-room import decision first (verified critical path, not ASR).
- **Gates.** No WS01-WS20 slice. E05 only (43-72 engineer-weeks).
- **Sibling status.** None.
- **Decider:** PLAN under 0-C; USER if 0-B. Latest EPIC.

### SD-08 / ADR 8: retention, legal hold, ZDR precedence
- **Options.** A: hold over retention over ZDR refusals unchanged, each owner plane runs its own sweeper with honest receipts. B: a central sweeper (needs cross-plane DB access, rejected by plane rules).
- **Recommendation: A**, Session Core's TTL sweeper first.
- **Sibling status: open in both.** Model 5.1 row "Audit, cost, retention" and S-C8: retention value in `space_effect_policies` vs org-core `DataRetentionDays` open; "OWNER to rule precedence" once for all three plans. Quarry ADR-4 matrix keeps every refusal.
- **Gates.** WS19 production for regulated orgs (D-6; canvas pilot-only until answered), WS02-D2 replay window, WS12 D-WS12-2 (original text on edit/delete), E07.
- **Can proceed without.** Everything builds with phase-1 defaults (keep versions, purge on Space deletion and org erasure).
- **Decider:** USER with counsel. Latest G5 (before any regulated org is onboarded).

### SD-09 / ADR 9: message search index placement
- **Options.** A: Session Core expression index, excluding ZDR and waived-run threads, per-hit re-authorisation. B: copy in Data v2 or Convex (second durable store).
- **Recommendation: A.** Why: identical to Model DEC-29 (expression GIN on `messages` in session-core, FTS `simple`, RPC `SearchMessages`); plane rules forbid B.
- **Sibling status: decided.** Residual conflict D-1 (O10): two indexes on the same table. Rule: WS13-S1 uses disjoint partial indexes (`space_id IS NOT NULL`), shares `build_tsquery` with mp-WS17-S3, whichever lands second rebases (also skips revised-away originals, 2.6 row 12). Migration numbers per 2.3.1.
- **Gates.** WS13-S1 (restart if B). **Can proceed without.** WS13-S4a, S6, S7.
- **Decider:** PLAN (accept; zero cost). Latest before WS13-S1 starts (G4).

### SD-10 / ADR 10: external surfaces and guests
- **Options.** Guests as a distinct subject type in auth-core with the S5.1 link graph; or guests as plain org members.
- **Recommendation:** distinct subject type, later. **v1 constraint (decide now):** rooms are single-organisation; designs assume it (WS06 egress intersection, WS04 audience, WS14 grants).
- **Sibling status.** Model DEC-40 parks the Channel Plane (docs-only per CLAUDE.md); Quarry ADR-7 defers external MCP. Both consistent.
- **Gates.** E06, F-t1-075/093 only. **Decider:** PLAN. Latest EPIC.

---

## 2.3 (C) Additional decisions

### 2.3.1 Resources the siblings already consumed or reserved, and Space renumbering

"Taken" = landed on `model-plane`/`quarryv2*` or reserved in a plan table (Model plan 4.3 and 4.4; Quarry plan 2.3 R-1..R-6, R-11, R-32). The Space designs were written assuming 0046 was free; they also collide with each other (session-core 0046 is claimed by WS01, WS02 and WS10; 0048 by WS08, WS12, WS13; user-core 030 by WS09 and WS10; 039 by WS14, WS17 and WS19; WS15 uses 0049 "provisional"). All numbers below are **provisional**: the integrator freezes a number when the PR enters its train (Model ledger entry 2026-10-15; Go services re-run every `*.up.sql` at boot, so SQL must be idempotent; session-core tolerates gaps via `ignore_missing` per mp-WS01-S9).

**Migrations**

| Service | Highest at `afb83928` | Taken by siblings | Space provisional allocation (design -> number) |
|---|---|---|---|
| session-core (Rust) | 0043 | Landed: 0044, 0045 (Z-series), 0046, 0047, 0048 (mp-WS03). Reserved by Model plan: 0046-0066 plus optional WS07.S6 (the plan table already drifted from what landed). Quarry: 0046 `run_trace_events`, 0047 `run_external_content`, 0048 and 0049 (epics). **Next free for Space: 0068** | 0068 WS01-S1 `message_post_kind` (post_kind only; dedupe comes from mp-WS04-S6); 0069 WS02-S1 `space_event_log`; 0070 WS17-S1 `thread_space_kind`; 0071 WS07-S9 `messages_space_artifact_index`; 0072 WS10-S3 `approval_gate_context`; 0073 WS08-S2 `space_mentions_and_wake_origin`; 0074 WS12-S4 `message_revisions`; 0075 WS13-S1 `space_message_search_index` (first line `-- no-transaction`); 0076 WS15-S5 `thread_activity_summary`; 0077 WS11-S2 `messages_space_activity_index` (drop it if WS15-S5's summary table serves the same read, see 2.3.5 #9); 0078 WS11-S5 `space_attention_outbox`; 0079 WS10-S8 `quarantined_outputs`; 0080 WS17-S3 `agent_memory_audience_labels`; 0081 WS17-S7 `memory_declassifications`; 0082 WS18-S3 `run_thread_actions`; 0083 held for WS13-S10 fork lineage only if it is not covered by mp-WS17-S6 `thread_fork_lineage` |
| user-core (Go) | 029 | Model plan: 030 (mp-WS07.S5), 031 (mp-WS24.S7), 032 (mp-WS21.S8). **Next free for Space: 033** | 033 WS03-S1 `space_effect_policy_events`; 034 WS09-S7 `space_run_control_effect_policy`; 035 WS04-S4 `space_membership_source_and_events`; 036 WS10-S2 `space_approval_grants` (standing rules only, see C3); 037 WS10-S5a `space_tool_policies`; 038 WS12-S1 `space_dm_kinds`; 039 WS04-S7 `space_archived_state`; 040 WS14-S2 `resource_grants_space_subject`; 041 WS19-S2 `space_canvas_effect_policy`; 042 WS12-S5 `space_message_lifecycle`; 043 WS04-S8 `space_visibility_directory`; 044 WS20-S1 `space_browser_effect_policy` (column named `browser_entitled`, DEC-19, replaces `browser_run_entitled`; mp-WS21-S8's 032 consumes it instead of adding its own); 045 WS17-S4 `agent_subjects_and_admissions`; 046 WS17-S7 `space_memory_entitlements`; 047 WS04-S11 `space_history_mode`; 048 WS20-S9 `space_browser_control_effect_policy` |
| org-core | 020 | Model 021-023; Quarry 021-024 (siblings already double-claim 021-023). **Next free: 025** | 025 WS05-S6 `org_identity_domains` (was 021); 026 WS05-S8 `org_scim` (was 022) |
| audit-core (embedded list in `internal/store/migrate.go`) | 006 | Model 007-010. **Next free: 011** | 011 WS05-S4 `audit_read_indexes` (was 007) |
| notification-core | 014 | Model 015-017; Quarry 015, 016 (+ epics). **Next free: 018; use 019** | 019 WS11-S9 `register_space_attention_notifications` (was 015) |
| capability-core | 0015 | Model 0016-0026; Quarry 0016 (outcome B) and 0017 (conditional). **Next free: 0027** | 0027 WS18-S2 `space_self_api_capabilities` (was 0016) |
| sandbox-manager | 0004 | Model 0005-0007 | none: WS06-S6b merges into mp-WS22-S1 (was 0005) |
| auth-core | 028 | none | 029 WS05-S7 `sso_enforcement` (unchanged) |
| integration-core / Data (documents-api, wiki-store) / Convex | n/a | no sibling claims found | own numbering as designed (WS05-S2 `0014`, WS14 IC `005`, WS19) |

**Proto tags**

| Message.field | Space design asked | Taken | Space final (provisional) |
|---|---|---|---|
| `AppendMessageRequest.post_kind` | 18 (WS01, WS08 V5) | 18 `client_message_id` (mp-WS04-S6) | 19; drop `client_message_key = 19` |
| `AppendMessageResponse` message_id, deduplicated | WS01 adds | 2, 3 (mp-WS04-S6) | consume, do not add |
| `ThreadSummary.reply_count`, `last_author_subject_id` | 15, 16 (WS01, referenced by WS15) | 15 archived_at, 16-18 attention, 19 fork, 20-22 starred/unread/last_read_at | 23, 24 |
| `Approval` context fields | 14-18 (WS10) | 14, 15 landed; 16 cause; 17 decision record | 18+ |
| `CreateApprovalRequest.context_json` | WS10 | 12 `continuation_mode` landed, 13 cause | 14+ |
| `ApprovalKind` quarantine (WS10-S8) / HUMAN_ASSIST (WS20-S7) | WS20 asked 6 | 6 is the one coordinated Space/mp-WS13 kind (Model 4.3) | quarantine = 6, HUMAN_ASSIST = 7 (or consume Model's `human_assist` card, C-row 2.3.2) |
| `OrchestrationEvent` (WS20-S7) | 24, 25 | 24 RunParked, 25 RunResumed, 26 ToolRetryScheduled, 27/28 Question, 29 RunUsageProgress; 0-23 exist | 30+ |
| `events.proto` enum (WS20) | 147, 148 | 146 last existing; Quarry 150-159, 160; Model 161+ | 147, 148 stay |
| `RunAgentRequest.tool_profile` (WS18-S4) | 18 | 18 run_context (landed), 19 resume (landed), 20 attachments, 21 effort, 22 max_questions, 23 connector_scope, 24 org_policy | 25 |
| `AcquireLeaseRequest.zdr` (WS06-S6b) | 7 | 8 (mp-WS22-S1) | consume Model's |
| `browser.proto` Space fields (WS20-S2) | space binding fields | Quarry R-6 10-59; Model mp-WS21-S4 10-12 | none: Space adds no grant field (C1) |
| New Space services (`SpaceEvents`, `SpaceRoomReadService`, `SpaceHistoryService`, `SpaceAttention`) | new files/services | Model appends new RPCs at the end of `SessionCore` in train order | Rule: every new Space RPC lives in a Space-prefixed service so it never collides with that order |

**Flags, events, names already used (do not reuse).** Flags: `AGENT_CONTINUE_AFTER_APPROVAL_ENABLED`, `SESSION_CORE_LOOP_CHECKPOINT_ENABLED`, `EXECUTION_EFFECT_LEDGER_ENABLED`, `SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED`, `MODEL_GATEWAY_RUN_CONTROL_BUS`, `MODEL_GATEWAY_DURABLE_PLAN_GRANT`, `MODEL_GATEWAY_SHARED_TURN_RECORD`, `MODEL_GATEWAY_SEND_RECEIPTS`, `MODEL_GATEWAY_SHARED_IDEMPOTENCY`; Quarry `QUARRY_EDGE__HUMAN_BROWSE`, `QUARRY_EDGE__ATTACH`, `QUARRY_EDGE__TAKEOVER_ENFORCEMENT`, `QUARRY_ZDR_LOG_FILTER`, `QUARRY_TELEMETRY_EXPORT`, gateway `BROWSER_HUMAN_BROWSE`, `BROWSER_ATTACH`. No Space flag name found identical; re-check at the ledger entry (the memory flags `MEMORY_SPACE_*`/`MEMORY_AUDIENCE_LABELS` against mp-WS07/WS26: UNVERIFIED). Events/types taken: orchestration RunParked, RunResumed, ToolRetryScheduled, QuestionAsked, QuestionAnswered, RunUsageProgress; SSE `retry_scheduled`, `compaction`, `context_change`, `run_parked`, `user_question`, `usage_progress`; notification type `modelplane.scheduled_run_outcome`; NATS `mp.v1.runctl.req.*`, `mp.v1.runctl.run.{id}.cancel`; Quarry `agent.expired`, `agent.budget_exhausted`, `policy.denied`, `agent.attached`, `agent.detached`, `control_requested`, `run_timed_out`, `schedule_skipped`, error codes `USER_TOOK_OVER`, `BUDGET_EXHAUSTED`, `UNSUPPORTED_MEDIA`. Space keeps `space.*` for notification types (Model uses `agent.*`/`chat.*`) and `mp.v1.space.<space_id>.event` for the projection. Shared modules to consume, not rebuild: `mp-contracts::run_context`, `::loop_fold`, `::durable_gate`, `::park_reason`, `::effect_ledger`, `::zdr_refusal`, `::retention`; `ConsumeApproval`; `RunAgentRequest.resume`; `compose_authored_instructions`.

### 2.3.2 Sibling conflicts C1-C23: ruling and Space amendments

Source: `.plan-work/haiku/cross-audit-overlap.md` section 5.1 reconciled against the Model plan (DEC and 2.6) and the Quarry plan (R-n, X-n, ADR-n).

| Id | Topic | Sibling ruling | Space ruling and design amendment | Status |
|---|---|---|---|---|
| C1 | Client-supplied `space_ref` on browser runs | Model b:1685 and Quarry 5.2 S1: server resolves the room; no `space_id`, `team_id` or shared flag in any grant, attach body or `AttachState` | Server resolves from thread/run; Space attribution travels only as a Control decision claim checked at run start (tighten-only). **WS20-S2 loses its proto fields on the broker**; WS20-S3's three-way check stays. If mp-WS21-S4 ships `space_id` on `AcquireGrantRequest`, Space reads it as attribution only | SIBLING |
| C2 | Queue and steer | DEC-07: owner-bound; client-only "Stop and send"; editable queue only behind mp-WS17 `withdrawn` marker; member steer via Control "run steer" decision later | WS09-D3 (refuse with send-when-ready held in the browser) and D4 (follow-up only) already agree; WS15 builds no queue. **Audit F-t1-040 durable leased queue becomes an epic.** WS09-S7 delivers the run-steer decision the Model bus waits for | SIBLING |
| C3 | Grant scope vocabulary | DEC-08: one enum `ApprovalScope {ONCE, THREAD, RUN}`; "always" is a standing rule with `rule_id`, only for `kind=mcp_tool`; provider writes, destructive, owner effects and browser actions are ONCE only; Control signs only Space standing rules; Quarry B7: nothing above once for browser sensitive actions | **WS10-S2 amended**: scope enum ONCE/THREAD/RUN; Control stores signed standing rules only, THREAD/RUN grants live in session-core (mp-WS13-S6, migration 0062 in the plan table); "always" never for provider writes. Rule: any deny beats any grant (Quarry R-7 order). Space records ADR-0011 adoption before any proto field | SIBLING |
| C4 | Origin vocabulary | DEC-17: one closed run-origin enum `{chat, schedule, trigger, api}` in `runs.metadata.origin`, Model-owned; do not widen the six-value thread `origin` CHECK; Control `turn_origin {direct, ambient, automation}` maps onto it, Space owns the mapping; bare "origin" = thread origin | **WS08-S2 amended**: `turn_origin` stays a Control-signed claim (qualified name), stored beside, not instead of, run origin; mapping function lives in Space; WS01 uses `post_kind` only | SIBLING |
| C5 | Policy registry of record | DEC-13: who-may-use (allowed_models, sites, entitlements) in org-core; DEC-14 and 2.6 row 7: how-tools-behave (class floors, modes, hooks) in capability-core; Quarry ADR-1: org-core owns web/browser policy, pulled by revision or signed claim | Split accepted. `space_effect_policies` in user-core stays the **Space ceiling** only (WS03 phase 1 stays on user-core, WS03-D1 A). **WS10-S5a amended**: the org floor is read from capability-core `OrgAgentPolicy`, not stored in user-core. WS03 phase 2 (allowlists) waits for mp-WS10 (DEC-13) | SIBLING (Model DRAFT, Quarry approved) |
| C6 | Run-action table | DEC-25: Frontend owns classification (typed side map), capability-core owns admission rows, Model consumes only those; Control-signed `run-action-v1` framework (mp-WS24 S5-S8) | **WS18 amended**: `spaces.*` rows are entries in mp-WS24's framework, not a Space-only table; WS18-S1/S2 may build the Control half with compatible names; `tickets.create` stays byte-identical | SIBLING |
| C7 | Canvas and version store | 5.1 "Artifacts" row: artifact revisions Model-owned (thread), canvas and durable files Data-owned, link by an explicit "promote" | WS19-D1 A stands: Data owns the canvas; promotion copies text | SIBLING |
| C8 | Egress enforcement point and tier vocabulary | Open in Model (EP-9 epic), Quarry (F-t2-058..060), Space (WS06-D3) | One closed tier vocabulary in the Control decision (WS06-S5); enforcement point deferred; WS06-S11 parked (SD-06) | OPEN (parked) |
| C9 | Schedule and watch owner | DEC-16: Control-signed stored-consent contract, capability-core owns cron and Watch, class ceiling `read`; owner ruling 2026-09-28 "scheduled runs are draft-only" amended not reversed; Quarry ADR-9: Quarry schedules acquisition-only | E03 and WS18-S9 follow Model; WS18-S9 stays gated on mp-WS20. Control `validatePersonalAuthority` blocks shared schedule/watch decisions (`space_capability_decision.go:138`, verified) | SIBLING |
| C10 | Agent recall over room history | Model F-t3-012 keeps recall owner-scoped | No Space slice exposes room history to agents; F-t1-038 stays an epic (E04); WS13 indexes for people only | SIBLING |
| C11 | Message versions vs revisions | DEC-29, 2.6 row 12: tree for personal threads only; Space keeps append-only revision entries; mp-WS17 lands first; first to land owns `append_message_in_tx` | WS12 keeps `entry_kind`/`revises_message_id`; WS12-S4 rebases on mp-WS17's search clause that skips revised-away originals | SIBLING |
| C12, C13, C23 | Memory label schema, containment owner, Letta mirror | 2.6 row 13: one migration `(scope_kind, scope_id)` plus audience predicate; owner session-core; Letta mirror suppressed for room-origin writes | WS17-S1 = F-t3-060 step 1 (no second implementation); WS17-S3 labels adopt `(scope_kind, scope_id)`; letta-bridge hardening stays in WS17-S3 | SIBLING |
| C14 | Two composers | WS15-D6: WS15 owns `useHomeDraft` scope and shell, F-t3-052 (Model epic EP-3) adopts | WS15 as designed | SIBLING |
| C15 | Audit and SIEM pipeline | DEC-35: audit-core hash chain and `siem-exporter` are Model (mp-WS06); Space lands the `GET /v1/audit` role-gate fix and the admin view (F-t1-031) | WS05-S1, S4 stay Space; **WS05-S4 export is bounded and audited only, no SIEM**; WS05-S5 must emit the one event shape mp-WS06 defines; audit-core migration renumbered (2.3.1) | SIBLING |
| C16 | ZDR matrix and posture carrier | DEC-39 and mp-WS01: Model owns the trunk; authored matrix and one refusal list | WS06-D1 A. **WS06-S0 is satisfied by `model-plane` commits `5ae7fe13`..`b67bb7f0`**; remaining work is consuming them (whether `REQUIRE_CONTRACT=1` is set in CI: UNVERIFIED) | SIBLING |
| C17 | Run-state vocabulary and content-free wake | DEC-11: mp-WS16 owns the closed set (needs_input > failed > running > parked > queued > unseen_done > idle); Space signals map `approval_needed`=`needs_approval`, `agent_awaiting`=`needs_input`, `agent_done`=`completed`; one `needs_you` badge | WS02 `run.state_changed.state` and WS11 `runStateToSignal` map through one function from mp-WS16's enum; WS16-S10 sidebar is the single sidebar slice (O8) | SIBLING |
| C18 | Resource taxonomy for sharing | Quarry S5/S8: owner-only run reads in v1, no copying of trace rows or artifacts into a Space; taxonomy decision before sharing | WS14-S2 owner-service registry covers documents only; no run or artifact-run sharing | SIBLING |
| C19 | Outward actions and connector content | DEC-06, DEC-26: connector tools refused when `space_id` is set; viewer-bound reads | E02/E04 only; no WS01-WS20 slice | SIBLING |
| C20 | Endpoint Plane | DEC-22: sandbox only, Endpoint Plane stays out | Drop the name; F-t1-096 parked | SIBLING |
| C21 | New user-content origin | DEC-40: MCP Apps on the existing sandboxed frame first | WS07-D1 A (opaque-origin srcdoc iframe, click-to-run) | SIBLING |
| C22 | Trajectory capture under ZDR | EP-14: refuse for ZDR | No Space slice persists model requests | SIBLING |
| (O9) | Upload path | Model mp-WS08: bytes ride requests (1 MB caps, `RunAgentRequest.attachments = 20`); durable files are Data | WS14-S8 per-turn attachments consume mp-WS08's contract; durable room files stay WS14-S3 (Data) | SIBLING |
| (S2 Quarry) | Members viewing another member's browser | Quarry Q7 approved: owner-only live frames (JPEG screencast unmasked), receipts masked on read | **WS20-S6 (relay of frames to room viewers) and WS20-S9 (manager takeover) are blocked** until the user reverses that default; WS20-S1..S5 (card, receipt-based evidence view) and S8 (starter's own view) proceed. See SD-23 | CONFLICT, USER |
| (human_assist) | Hand-off representation | Model plan row "Takeover / human assist": Model owns the `human_assist` request/resume card; Quarry enforces control epoch | WS20-S7 re-scoped to Space addressee routing and card copy on top of Model's card; HUMAN_ASSIST kind is not minted by Space | SIBLING |

### 2.3.3 Space-owned decisions that block building (SD-11..SD-29)

| Id | Decision | Options (one line each) | Recommendation | Gates | Decider | Proceed meanwhile |
|---|---|---|---|---|---|---|
| SD-11 | Implementation base branch, integrator, reservation ledger, leftover live work | (a) new local branch `space` cut from the `model-plane` baseline (after the commit that carries the ledger entry, `22090c3b`; the Model plan's tag `mp-baseline-0` does not exist, so the exact cut is UNVERIFIED), rebased on `model-plane` per train, numbers claimed in one append-only entry added to the Model ledger tables 4.3/4.4; (b) wait until `model-plane` reaches `main`; (c) build on `main` `afb83928` with hand-staged hunks as Quarry does | (a). Space WS01/02/09/10 edit `grpc.rs`, `auth.rs`, `sessions.proto`, which `model-plane` already changed (+0044..0048); name one integrator for all three plans (Model DEC-39, rank 1 in its 7a, still open). Space-only leftovers in the live tree: `SpaceRoomComposer.tsx` (2 lines, `translateApiError` feature field) and its test, `execution-core/src/workspace_promote.rs` (7 lines, typed ZDR refusal); the owner lands or discards them, Space depends on neither. After `model-plane` merges, the owner must discard the duplicate dirty Z-series files in the live tree rather than commit them | all migration/proto/`grpc.rs` slices (G1) | USER | Pure Frontend, Application and Ingestion slices (2.5) |
| SD-12 | ZDR product behaviour for Space (audit open question 5; WS01-D3, WS02, WS04-D6, WS06-D2, WS09-D5, WS10, WS11-D4, WS14, WS16-D3, WS19-D5; Model DEC-32 for waived runs) | A observe first: Space privacy flag stays "not applied", new durable features refuse for ZDR callers with typed refusals, in-room stream falls back to polling, enforce flip after one release; B enforce thread writes now; C enforce plus scratch-only sandboxes (WS06-S6b) | A. One product statement for ZDR orgs: no human posts, canvas, files, approval resume, in-app signals or room stream; typed refusals and a visible note; a waived run counts as ZDR for every new store (DEC-32) | WS06-S2 enforce flip, S6b, WS01-S4/S5 split, WS02-S4, WS09-S4/S5, WS11-S6, WS12-S8, WS14, WS16-S8, WS19-S3 | USER (counsel for wording) | Everything ships with the refuse/poll-only defaults |
| SD-13 | Who writes room membership (audit 9.4 #4, open question 4, WS04-D-WS04-1) | A Application intent record stays the writer (cannot enforce rank, clobbers self-join); B Control source-tagged writes with a source-scoped declarative sync; C Control only incl. the org room (needs SCIM) | B | WS04-S4 enablement in production, S6, S8; WS12-S3 enable; WS17-S4 cascade; WS05-S9; E06 | USER (Control lead) | WS04-S1, S2, S3, S5, S9, S10; build S4 dark |
| SD-14 | Registration-shape bundle that amends ADR-0001 (the Control registration ADR): `archived` as a Control read-class state (WS04-D3), DM kinds `dm`/`group_dm` (WS12-D1, D5), visibility default (WS04-D2), co-owners (WS04-D4), topic under ZDR (WS04-D6) | Take them together as one ADR-0001 amendment, or reject archive as Control state (then Application lifecycle value, which breaks reads) | Accept: private default with per-org opt-in and full history; several owner rows assignable only by an owner; archive as Control `registration_state`; DM roster fixed at creation; topic stored as a label under ZDR | WS04-S7, S8, S11; WS12-S1..S3 | USER (architecture owner) | Everything else in WS04, WS12-S4..S8 |
| SD-15 | Shared vocabularies with the siblings (C2-C7, C11, C17 above) | adopt the sibling ruling per row | Adopt all; ratify once in the reservation ledger entry | WS10-S2, WS08-S2/S4, WS11-S5/S6, WS18-S1/S2, WS03 phase 2, WS09-S9 | USER (ratify) | Schema-free parts of each slice |
| SD-16 | Default Send and default room agent (audit open question 7; WS01-D2, WS08-D-7) | (a) Send posts to humans; the default Verevon assistant stays a bound agent reachable by @mention or an explicit Ask control; (b) Send keeps waking the room agent; (c) remove the default agent | (a), consequence of SD-00 0-C; ship both modes behind a prop (WS01-S6) | WS01-S6 default, WS08-S6 | USER | Both modes built |
| SD-17 | Attention transport and badge semantics (WS11-D1/D2/D3/D6, WS16-D5; Model DEC-11, DEC-38) | Convex projection plus own stream vs joining the Model per-user hub in session-core as second upstream | Convex projection for Space items; per-Space stream is WS02; the cross-room aggregate becomes a second upstream of the Model hub when it exists (DEC-38 says Space must confirm), poll aggregate until then; default level `mentions` with muted counts; one `needs_you` badge; notification unread excluded from the badge; route `/attention`; notification type namespace `space.*` | WS11-S5..S9, WS16-S10, WS02-S7 | USER (confirm) | WS11-S1..S4, S10 |
| SD-18 | Entitlement bits and attestation: many `*_entitled` columns default FALSE (`run_control`, approvals, `dm_create`, `personal_agent_admit`, `canvas`, `browser`); who may set them before the WS03 admin exists; who attests Space ZDR for self-serve orgs (WS03 OQ1) | operator script only; org owner; org owner or admin | Writes through the audited WS03-S1 admin path, operator script for dev and pilots, org owner or admin per the gateway's existing rule; attestation stays operator-only until a DPA flow exists | enablement of WS09-S7, WS10-S2, WS12-S5, WS17-S4, WS19-S2, WS20-S1 | USER (attestation); PLAN (rest) | All builds dark |
| SD-19 | Message lifecycle and DM policy (WS12-D2..D5, OQ1-7) | original text purge vs retain per org; reactions in Convex vs session-core; who may delete others' posts; DM entitlement; DM when creator is erased | purge by default with per-org `retain`; reactions in Convex (S8 swappable); room manager/owner delete only, none in DMs; strict DM entitlement; DM erase behaviour needs counsel | WS12-S4, S5, S8 | USER with counsel | WS12-S1..S3 |
| SD-20 | Run visibility (WS09-D1/D2/D4/D5, WS07-D2) | members see assistant text deltas or only step cards; step detail class vs object labels; manager Stop as per-org entitlement; steering; non-owner transcript metadata | text off in production first, class-only steps, manager Stop behind `run_control_entitled` default FALSE, follow-up only, ZDR orgs no run view, allowlisted `artifacts`/`work` metadata for non-owners | WS09-S4/S5/S8, WS07-S9a | USER (privacy) | WS09-S1..S3, S7 |
| SD-21 | Approval semantics (WS10-A..E) beyond SD-15 C3 | approver executes as requester vs approver's authority; who creates "always"; quarantine shadow vs never; action-only continuation vs wait for in-place resume | requester's authority (matches doc L367-L368 "approvals belong to that person"); managers/owners create standing rules; shadow then enforce per Space; action-only continuation plus once-token until mp-WS03 S7. Note: Space runs are never loop-owned on `model-plane` (commit `78a3d169`); Space adds no resumable kinds to the cold worker (WS10-S9 limited to registry and proof) | WS10-S3, S6, S8 | PLAN; USER confirms requester-authority | WS10-S1, S2, S4, S5 |
| SD-22 | Personal-agent and memory first release (WS17 D-1..D-10) | personal agents in shared rooms at all; org-scope recall; private panel memory; who retracts; legacy agents | personal agents in shared rooms only per-org opt-in (`personal_agent_admit_entitled`); org-scope recall excluded; private panel conversation-scope in v1; declassifying owner retracts; legacy agents stay org-visible | WS17-S4..S8 | USER (OQ1, OQ2 are product) | WS17-S1..S3 |
| SD-23 | Room browser sharing (WS20; Quarry Q7, S1, S2) | frames to room viewers vs receipts only; manager takeover; who may start; limits | v1: receipts-based evidence for the room, live frames for the starter only, any editor may start once `browser_entitled`, limits 10 min / 2 runs per Space / 8 hosts, ship behind a dev flag until q-WS1 grants exist (F-t2-001) | WS20-S6, S8, S9 | USER (reverses an approved Quarry default if frames are wanted) | WS20-S1..S5 |
| SD-24 | Enterprise identity commercial choices (WS05 D1-D7, OQ1-8) | Trust Center org-wide redacted view; SSO by email domain; SCIM host; deactivate meaning; IdPs; SAML or OIDC first; plan gating (`sso`, `audit_logs`) | technical defaults as designed (members see the redacted projection, SSO by domain, SCIM in org-core, deactivate = remove Better Auth member row plus session cleanup, redact legacy fake SCIM payloads to `{}`); user names IdPs, plan gating, SAML scope | WS05-S4 export, S5, S6, S7, S8 | USER (commercial) | WS05-S1, S2, S3 (security fixes) |
| SD-25 | Realtime parameters (WS02-D1..D4) | gRPC tail vs NATS fan-out; replay 14 days; revocation SLO 10 s plus bus delay (2 min hard) vs strict per-lease; personal Spaces in; ZDR note | gRPC tail with Postgres NOTIFY; 14-day replay with opaque tombstone ids; default SLO; personal Spaces in; visible "live updates off" note for ZDR | WS02-S3, S6 | PLAN | WS02-S1, S2 |
| SD-26 | Wake policy (WS08-D-2..D-8) | default wake right; agents per message; `@room`; room context names to the model | any editor/manager/owner wakes any bound agent, `respond_to` default `members`; at most 3 agents per message, sequential; `@room`/groups excluded; room context off; retire `group` trigger | WS08-S5, S6 | PLAN | WS08-S1, S5 |
| SD-27 | Operator facts and test seeding | second dev member for live proof (WS01 OQ6, WS11 OQ9); deterministic dev model (WS18 OQ8); Convex deploy command (WS08 OQ8); production `USER_CORE_SERVICE_CREDENTIALS` location (WS04 OQ9); saga migrations 022/023/0026/0027 applied in staging (audit Q11); HTTP/2 at the edge (WS02 OQ5); `socat` in the sandbox image (WS06 OQ5) | one seed script owned by WS01; Convex re-push is `docker restart convex-gateway` and waiting for "Convex functions ready!" (project memory, re-verify); the rest are operator answers | live proof of every slice | USER/OPS | Source-verified work |
| SD-28 | Control shared-Space decision family: who builds what | per-WS issuers (WS14-S1 import, WS19-S2 canvas, WS20-S1 browser, WS09-S7 run control, WS10-S2 grants, WS11-S1 bulk read, WS17-S7 declassify) now; schedule, watch, sandbox, egress, process decisions later | Space builds the per-WS issuers; the remaining family (schedule/watch/sandbox/egress/process: `validatePersonalAuthority` blocks them today) is epic E02/E03 and is what the Model plan lists as "Control epic" | EPIC | PLAN | n/a |
| SD-29 | Ownership of shared Space read skeletons | WS07-S9 or WS15-S6 creates `SpaceRoomReadService` | WS07-S9 creates it; WS15-S6, WS11-S2 consume | WS15-S6, WS11-S2 | PLAN | n/a |

### 2.3.4 Per-workstream default sets (PLAN decides; the user may veto)

| PD | Defaults adopted (design ids) | Cost to reverse |
|---|---|---|
| PD-WS01 | origin vocabulary = `post_kind` only (D-1); post floor editor/manager/owner; 8,000-char plain-text cap; owner delete/archive as today (D-4) | cap and floor are constants; D-4 revisit in WS12 |
| PD-WS02 | see SD-25 | S6 strict mode is optional |
| PD-WS03 | D1 A (floors in user-core); D2 write switch `SPACE_POLICY_ADMIN_WRITES` off in production until attestation process (SD-18) is answered; D3 sandbox and processes blocked under Space ZDR until WS06 proves handling; D4 no plan gating; admin = owner or admin; attestation values shown read-only | catalogue flip |
| PD-WS04 | per SD-13/SD-14; email in directory as `list_members` shows today; WS04-S5 owns audience maintenance; 512-person audience cap; 10-minute reconcile | S3 can return names only |
| PD-WS05 | D1 A, D2 A, D3 A, D4 audit-core owns reads/export, D5 A org-core, D6 A redact, D7 A previous role (owner returns as admin) | one-line each, S8 beta ships with B |
| PD-WS06 | D1 A authored matrix, D4 receipt gap closed when the waiver kill switch is first turned on, D5 code `zdr_durable_mutation_forbidden`, D6 Z-series authors land it (already on `model-plane`); resource defaults 3 GB, 3 CPUs, 1536 pids, 4 GiB address space, 8 concurrent spawns, calibrated in S10 | numbers are config |
| PD-WS07 | D1 A opaque-origin iframe, HTML click-to-run; D2 A allowlist; D3 opt-in snippet; D4 summaries from the aggregate; D5 banner, `enforce` only with registered probes; D6 counters only in production; D7 A https/http/mailto (audit text said https and mailto only: decided for the designer, security owner may tighten); D8 new proto service; D9 spike decides | D7 is one constant |
| PD-WS08 | per SD-05 Option 1 and SD-26; D-3 model-gateway client with a new principal; D-4 mention set not Control-signed; D-8 retire `group` | S10 is the member-invoked epic |
| PD-WS09 | per SD-20; D3 client-side hold, no server queue | durable queue is an epic |
| PD-WS10 | per C3, C5, SD-21 | scope enum is the schema; decide before WS10-S2 |
| PD-WS11 | per SD-17; org-scoped status text; 60-room cap, 30-day item life (14 more for done); Home route `/attention` | one constant each |
| PD-WS12 | per SD-19; DM needs `dm_create_entitled` plus the two thread bits | S8 swaps the store |
| PD-WS13 | D-2 exclude waived threads, include assistant text until WS17 labels; D-3 POST; D-4 room pins by role, author pin stays as legacy badge; D-5 batch Control mint (S9 may defer); D-6 no search audit; D-7 `Alt+1..9`; D-8 Model owns `ForkThread`, forward not built; D-9 `simple` config | each is a single clause or constant |
| PD-WS14 | D-1 A owner-asserted delegation; D-2 attach path only writes space grants; D-3 A signed subject; D-4 no originals retained; D-5 plumb `edit`, refuse for documents; D-6 uploader-only delete; limits 25 MB, 10 files, 500 files, 200 MiB text per room | D-4 B is an XL epic |
| PD-WS15 | D1 A read model (system rows merged by the BFF, no second append path; deviates from audit F-t1-052 "server-signed system messages"); D2 keep bump-on-reply; D3 text-only previews; D4 one shared `scroll-anchor-policy.ts`; D6 WS15 owns composer; D7 leave | S5/S6 key only |
| PD-WS16 | D-1 Convex own-row; D-2 adopt local locale once, then server wins; D-3 ZDR keeps a local document; D-4 hide never hides blocking work; D-5 mute = WS11 level `off`; D-6 locale hint in the identity block (needs Model agreement); D-7 hide `fr-FR`, `de-DE` | S7/S8 only |
| PD-WS17 | per SD-03, SD-04, SD-22; D-1 parity with message reads by audience revision; D-9 semantic recall stays off; D-10 `/retrieval-decision` scope added to `spaces:issue` | each assumed row is a flag |
| PD-WS18 | D-1 amended by C6; D-2 A separate self-API admission; D-3 A `tool_profile=space_self_api`; D-4 A owner's pin; first release = rename and pin; cards after WS15-S9; self-API toggle off by default | S8/S9 gated |
| PD-WS19 | D-1 A Data owns canvas; D-2 A Space role; D-3 A direct CAS write, `ask` default, restorable; D-4 one `canvas_entitled`; D-5 A refuse ZDR; D-6 keep all versions, pilot-only until SD-08; D-7 reuse wiki scopes; D-8 no canvas in DMs | S9 needs D-3 choice |
| PD-WS20 | D-1 A server-resolved; D-3 A starter only v1; D-5 human-initiated only; D-6 withheld during take-over | per SD-23 |

### 2.3.5 Where designers disagree with the audit or each other, and what is decided

| # | Disagreement | Decision |
|---|---|---|
| 1 | Audit F-t1-003 puts the event projection in the Application plane; WS02 puts the log in Session Core with a gRPC tail and uses Application only for cross-room (WS11, reserved WS02-S7) | WS02 wins (plane rule: Session Core owns the transcript; Application keeps projections) |
| 2 | Audit F-t1-040 wants a durable leased run queue; WS09-D3 holds the message client-side | WS09 and DEC-07 win; durable queue is an epic |
| 3 | Audit F-t1-002 implies members can wake agents in shared threads; WS08-D-1 allows only the thread owner's own post | WS08 for W2; funding of member-invoked runs is SD-05 |
| 4 | Audit F-t1-052 wants server-signed system messages; WS15-D1 uses a read model merged by the BFF | WS15 wins (no second append path); S11/S12 wait for the user to confirm |
| 5 | Audit F-t1-017 revision entries vs Model version tree | Both live: tree for personal threads, revision entries for Space (DEC-29) |
| 6 | Audit sizes F-t1-005 at S; WS06 has 14 slices | S is true for S0-S4 only; S5-S10 are F-t1-021 (M); S11 egress XL parked |
| 7 | Audit places F-t1-011/012 in W3; WS17 containment (S1, S2) must precede any room agentic run | Move WS17-S1, S2 into W1 (cross-audit O3) |
| 8 | Audit F-t1-033 wants a new Session Core dedupe; mp-WS04-S6 already plans it | Consume mp-WS04-S6 (SD-01) |
| 9 | WS11-S2 (messages activity index, migration A) and WS15-S5 (thread activity summary) both answer "latest activity per thread/Space" | Build WS15-S5 first; WS11-S2 reads its summary; drop WS11 migration A if it serves |
| 10 | WS10-S3 grant tables in Control vs mp-WS13-S6 session-core scope grants | Control keeps standing rules; session-core keeps THREAD/RUN grants (C3) |
| 11 | WS20-S7 hand-off kind vs Model `human_assist` card | Model owns the card (2.3.2) |
| 12 | WS19 audit and designer both put canvas in Data; Model's mp-WS19-style artifact versions stay thread-owned | consistent; C7 |
| 13 | Space `space-defenition.md` L163 vs L294 on mentions | resolved by the SD-00 amendments |

### 2.3.6 What Space owes the siblings (their typed refusals lift only when these ship)

Model plan DEC-06 refuses Space-scoped paths until Space delivers: (1) `space_scope_for_thread(thread_id)` BFF helper and the "Visible to N members" confirmation semantics (mp-WS18; WS01-S5 gateway module); (2) owner-only grant semantics and the Control standing-rule record `{subject, space_ref, agent_ref, action_hash, scope, expiry, revoked}` (WS10-S2); (3) the shared-Space capability decision family (SD-28); (4) the audience predicate `(audience_hash, audience_revision, source_label)` (WS17-S3); (5) the Control run-steer decision so the bus can accept `authorized_actor` (WS09-S7); (6) per-member `thread:append` decisions and `history_from` on member add (WS01-S2, WS04-S11); (7) the `GET /v1/audit` role-gate fix and admin view (WS05-S1, S4); (8) a Space decision on `@` (kept for agents; mp-WS23 uses chips first). Toward Quarry: `space_ref` as run-start attribution only, never in a grant (C1), and a decision on a masked frame stream (SD-23).

---

## 2.4 (D) Gating table

| Decision | Gated slices | Latest step | What proceeds meanwhile |
|---|---|---|---|
| SD-00 stance | WS01-S3..S6, S7b; WS12 all; WS13-S5..S7; WS14-S8/S9; WS15-S3/S4; WS16-S7..S11; WS11-S4+; E05 | G0 | WS01-S1, S7a; WS02-S1/S2; WS03; WS04-S1..S3; WS05-S1..S3; WS06; WS07; WS09-S1..S3, S7 |
| SD-11 base/integrator/ledger | WS01-S1, WS02-S1, WS07-S9, WS08-S2, WS10-S3, WS12-S4, WS13-S1, WS15-S5, WS17-S1, WS18-S3 and every user-core migration; any edit of `grpc.rs`, `auth.rs`, `sessions.proto` | G1 | Slices with no shared number (2.5 list A); other slices can be written, not merged |
| SD-01 ADR 1 | WS01-S2, S3; WS08-S2; WS10-S6/S7; WS12-S4..S7; WS13-S10/S11; WS15-S9 | G2 | WS01-S1 (amended), S7a, S8; WS02-S1..S5; WS10-S1..S5; WS12-S1..S3 |
| SD-02 ADR 2 | WS02-S3..S5; WS09-S4; WS11-S5..; WS20-S6 | G2 (before WS02-S3) | WS02-S1/S2; WS09-S1..S3; WS11-S1..S4; polling |
| mp-WS01 S2 outbox fix deployed (not a Space decision) | WS02 live enablement, WS11-S5 | G3 | build and test dark |
| SD-12 ZDR behaviour | enforce flips and ZDR-specific copy in WS06-S2/S6b, WS01-S4/S5, WS02-S4, WS09-S4/S5, WS11-S6, WS12-S8, WS14, WS16-S8, WS19-S3 | G3 (before any enforcement flag flip) | refuse/poll-only defaults ship |
| SD-13 membership writer | WS04-S4 prod enablement, S6, S8; WS12-S3 enable; WS17-S4; WS05-S9 | G4 | WS04-S1..S3, S5, S9, S10; WS04-S4 dark |
| SD-14 registration bundle | WS04-S7, S8, S11; WS12-S1..S3 | G4 | rest of WS04 and WS12-S4..S8 (design only) |
| SD-15 vocabularies | WS10-S2; WS08-S2/S4; WS11-S5/S6; WS18-S1/S2; WS03 phase 2; WS09-S9; WS12-S4 | G1 for schema-bearing slices, G4 otherwise | schema-free parts |
| SD-16 default Send | WS01-S6 default, WS08-S6 | G3 | both modes behind a prop |
| SD-17 attention transport | WS11-S5..S9; WS16-S10; WS02-S7 | G4 | WS11-S1..S4, S10 |
| SD-05 ADR 5 | WS08-S10; WS09-S9; WS10-S6/S7; E01 | G4 statement, G5 funding | WS08-S1..S8; WS09-S1..S8; WS10-S1..S5 |
| SD-18 entitlements | enablement of WS09-S7, WS10-S2, WS12-S5, WS17-S4, WS19-S2, WS20-S1 | G5 | builds dark |
| SD-19 message lifecycle | WS12-S4, S5, S8 | G4 | WS12-S1..S3 |
| SD-20 run visibility | WS09-S4/S5/S8; WS07-S9a | G4 | WS09-S1..S3, S7 |
| SD-21 approval semantics | WS10-S3, S6, S8 | G4 | WS10-S1, S2, S4, S5 |
| SD-25..26 realtime/wake defaults | none blocking | n/a | all |
| SD-08 ADR 8 | WS19 regulated production; WS12 D-WS12-2; E07 | G5 | pilot-only canvas, phase-1 defaults |
| SD-09 ADR 9 | WS13-S1 | G4 | WS13-S4a, S6, S7 |
| SD-03 ADR 3 | WS17-S3; WS09 detail class; WS13 D-2 | G6 (S1/S2 need nothing, W1) | WS17-S1, S2 |
| SD-04 ADR 4 | WS17-S4..S6; WS05-S9 agent cascade | G6 | WS17-S1..S3; WS05 rest |
| SD-22 personal-agent scope | WS17-S4..S8 | G6 | WS17-S1..S3 |
| SD-23 room browser | WS20-S2 (real use), S6, S8, S9 | G6, and q-WS1 (F-t2-001) grants | WS20-S1, S3..S5 against a fake broker |
| SD-24 enterprise identity | WS05-S4 export, S5, S6, S7, S8 | G4 | WS05-S1..S3 |
| SD-06 ADR 6, SD-07 ADR 7, SD-10 ADR 10, SD-28 | WS06-S11; E05; E06; E02/E03 | EPIC | WS06-S1..S10 |
| SD-27 operator facts | live proof of every slice (seed second member, deterministic model) | with each slice's DoD | source-verified work |

---

## 2.5 (E) Proceed now, no further decision needed

Assumes the plan defaults above and the amendments in 2.3.1 and 2.3.2.

**List A: start now and merge when ready (no shared migration, proto tag or ZDR-policy dependency).**
- Frontend only: WS07-S1, S2, S3 (after S2), S4, S5, S6, S7, S8, S10..S13; WS16-S1, S4, S5; WS13-S4a, S6; WS15-S1, S2, S3, S4; WS01-S7a; WS14-S8.
- Application only: WS04-S1, S2 (spans gateway and SPA, each under 100 lines), WS16-S7; WS04-S9 after S3.
- Control only: WS03-S1, S2, S3 (writes off in production), WS03-S4..S7; WS05-S1 (role-gated audit read, security bug fix), WS05-S3; WS04-S3 (search); WS16-S2; WS11-S1.
- Ingestion: WS05-S2 (SCIM stub to 501, redaction migration `0014`).
- Model, no new migration or tag: WS06-S1, S3, S4, S5, S7, S8, S9, S10 (WS06-S0 is already satisfied on `model-plane`); WS09-S1, S3 (S1 must ship ON by design); WS07-S9a.
- Data: WS19-S1 (own migration in wiki-store); WS14-S1 (Control import decision), WS14-S6 (imports-core).
- Design-only plus fixtures: WS18-S1 registry golden (compatible with mp-WS24 names).

**List B: write now, merge after G1 (needs a reserved number or tag from 2.3.1).** WS01-S1 (amended), WS02-S1, S2 (user-core audience, no migration number needed: merge-ready), WS07-S9, WS08-S1 (Control digest, merge-ready), WS09-S7, WS10-S1, S2 (after the scope-enum amendment), WS11-S2, WS12-S1 (after SD-14), WS13-S1, WS15-S5, WS17-S1, S2, WS17-S3 (after SD-03), WS18-S2, WS20-S1.

**List C: explicitly not now.** Anything depending on SD-00 (human-chat basics beyond S1/S7a), WS20-S6/S8/S9 (SD-23), WS06-S11 (SD-06), WS08-S9..S11 (ambient, member runs, `@room`), WS17-S4+ (SD-04), WS05-S7/S8 (SD-24), WS04-S11 (history window, depends on S5, S8, S10), all epics E01-E12.

---

## 2.6 What the user must answer (ranked; "ok" accepts the recommendation)

| Rank | Question | Recommendation | If it waits |
|---|---|---|---|
| 1 | SD-00: accept chat parity for the Space chat core, addressed wake-up as the default, ambient default-off per binding, calls and huddles deferred behind ADR 7? | Yes (0-C); owner of the doc amends the lines in 2.1 | No human-only posting, no W2 chat basics |
| 2 | SD-11: build on a new local branch `space` cut from `model-plane`, one integrator for the three plans, Space numbers claimed in the Model ledger tables; owner discards duplicate dirty Z-series files after `model-plane` merges | Yes | Number and proto collisions, rebases onto a moving local branch |
| 3 | SD-01 and SD-02: accept ADR 1 (Session Core owns posts) and ADR 2 (ids and kinds only, authority per delivery) | Accept both | WS01-S3 and WS02-S3 cannot start |
| 4 | SD-12: ZDR orgs get a refs-only room (no human posts, canvas, files, resume, in-app signals, room stream), observe first then enforce, one visible note | Yes | Typed refusal copy and the enforce flips wait |
| 5 | SD-15: ratify the sibling vocabularies (scope enum, origin enum, run-state enum, policy split, run-action table, revisions, queue/steer) | Ratify | Schema freezes for WS08, WS10, WS11, WS18 slip |
| 6 | SD-13 and SD-14: Control source-tagged membership writes; ADR-0001 amendment (archive state, DM kinds, private default, co-owners) | Yes | WS04-S4 stays dark, WS12-S3 cannot enable |
| 7 | SD-05: members cannot wake an agent in someone else's thread in W2 ("ask in your own post"); fund member-invoked runs after W2 | Yes | WS08 copy and WS09-S9 undefined |
| 8 | SD-16: Send posts to humans; assistant reachable by @mention or an Ask control | Yes | Default prop stays open |
| 9 | SD-17: Convex attention items, join the Model hub as second upstream, `mentions` default, one `needs_you` badge | Yes | WS11-S5..S9 wait |
| 10 | SD-20: members see step cards (class only), not live text, in production first; manager Stop behind an entitlement default off | Yes | WS09-S4/S5 undefined |
| 11 | SD-19 and SD-08: purge original text on edit/delete by default; hold over retention over ZDR refusals; canvas pilot-only until counsel rules | Yes, counsel confirms | WS12-S4 and WS19 production |
| 12 | SD-22: personal agents in shared rooms only as a per-org opt-in | Yes | W3 scope |
| 13 | SD-23: room members do not see another member's browser frames in v1 (keeps Quarry Q7) | Yes | WS20-S6, S9 stay blocked |
| 14 | SD-24: name the first SCIM IdPs, plan gating for SSO/SCIM/audit export, SAML or OIDC first | Entra and Okta both, SSO and audit export enterprise-only, OIDC first | WS05-S6..S8 sizing |
| 15 | SD-18: who attests ZDR for self-serve orgs, who flips entitlement bits | Operator only for attestation; audited admin path for the rest | Enablement waits |
| 16 | SD-21: approver's action runs as the requester | Yes | WS10-S6 undefined |
| 17 | SD-27: second dev member seed, deterministic dev model, production credential registry location, staging saga status | operator answers | Live proofs |
