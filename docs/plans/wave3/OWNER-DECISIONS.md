# Wave 3 owner decisions (decided 2026-11-04)

Generated 2026-11-04 from `docs/plans/model-plane-depth-register.md` (rows D0-036 and later, D3-261 and later, kind `decision` or status `open (owner)`), after the Wave 3 code build on branch `model-plane`.

Wave 3 was built code-first on the owner's order (2026-11-03): a row that waited on an owner decision was built on its design's **recommended** option, dark, behind a switch that defaults off, and the assumption was recorded. So for most rows below the recommendation is: **confirm the assumed default** (or name the other option, and the slice is changed before its switch goes on). Nothing here is switched on; no decision below is needed to merge the code, only to turn a feature on.

105 open rows in 13 groups.

**Owner ruling 2026-11-04:** "run the recomended and continue": every row below is decided at its recommended option (for rows built dark, the assumed default stands). Switches stay off until each feature's live proof.

## EP-1 (6)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-584 | EP-1-S1 | P1 | A takeover pause needs the loop-owned resume (WS03 continue/loop-resume switches on): with continuation mode cold the delivery worker would try to run browser.observe/open, which it cannot serve. | Decide before the switch goes on (see the row). |
| D3-642 | EP-1-S2 | P1 | Assumed D1 = (a) grant owner of record: the help card's Take over button needs Quarry Q-1 (beneficiary = grant owner_id) and the chat turn's browser binding. Until both are live, the card offers only Skip this site. | Confirm the assumed default (built dark). |
| D3-643 | EP-1-S2 | P1 | Assumed D4 = (a): only Quarry's ChallengeSignal raises a help request. The model has no browser.request_help tool, so page text cannot steer it into sending the person to a page. | Confirm the assumed default (built dark). |
| D3-644 | EP-1-S2 | P1 | Assumed D7 = read_only applies to the agent only; Quarry's K3-6 contract (D2-961) must let the person's own steps type into a read-level run, or a sign-in by hand is refused. | Confirm the assumed default (built dark). |
| D3-645 | EP-1-S2 | P1 | Assumed D9 = (a): a sign-in that leaves the run fence ends in Quarry's navigation refusal, and the login card says sign-in through another site is not supported yet. | Confirm the assumed default (built dark). |
| D3-671 | EP-1-S5 | P2 | The assist TTL may be up to 3600 s while the run grant asks for 1800 s (GRANT_TTL_SECONDS), so any takeover longer than the grant ends in browser_session_lost at give-back; the owner should consider capping the assist TTL at the grant TTL. | Decide before the switch goes on (see the row). |

## EP-2 (13)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-613 | EP-2-S4 | P2 | The knowledge locator is the first GroundingFact of the citation's document, which matches the chunk the context block numbered from only when the pack facts and Grounding.facts share order; a later citation of a second chunk from the same document reuses the first chunk's locator. | Decide before the switch goes on (see the row). |
| D3-621 | EP-2-S8 | P2 | A Do turn from the SPA is a planMode run (READ_ONLY rung). Confirm live that subagent.research_fan_out children run under a plan-mode parent; if they do not, the entry must send an execute-capable turn instead. | Decide before the switch goes on (see the row). |
| D3-676 | EP-2-S5 | P1 | Assumed D1 = (a) research plan state in the stream and assistant message metadata only (no durable run, no table); owner must confirm before DEEP_RESEARCH_PLAN_TURN is switched on. | Confirm the assumed default (built dark). |
| D3-677 | EP-2-S5 | P1 | Assumed D7 = clarifying questions (max 3) are asked by the plan inference and answered in the plan card, carried back in research_plan.answers, never stored in the WS14 question store. | Confirm the assumed default (built dark). |
| D3-678 | EP-2-S5 | P1 | Assumed D12 = research_plan metadata is not persisted on ZDR or waived turns (with_research_plan), while the event still streams; owner of ZDR must acknowledge. | Confirm the assumed default (built dark). |
| D3-679 | EP-2-S5 | P1 | Assumed OD-1 = accept research state in message metadata with no durable run; unattended research (OD8) would need design option (c). | Confirm the assumed default (built dark). |
| D3-690 | EP-2-S9 | P2 | A retry's item is the child run's stored goal, which is the model-written fan-out item text (cut by durable_goal), not necessarily the person's original pasted item. | Decide before the switch goes on (see the row). |
| D3-694 | EP-2-S6 | P1 | D1 assumed (a): deep research state stays in-stream plus message metadata with no durable run; owner must confirm before DEEP_RESEARCH_SECOND_PASS is switched on. | Confirm the assumed default (built dark). |
| D3-695 | EP-2-S6 | P1 | D2 assumed: closed research_ledger v1 stored under artifacts[].provenance.research_ledger on non-ZDR non-waived turns only; the ZDR owner must acknowledge it, and the ledger needs a row in MP/docs/ZDR.md. | Confirm the assumed default (built dark). |
| D3-696 | EP-2-S6 | P1 | D11 assumed: second-pass bearer-window guard reads the JWT exp of the inference and session bearers and skips the pass unless 90 s of work plus 180 s of synthesis fit before min(wall clock, exp - 30 s). | Confirm the assumed default (built dark). |
| D3-697 | EP-2-S6 | P1 | D12 assumed: waived and ZDR turns stream the ledger on the report frame but persistable_artifacts strips it with the provenance stamp. | Confirm the assumed default (built dark). |
| D3-698 | EP-2-S6 | P1 | OD-1 assumed (D1+D2 recommendation); S6 enablement waits on the owner's confirmation. | Confirm the assumed default (built dark). |
| D3-825 | EP-2-S10 | P1 | find_in_page in the Do loop ranks paragraph slices by exact word or prefix match (mp_contracts::page_reader::select_chunks), not grounding::select_passages' stemmed sentence windows, so the chat and Do loops can rank one page differently; one selector needs grounding.rs and relevance::stem moved into mp-contracts. | Decide before the switch goes on (see the row). |

## EP-3 (17)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-591 | EP-3-S2 | P1 | OD-1 and OD-2 (two stores, promote-only link; session-core table, dual-write) are built on the recommendation and still need the owner's ledger entry and the Space session's acknowledgement on overlap-space row 13. | Decide before the switch goes on (see the row). |
| D3-592 | EP-3-S2 | P1 | OD-8 to OD-11 (thread-wide head, Space threads take model revisions only, 200 revisions / 20 MiB caps, waived turns write nothing) are built on the recommendation and need owner sign-off before MODEL_GATEWAY_ARTIFACT_REVISIONS leaves off. | Decide before the switch goes on (see the row). |
| D3-712 | EP-3-S3 | P1 | OD-1 assumed (c): artifact revisions are thread-owned in session-core and the Space canvas is Data-owned, linked only by a one-way promote; this needs the owner's ledger entry and the Space session's acknowledgement on overlap-space row 13 before the switch goes to on. | Confirm the assumed default (built dark). |
| D3-713 | EP-3-S3 | P1 | OD-2 assumed (b): the session-core artifact_revisions table is dual-written next to an unchanged metadata.artifacts, and S3's on mode makes that table the cross-replica compare-and-set. | Confirm the assumed default (built dark). |
| D3-714 | EP-3-S3 | P1 | OD-8 assumed: the artifact head is thread-wide and a branch switch does not move it; revisit when SESSION_CORE_MESSAGE_VERSIONS_ENABLED flips (D2-620..623). | Confirm the assumed default (built dark). |
| D3-721 | EP-3-S4 | P1 | OD-1 assumed (c): artifact revisions stay a session-core thread store and the Space canvas stays Data-owned; owner and Space-session acknowledgement are needed before MODEL_GATEWAY_ARTIFACT_EDITING is switched on. | Confirm the assumed default (built dark). |
| D3-722 | EP-3-S4 | P1 | OD-2 assumed (b): the session-core artifact_revisions table is the compare-and-set authority for user edits, and record_user_revision overwrites this replica's in-memory head with the table's version. | Confirm the assumed default (built dark). |
| D3-723 | EP-3-S4 | P1 | OD-9 assumed refuse: Space-origin threads get 403 artifact_edit_space_thread (session-core enforces it, the panel hides Edit when spaceThread is true). | Confirm the assumed default (built dark). |
| D3-724 | EP-3-S4 | P1 | OD-11 assumed refuse: a thread holding a waived run gets 403 artifact_edit_waived_thread, mapped from session-core's artifact_revisions_waived_thread. | Confirm the assumed default (built dark). |
| D3-730 | EP-3-S5 | P1 | OD-4 assumed = picker button plus /kontekst, no character trigger; needs the owner's confirmation and a ledger entry before switch-on. | Confirm the assumed default (built dark). |
| D3-731 | EP-3-S5 | P1 | OD-7 assumed = owner's own origin=chat threads only, max 20, lazy; S5 refuses every thread ref, so S7 must code the OD-7 refusals (Space, ZDR/waived, current thread) on the server. | Confirm the assumed default (built dark). |
| D3-732 | EP-3-S5 | P1 | When the switch is off, non-ZDR refs are ignored (byte-identical rule) rather than refused with the design's context_refs_disabled, so a stale client with refs on a switched-off gateway loses them silently; the owner should pick one. | Decide before the switch goes on (see the row). |
| D3-769 | EP-3-S6 | P1 | Assumed OD-3 = (a) (recommended, built dark): the model rewrites via update_artifact and the gateway pins the text outside the range to the admission-time head. | Confirm the assumed default (built dark). |
| D3-770 | EP-3-S6 | P1 | Assumed OD-9 for selection edits: refused in a Space (selection_edit_in_space), which the design leaves open for S6. | Confirm the assumed default (built dark). |
| D3-771 | EP-3-S6 | P1 | Waived turns are not refused for selection edits; under OD-11 a waived turn writes no revision row, so an accepted edit persists as metadata only. Owner to confirm or add a refusal. | Decide before the switch goes on (see the row). |
| D3-783 | EP-3-S7 | P2 | Thread reference tools do not check the person's memory controls (recall_conversations refuses when memory use is paused); attaching a thread is treated as explicit consent, and the owner should confirm this. | Decide before the switch goes on (see the row). |
| D3-787 | EP-3-S8 | P1 | OD-1 assumed = (c) two stores with a one-way promote link; owner ruling, ledger entry and the Space session's acknowledgement on overlap-space row 13 are needed before CHAT_ARTIFACT_PROMOTE_ENABLED flips. | Confirm the assumed default (built dark). |

## EP-4 (3)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-446 | EP-4-S4 | P2 | The elicitation revision is pinned to 2025-06-18 (checked against the spec page). Servers on 2025-11-25 negotiate down; any reply other than 2025-06-18 makes the session fall back to 2024-11-05, so such a server never elicits. | Decide before the switch goes on (see the row). |
| D3-453 | EP-4-S5 | P2 | The origin is not part of the idempotency check: a retry with an existing key returns the stored row even if it names a different origin. This is safe because S7's keys are `{run_id}:{step_id}:elicit:<n>`, which no model question uses. | Decide before the switch goes on (see the row). |
| D3-461 | EP-4-S11 | P2 | OD-6 (no dedicated user-content origin in W3) is an owner ruling; the ADR records the agent default as Deferred and stays open until the owner accepts or overrides it. | Decide before the switch goes on (see the row). |

## EP-5 (13)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-802 | EP-5-S2 | P2 | The download is always named deck.pptx so the build command stays constant. A title-derived name would need the sandbox to compute it or a rename on delivery. | Decide before the switch goes on (see the row). |
| D3-805 | EP-5-S7 | P1 | D4 assumed: brand templates are capability-core skill bundles identified by templates/deck-theme.json (reverses the audit's Data Plane wording); needs owner OD-2 before SKILL_DECK_TEMPLATES_ENABLED goes on. | Confirm the assumed default (built dark). |
| D3-806 | EP-5-S7 | P1 | D5 assumed: templates are chosen only by explicit skill id (deck_template) validated server-side; needs owner OD-3 before enabling. | Confirm the assumed default (built dark). |
| D3-807 | EP-5-S7 | P1 | D8 assumed: the list route refuses ZDR with feature skills and reads bundles under the user's capability bearer; if OD-4/D2-383 lands as 'user principals refused for blob reads', GetSkillBlob in deck_templates::template_for_skill will fail and must move to a service principal. | Confirm the assumed default (built dark). |
| D3-843 | EP-5-S3 | P1 | OD-1/D1 assumed (a): the deck manifest is persisted in assistant message metadata `artifacts` as kind `deck`, and the pptx is never stored; this needs the owner's confirmation before MODEL_GATEWAY_DECK_TOOL is turned on. | Confirm the assumed default (built dark). |
| D3-844 | EP-5-S3 | P1 | OD-1/D6 assumed do-not-persist: session_flow::persistable_artifacts drops deck artifacts on ZDR or waived turns, so a waived thread reopens with no deck; this needs the owner's confirmation. | Confirm the assumed default (built dark). |
| D3-852 | EP-5-S8 | P1 | D1 assumed (the deck manifest and its source key live in the thread-owned artifact store, persisted in message metadata); owner ruling needed before MODEL_GATEWAY_DECK_FROM_ARTIFACT is turned on. | Confirm the assumed default (built dark). |
| D3-853 | EP-5-S8 | P1 | D6 assumed (waived turns do not persist the manifest or its source); S8 inherits S3's waived skip and adds no persistence path of its own, so owner sign-off is needed before switch-on. | Confirm the assumed default (built dark). |
| D3-854 | EP-5-S8 | P1 | OD-1 assumed as recommended (thread-owned deck kind, not persisted on waived turns); OD-1 blocks enabling S3 and therefore S8's persistence of the source. | Confirm the assumed default (built dark). |
| D3-861 | EP-5-S6 | P1 | D4/OD-2 assumed: brand templates are capability-core skill bundles, not Data Plane documents; needs the owner's ruling and the ledger entry reversing the audit wording before switch-on. | Confirm the assumed default (built dark). |
| D3-862 | EP-5-S6 | P1 | D5/OD-3 assumed: a template is chosen only by the request's deck_template id, never by keyword or by the model; needs owner acknowledgement and ledger entry before switch-on. | Confirm the assumed default (built dark). |
| D3-863 | EP-5-S6 | P1 | D8/OD-4 assumed: GetSkillBlob for service principals with capability:read only, list/manifest also for users, no ZDR refusal at rest; owner must acknowledge (closes D2-383/D2-384) before SKILL_BUNDLE_READ_RULE_ENABLED or SKILL_DECK_TEMPLATES_ENABLED is turned on. | Confirm the assumed default (built dark). |
| D3-864 | EP-5-S6 | P1 | D9 assumed: the 1 MiB per-file cap stays, so a real brand master.pptx must fit in 1 MiB; verify with a real template. | Confirm the assumed default (built dark). |

## EP-6 (9)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-431 | EP-6-S10 | P1 | OD-5 assumed = (a): the agent allowlist and cap are applied tighten-only from the Convex definition, with no capability-core agent table. The owner must rule before either role switch is turned on. | Confirm the assumed default (built dark). |
| D3-432 | EP-6-S10 | P1 | OD-4 assumed = (b): immutable published versions identified by agent_ref, version and digest. The child's lineage agent_id is agent_ref@version; the owner must rule before switch-on. | Confirm the assumed default (built dark). |
| D3-499 | EP-6-S8 | P1 | OD-4 was assumed to be option (b), immutable versions plus one draft; the builder routes, contracts and page all depend on it and stay dark until the owner rules. | Confirm the assumed default (built dark). |
| D3-557 | EP-6-S2 | P1 | The design's zdr-posture-matrix row `scheduled_run_now` and Rust case `e_zdr_token_schedule_run_now_refused` were not added, because matrix rows are keyed on resolver inputs only. The owner or lead should confirm that the space-surface `refuse` row plus `TestCronRunRefusedForZDRPrincipal` is the accepted ZDR golden. | Decide before the switch goes on (see the row). |
| D3-561 | EP-6-S2 | P2 | Run now refuses only a lapsed approval. It still fires a schedule that is disabled (paused by its owner, after failures, or after a sweeper authorization refusal), on the grounds that the creator pressing it is an explicit choice. Control re-decides each fire; this should be confirmed under OD-1/OD-8. | Decide before the switch goes on (see the row). |
| D3-564 | EP-6-S9 | P1 | OD-5 was assumed (a): the allowlist and autonomy cap live in the Convex definition and apply tighten-only at invoke; the owner must confirm before `AGENT_VERSION_PINNING` or `MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST` is switched on. | Confirm the assumed default (built dark). |
| D3-565 | EP-6-S9 | P1 | OD-4 was assumed (b): immutable published versions plus one draft, with bindings pinning via `pinnedVersion`; the owner must confirm before S9 is switched on. | Confirm the assumed default (built dark). |
| D3-627 | EP-6-S3 | P2 | With CRON_RUN_NOW_ENABLED off, paused reasons ('Pauset etter feil') still show in chat, Settings and the agents card, because the reason now comes from last_outcome, which the list returns whatever the flag; the design requires it but no switch gates it, so the owner should confirm. | Decide before the switch goes on (see the row). |
| D3-629 | EP-6-S3 | P2 | Run now is offered on paused schedules because the server allows it (it checks creator, approval and limits, not enabled); OD-1 does not rule on paused schedules, so the owner should confirm. | Decide before the switch goes on (see the row). |

## EP-7 (7)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-299 | EP-7-S3 | P1 | D3 assumed as the design recommends (separate aux-model judge, closed verdict set, met requires a supplied evidence id, user override both ways); the owner must confirm before MODEL_GATEWAY_GOAL_JUDGE_ENABLED is switched on. | Confirm the assumed default (built dark). |
| D3-308 | EP-7-S5 | P1 | D1 was assumed on the design's recommendation (goal bar in the chat thread; the console shows a read-only GoalPanel with Open in chat and no goal creation); the owner must accept it before the goal switches go on. | Confirm the assumed default (built dark). |
| D3-821 | EP-7-S1 | P2 | A Continue turn whose goal read fails is refused as thread_goal_unavailable, while an ordinary Do turn whose goal read fails just runs without the recitation (fail-open for context only). | Decide before the switch goes on (see the row). |
| D3-870 | EP-7-S2 | P1 | D2 was taken by recommendation (metered tokens only plus a mandatory turn cap; judge tokens count; no new turn at 100%) and built dark behind the S1 switches; the owner must confirm before switch-on. | Decide before the switch goes on (see the row). |
| D3-871 | EP-7-S2 | P1 | D10 was taken by recommendation (goal cap = min(remaining, org per-run ceiling), never above the org's) and built dark; the owner must confirm before switch-on. | Decide before the switch goes on (see the row). |
| D3-879 | EP-7-S4 | P1 | Assumed D4 = (a): the SPA drives attended continuation with a 5 s visible countdown and Stop; a hidden tab never sends; closing the tab stops the goal. The owner must confirm before switch-on. | Confirm the assumed default (built dark). |
| D3-880 | EP-7-S4 | P1 | Assumed D5 = (a): any pending approval, question, proposed plan or park stops the pump and ClaimGoalTurn refuses thread_goal_pending_decision (30 claims/goal/hour). This also refuses a manual Continue while a plan is proposed, so plan-mode goals stop every turn. The owner must confirm. | Confirm the assumed default (built dark). |

## EP-8 (9)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-267 | EP-8-S4 | P1 | session-core has no GDPR user-subject erasure (gdpr.rs parse_erasure_event returns None for subject_type 'user'). user_profiles, like agent_memory, is erased only by ResetMemory and the org purge, so the design's 'GDPR user erasure deletes the row' has no path to hook into. | Decide before the switch goes on (see the row). |
| D3-347 | EP-8-S7 | P1 | Assumed D3 = (b): scope stays the kind and scope_id is the instance; every Space reader filters scope='space' AND scope_id=<Space>. Needs owner and Space-session acknowledgement before SESSION_SPACE_MEMORY_ENABLED goes on. | Confirm the assumed default (built dark). |
| D3-348 | EP-8-S7 | P1 | Assumed D4 = (a): a shared room with the switch on recalls only its own Space rows. That also stops recall of the room thread's own thread-scope rows, which WS17-S1 recalls today. | Confirm the assumed default (built dark). |
| D3-349 | EP-8-S7 | P1 | Assumed D5 = (b): only rows with audience_hash equal to the thread's current hash are recalled and the rest are counted as withheld. There is no Control declassify path, so every membership change withholds all earlier rows. | Confirm the assumed default (built dark). |
| D3-350 | EP-8-S7 | P1 | Assumed D11 = refuse: recall_space_memory returns nothing (skip_reason zdr) on a thread with a zdr_waived run. Space ZDR, which does not exist yet, is not consulted. | Confirm the assumed default (built dark). |
| D3-351 | EP-8-S7 | P1 | The GDPR rule for multi-member Space rows is assumed: a row only the erased person sourced is deleted; a shared row loses that person's message links but keeps its content text, which may still contain their words. The DPO must confirm. | Confirm the assumed default (built dark). |
| D3-356 | EP-8-S8 | P1 | The role step cannot save on first run: memory.remember needs a threadId, the empty chat has none (threads start on the first send), so the Remember-my-role box never renders on the first-run card; the owner must pick a thread-less save or a save on the first turn before PERSONAL_SETUP_VISIBLE goes on. | Decide before the switch goes on (see the row). |
| D3-413 | EP-8-S10 | P1 | D12 assumed = (a) per (Space, agent) binding, quoted, no authority; the owner must confirm together with EP-6 OD-5 / DEC-14 before SESSION_AGENT_MEMORY_ENABLED goes on. | Confirm the assumed default (built dark). |
| D3-414 | EP-8-S10 | P1 | D3 assumed = (b) `scope` kind plus `scope_id` instance (migration 0103); it still needs the Space session's acknowledgement before either Space or agent memory switches on. | Confirm the assumed default (built dark). |

## EP-9 (12)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-318 | EP-9-T1 | P1 | The zdr enum has a fourth value, not_gated, beyond the three the design names; without it the 15 not_gated destinations would need a claim the code does not support. The owner must accept or reject it. | Decide before the switch goes on (see the row). |
| D3-326 | EP-9-T3 | P1 | Assumed C-B1 = no panel change: round B gives no recommended option for the organization_web_policy store question and the panel is store-agnostic, so the owner must still rule C-B1 before T2 ships the web item. | Confirm the assumed default (built dark). |
| D3-327 | EP-9-T3 | P1 | Assumed D-W1 = (c) (web switches as capability-core keys, host lists in org-core); the panel has no branch on it, and only the origin of T2's web item depends on the choice. | Confirm the assumed default (built dark). |
| D3-328 | EP-9-T3 | P1 | The Space mount is in SpaceRoomHeader.tsx (room details), not WorkspaceSettingsPage.tsx, because the workspace settings route carries no space id; the owner may instead want a Space picker in the space-policy section, which does not exist yet. | Decide before the switch goes on (see the row). |
| D3-372 | EP-9-E1 | P2 | Enforcement gains `none` for an unwrapped process and `partial` covers every non-off network, where the design reserved `partial` for E3's proxy path; the owner should accept both into the design and the ledger. | Decide before the switch goes on (see the row). |
| D3-374 | EP-9-E2 | P1 | D-E3 was assumed at the design's recommendation (closed enum, unknown = off, all not storable, no egress under ZDR); the owner must confirm before SPACE_SANDBOX_EGRESS_TIERS is turned on. | Confirm the assumed default (built dark). |
| D3-375 | EP-9-E2 | P1 | D-E1 (netns plus socket proxy) was assumed at the recommendation and is not built in E2; the owner must confirm before E3. | Confirm the assumed default (built dark). |
| D3-376 | EP-9-E2 | P1 | D-E2 (grant the lease at off instead of refusing) was assumed but not built: the sandbox-manager verifier still refuses an egress-capable v2 claim with no tier granted, so E3 must change that behind SANDBOX_MANAGER_EGRESS_TIERS with owner sign-off, because it amends ledger :4440-4450. | Confirm the assumed default (built dark). |
| D3-383 | EP-9-E5 | P1 | Assumed DEC-21 and D-E4 = integration-corev2 custody with sealed references and sentinels (design recommendation); no owner has ruled and the ledger has no DEC-21 entry, so W4 043(c) and F-t3-057 stay blocked until the Model Plane owner, the Quarry plan and the Control Plane ratify it (ADR open item O1). | Confirm the assumed default (built dark). |
| D3-384 | EP-9-E5 | P1 | Assumed the injection mechanism = loopback base-URL routes over the E3 socket bridge with TLS interception rejected, a choice the design does not list, so the Model Plane owner must confirm it before W4 (ADR O2). | Confirm the assumed default (built dark). |
| D3-385 | EP-9-E5 | P1 | Assumed injection is also refused for plan-mode and tainted runs, stricter than the slice's ZDR and waived minimum and matching E3's run-level refusals (ADR O2). | Confirm the assumed default (built dark). |
| D3-397 | EP-9-C5 | P2 | data_use is derived at the gRPC edge from attests_zdr rather than stored per provider, so the first sourced trains or retention row needs an internal provider::ModelInfo field and edits to every provider's list_models; settle that before the row lands. | Decide before the switch goes on (see the row). |

## EP-10 (3)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-285 | EP-10-S3 | P2 | EditError has a 7th code `invalid_body` for a blank body; the S4 gateway mapping (422) and the S6 SPA copy must cover it, and the design's D2 row should list it. | Decide before the switch goes on (see the row). |
| D3-579 | EP-10-S7 | P2 | The 422 already_always_ask uses capability-core's existing {"error": msg} body, not the typed {error:{code,message,details}} envelope; the gateway/BFF mapping of that code to the SPA was not checked. | Decide before the switch goes on (see the row). |
| D3-710 | EP-10-S4 | P2 | An edit replayed after its own successful grant (for example through the BFF mg_post_decide_with_retry on a lost response) gets 409 approval_changed rather than an idempotent 200. The SPA (S6) must treat 409 as 'reload the approval' and read approval.revision. | Decide before the switch goes on (see the row). |

## EP-12 (5)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-469 | EP-12-S2 | P2 | A refusal whose durable audit cannot be written still answers the refusal (it issues nothing), and refusals of an unauthenticated calling service are not audited at all, to keep the audit stream from being floodable; only grants are fail-closed. | Decide before the switch goes on (see the row). |
| D3-470 | EP-12-S2 | P2 | The recommended registry change adds apiKeyExchange to the existing verevon-gateway (mail-task) credential; a dedicated principal would keep the two authorities on separate credentials, as the retention-decision row does, but every registry row must also carry a non-empty audiences/scopes internal-token grant. | Decide before the switch goes on (see the row). |
| D3-523 | EP-12-S6 | P3 | budget_user_id for a key is the subject apikey:<key_id>, so the prompt cache, request capture and budget scope are per key, never shared with the owner or another key. | Decide before the switch goes on (see the row). |
| D3-524 | EP-12-S8 | P1 | The public contract names only x-api-key, but auth-core's exchange requires orgId, so the ingress requires a caller header x-agent-org-id (checked by auth-core against the key's org binding). Before switch-on, either the spec and golden must list this header or S2 must derive the org from the key. | Decide before the switch goes on (see the row). |
| D3-536 | EP-12-S13 | P2 | The ADR adds that revoking a snapshot also removes its Control grants and that membership is re-checked at read, beyond the design's 'revoked_at + delete body'; S14 should implement or the owner should strike it at OD-5. | Decide before the switch goes on (see the row). |

## EP-14 (5)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-422 | EP-14-S2 | P1 | D5 was built at its recommendation without owner sign-off (four levels, additive user-core transcriptDensity, cache key verevon.chat.density.v1, compactMode fallback); the owner must accept it, and user-core must acknowledge the field, before the switch is turned on. | Decide before the switch goes on (see the row). |
| D3-490 | EP-14-S5 | P1 | D2-199 assumed: turn-timing-v1 stores the WS15-S4 optional failed_attempts_ms and retry_wait_ms as part of its shape; the owner must confirm before MODEL_GATEWAY_RUN_TIMING_PERSIST is switched on, because a gate added later would make it v2. | Confirm the assumed default (built dark). |
| D3-491 | EP-14-S5 | P1 | D11 assumed (recommendation): timing is a RUN_TIMING_RECORDED event (EventType 161, not the reserved 150, which the trace already uses) plus a read over session_tool_audit_intents; the integrator must confirm 161. | Confirm the assumed default (built dark). |
| D3-492 | EP-14-S5 | P1 | D13 assumed (recommendation): a waived run is treated as ZDR at both layers. The gateway never arms or writes for a waived turn, session-core refuses the write, and GetRunTiming returns an empty response for a non-persistent run. | Confirm the assumed default (built dark). |
| D3-541 | EP-14-S7 | P1 | Assumed D3 = (b) (platform sections label + digest + length, org/user/memory text, owner only, server-side filter); PROD + OWN must ratify before SESSION_CORE_REQUEST_INSPECTOR_READ or MODEL_GATEWAY_REQUEST_INSPECTOR is switched on. | Confirm the assumed default (built dark). |

## Merges and lead (3)

| Row | Slice | P | What is open | Recommendation |
|---|---|---|---|---|
| D3-261 | W3-07 lead | P2 | EP-14-S6 added two zdr-posture-matrix rows (e_request_snapshot_refused_under_zdr, e_request_snapshot_refused_for_waived_run) whose inputs equal existing rows, so zdr-contract-schema refused them ('rows must differ') and no row-count constant moved; the lead dropped both and restored the matrix, its copies and its pin. The request-snapshot refusal stays recorded in apps/Model Plane/docs/ZDR.md; give it distinct, resolver-verified inputs if a matrix row is wanted. | Decide before the switch goes on (see the row). |
| D3-263 | W3-07 lead | P1 | EP-14-S6 asked for an inference-core grant session:request-snapshots:write (and a retention change) in apps/Control Plane/config/plane-service-principals.json; the guard did not apply it (owner decision). Request snapshots stay unwritable from inference-core until it lands. | Decide before the switch goes on (see the row). |
| D0-037 | merge mp-w2w3 | P2 | Merge choices to confirm: a Quarry policy denial on the Do-loop `browser.open` maps to GRANT_REFUSED (not SITE_DENIED, which points at browser site settings); `browser.act` leaves `initiated_by` unset as before (mp-w2w3's own path sets "agent"); the authz_decision audit now also fires for Space-member decisions. | Decide before the switch goes on (see the row). |
