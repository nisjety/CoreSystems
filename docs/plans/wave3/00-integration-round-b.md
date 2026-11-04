# Wave 3 integration, round B (EP-1, EP-3, EP-5, EP-7, EP-9, EP-11)

Written 2026-11-02 at HEAD `70e67c8d7` (branch `model-plane`). Round A (`00-integration-round-a.md`) is treated as taken: session-core 0072-0076, cost-core 0012, InferRequest 21-23, RunContext 7-8 and the rest of its table. Numbers below were checked against this worktree and against the other live branches (`main`, `space-w2`, `mp-w2w3`, `w23/integ`), because those branches already hold numbers the worktree does not.

Edits made to design files: the `<next>` migration placeholders in EP-3, EP-5, EP-7, EP-9, EP-11 (all but `ADR-<next>` in EP-9 E5, left open because E5 is decision-gated); EP-7's two proto field numbers (`RunContext.goal_recitation` 7 to 9, `ThreadSummary.goal_status` 23 to 29, see C-B3); and round A's two pending renumberings (EP-14 `InferRequest.run_id = 22` with `call_purpose` dropped; EP-6 `RunContext.agent_roles = 8`).

## 1. Reservation table

### 1.1 Migrations

Finding that changes the numbering rule: round A reserved session-core 0072-0076, but other branches already hold those numbers: `main` has 0071 (`message_post_kind`), 0072, 0073, 0085; `space-w2` has 0072-0085; `mp-w2w3` has 0086, 0087. sqlx refuses two files with one version. Round B therefore allocates from **0090** upward (0088-0089 left as slack for `mp-w2w3` stragglers) and recommends a merge-time renumber map for round A's five (C-B2).

| Number | Service | Design / slice | Name |
|---|---|---|---|
| 0090 | session-core | EP-3 S2 | `0090_artifact_revisions` (+ `revert/`) |
| 0091 | session-core | EP-7 S1 | `0091_thread_goals` (+ revert) |
| 0092 | session-core | EP-7 S2 | `0092_thread_goal_turns` (+ revert) |
| 0093 | session-core | EP-7 S3 | `0093_thread_goal_verdicts` (optional; only if the verdict columns do not fold into 0092) |
| 0094 | session-core | EP-7 S6 (gated) | `0094_cron_schedule_goal_link` (+ revert) |
| 0107 | session-core | EP-9 C2 | `0107_retention_sweep_runs` (+ revert; renumbered from 0095 at the W3-19 close, after 0100-0105 landed) |
| 0108 | session-core | EP-11 S2 | `0108_agent_skill_versions` (+ revert; renumbered from 0096 at the W3-20 close, after 0100-0107 landed) |
| 0097 | session-core | EP-11 S5 | `0097_agent_skill_drafts` (+ revert) |
| 0098 | session-core | EP-11 S6 | `0098_authored_behaviour_kind` (+ revert) |
| 0099 | session-core | EP-11 S9 | `0099_agent_skills_space_scope` (+ revert) |
| 0033 | capability-core | EP-5 S2 | `0033_deck_tool_class` (head here 0030; `mp-w2w3` holds 0031 `web_crawl_tool_capabilities` and 0032 `document_pii_default_policy`) |
| 0034 | capability-core | EP-11 S5 | `0034_skill_bundle_retention` |
| 0035 | capability-core | EP-5 S6 | reserved, only if the deck-template flag is stored; the design derives it at read time, so normally unused |
| 018 | notification-core | EP-7 S5 | `018_register_goal_stopped` (head 017) |
| 030 | org-core | EP-9 C1 | `030_org_retention_policies` (025-029 exist on `w23/integ` and `space-w2`) |
| 044 | user-core | EP-9 E2 | `044_space_sandbox_egress` (033-043 exist on other branches) |
| 0008 | sandbox-manager | EP-9 E3 | `0008_lease_egress_tier` |
| 0003 | orchestrator-core | EP-11 S3b (optional) | `0003_feedback_skill_version` (head 0002; unchanged) |
| none | EP-1 | all | cause lives in `approvals.metadata` JSONB (Quarry may add its own migration for the beneficiary field) |
| none | EP-5 S1-S5, S7-S10 | | manifest rides message metadata |

### 1.2 Proto fields

| File / message | Field | Owner | Status |
|---|---|---|---|
| execution.proto `RunContext` | `user_profile=7` (EP-8), `agent_roles=8` (EP-6), **`goal_recitation=9`** (EP-7 S1) | | EP-7 had claimed 7; moved |
| sessions.proto `ThreadSummary` | **`goal_status=29`** | EP-7 S5 | tree max 22, but `main` uses 23-24 and `space-w2` 23-28; moved from 23 |
| attention.proto `AttentionReason` | `ATTENTION_REASON_GOAL=5` | EP-7 S5 | 4 stays reserved for WS03 parked; tree max 3 |
| orchestration.proto `ApprovalCause` | `host=11`, `assist_kind=13`, `assist_origin=14`; **12 reserved** for WS21-S6 `max_level` | EP-1 S1/S2 | tree max 10; shared with WS21-S6, first lander adds `host` |
| orchestration.proto `BrowserRunPaused` / `BrowserRunResumed` | `reason=3` (string; **already landed on `mp-w2w3`** by Quarry WS8.G, so EP-1 shares it, does not add it), `control_epoch=4` | EP-1 S1 | tree max 2; `control_epoch` is free everywhere |
| sandboxes.proto `AcquireLeaseResponse` | `egress_tier=7` + enum `SandboxEgressTier` | EP-9 E3 | tree max 6 |
| execution.proto `OrgPolicyContext` | `web=4` (+ message `WebPolicyContext`) | EP-9 W2 | tree max 3 |
| inference.proto `ModelInfo` | `data_use=12` (+ `ProviderDataUse`) | EP-9 C5 | EP-13 holds 9-11 |
| sessions.proto `AgentSkill` | `version=19`, `digest=20`, `kind=21`, `settings_json=22` | EP-11 S2/S6 | tree max 18, branches also 18 |
| sessions.proto `ListAgentSkillsRequest` | `kinds=4`, `space_id=5` | EP-11 S6/S9 | tree max 3 |
| gateway.proto `Skill` | `version=7`, `digest=8` | EP-11 S3 | tree max 6 |
| runs.proto | new `SkillPin`; `RunContextStatus.skills=4` | EP-11 S3 | tree max 3 |
| capabilities.proto `SkillBundleSummary` | `deck_template=7` | EP-5 S6 | tree max 6 |
| capabilities.proto `GetSkillBundleManifestRequest` | `manifest_sha256=2` | EP-11 S5 | tree max 1 |
| NEW `artifact_revisions.proto` | `ArtifactRevisionService` + messages | EP-3 S2 | add to mp-contracts `build.rs` list |
| sessions.proto | NEW services `ThreadReferenceService` (EP-3 S7), `ThreadGoalService` (EP-7), `SkillVersionService` (EP-11) | | no name clash on any branch; see C-B6 for the insertion-point conflict |
| inference.proto `InferPurpose` | `GOAL_JUDGE` at next free value | EP-7 S3 follow-up | after EP-13 S3 |
| no change | `RunAgentRequest` (tree 24; 25 EP-2, 26 EP-6 if needed), `RunAgentResponse`, `OrchestrationEvent` (next free 30), `EventType`, `ApprovalKind` | | EP-1, EP-3, EP-7 verified |

Carry-over for round A (not edited here): EP-10's `Approval.revision=17` collides with `space-w2`, which already uses `Approval` 17-24 (`action_label`, `addressee`, `continuation=24`...). Recommend EP-10 take 25 (and re-verify `ApprovalStateChanged`, `ConsumeApprovalResponse`, `DecideApprovalRequest` against `space-w2`).

### 1.3 Flags

About 70 distinct names in the six designs. No two designs define the same name, none clashes with round A's 47, and none exists anywhere in `apps/Model Plane` today (searched). The only shared names are deliberate reuses: `BUILTIN_OFFICE_SKILLS_ENABLED` and `SKILL_BUNDLES_ENABLED` (EP-5 and EP-11), `WEB_CLASS_TOOLS` (a const read by EP-2 and EP-9). Vocabulary is consistent (literal `true`, or off/shadow/enforce). Near-misses to keep distinct: `EXECUTION_SANDBOX_EGRESS` (EP-9, mode) vs `SANDBOX_MANAGER_EGRESS_TIERS` (EP-9, claim); `SKILL_DECK_TEMPLATES_ENABLED` (EP-5) vs `AUTHORED_KINDS_ENABLED` (EP-11) (C-B7).

## 2. Hot-file matrix

Counts are mentions across the six round-B designs (EP-1/3/5/7/9/11). Rule for every row: one slice at a time, rebase onto the previous landing, additive blocks at the end, own flag, CRLF preserved where the file is CRLF.

| File | Mentions | Order / rule |
|---|---|---|
| `model-gateway http_routes.rs` | EP-3 7, EP-5 4, EP-7 7, EP-9 3, EP-11 2 | Additive route blocks. EP-3 S5 (`InvokeRequest.context_refs`), EP-7 S1 and EP-5 S7 also add fields to `InvokeRequest`/routes: land in slice order, never reformat. |
| `session-core grpc.rs` | EP-11 10, EP-7 6, EP-9 4, EP-3 2 | New RPC handlers in their own modules (`thread_goals.rs`, `artifact_revisions.rs`, `skill_versions.rs`); grpc.rs gets registration lines only. |
| `model-gateway sse.rs` | EP-7 11, EP-11 6, EP-3 6, EP-9 4, EP-5 3 | After EP-13 S3 (purpose tag). EP-7 S3/S4 and EP-11 S3 both touch the run-start path: EP-11 S3 first (smaller). |
| `tool_loop.rs` | EP-5 12, EP-9 7, EP-3 4, EP-11 4 | Independent match arms (`deck`, `read_artifact` edits, web scope, skill pins). EP-5 S2 and EP-3 S4 both extend the artifact tool family: EP-3 first. |
| `execution-core runtime_loop/agent.rs` | EP-1 7+, EP-9 4-6, EP-7 1 | EP-1 S1 (pause/approval cause) and EP-9 W2/W3 both near the browser and web tool gates; EP-1 first. |
| `execution-core browser_session.rs` | EP-1 13 | EP-1 only. |
| `execution-core code_interpreter.rs` (`CodeResult`) | EP-9 E1 5, EP-5 S1/S9 1 | EP-9 E1 adds a report field to the same struct EP-5 reads previews from: additive, EP-5 does not depend on E1. |
| `gdpr.rs` | EP-7 4, EP-9 2, EP-11 2, EP-3 1, EP-5 1, EP-1 1 | See C-B9: one registered list of thread-owned tables. |
| `zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs` | EP-1 2, EP-3 3, EP-5 2, EP-9 2 | Row-count tests change every time; rebase each. |
| `sessions.proto` | EP-3 6, EP-7 6, EP-11 4 | Three new services and several fields; C-B6. |
| `execution.proto` | EP-9 5, EP-7 2, EP-5 1 | Numbers fixed in 1.2. |
| `capabilities.proto` | EP-5 3, EP-11 3 | Different messages; disjoint. |
| `orchestration.proto` | EP-1 2, EP-7 1 | `ApprovalCause`, `BrowserRun*` only; branch-merge conflicts with Quarry WS8.G. |
| `FES shared/actions/action-registry.ts` (+ test) | EP-3 6, EP-5 4, EP-1 3, EP-9 3, EP-11 3, EP-7 2 | Append-only; count assertions collide. |
| `FES chat-client.ts` | EP-3 6, EP-5 3, EP-7 3, EP-9 1 | Rebase each; EP-3 S5 changes the send payload that EP-7 S4 (auto-continue) also sends. |
| `FES use-chat-controller.ts` | EP-3 5, EP-7 4, EP-1 1 | Same. |
| `FES DashboardComposer.tsx` | EP-3 5, EP-11 5 | Slash and picker triggers: C-B10. |
| `FES composer-trays.ts` / `ComposerTrays.tsx` | EP-7 3 (EP-1's card is an approval tray) | Goal tray after plan, before queue; EP-1 reuses the approval kind, so no new kind there. |
| `FES ChatArtifactPanel.tsx` | EP-3 9, EP-5 4 | EP-3 S3/S4 first, then EP-5 S4 slide strip. |
| `FES approval-copy.ts` | EP-1 4 | Closed cause list: EP-10 and EP-1 both add codes; additive. |
| `MP docs/decisions/ledger.md`, depth register, compose file | all six | One ledger entry per slice, append at top; compose additive. |

## 3. Contradictions and recommended resolutions

**C-B1 (high). EP-9 W2/W3 vs an org web-policy store that already exists on `w23/integ`.** Branch `w23/integ` (HEAD `76775d264`) has org-core migrations 025-027 (`organization_web_policy`, history, audit outbox): the org's web policy of record (per-capability modes, robots and proxy ceilings, domain rules) that Auth Core folds into a `web_policy` token claim at mint (WS7.I) and Quarry enforces. EP-9 D-W1 puts web switches in the capability-core agent-policy registry and host lists in org-core browser sites, and D-W2 rejects minting the claim as contradicting DEC-19. That would make a second org web-policy store with a competing enforcement path. Resolution: before EP-9 W2 starts, the owner decides whether `organization_web_policy` (w23) is the single store. If yes, EP-9 W2/W3 read it via Control (modes) and forward the claim, and the capability-core `web.*` keys are dropped; if no, DEC-19 must say why two stores exist. EP-9 W1, T1, E1, C-slices are unaffected. Treat as blocking W2, W3, T2, T3.

**C-B2 (high). Migration numbers already used elsewhere.** Round A's session-core 0072-0076 (and the 0071 head) collide with `main`/`space-w2`/`mp-w2w3` (see 1.1); `capability-core` 0031-0032 are on `mp-w2w3`; org-core 025-029 and user-core 033-043 on other branches. Resolution: round B numbers are collision-free against every local branch's migration folders as of today; before the first merge, renumber round A's five by `git mv` to 0100-0104 (or the next free) and update their designs; keep the "number belongs to its slice" rule after that. Only the branches named above were scanned; other local branches were checked for migration numbers only.

**C-B3. Proto numbers claimed twice or already used on a branch.** `RunContext` 7: EP-8, EP-6, EP-7 (resolved: 7, 8, 9). `ThreadSummary` 23: EP-7 vs `main` 23-24 / `space-w2` 23-28 (resolved: 29). `BrowserRunPaused.reason=3`: EP-1 vs Quarry WS8.G on `mp-w2w3` (same field, same meaning: EP-1 adopts it, adds only `control_epoch=4`). `ApprovalCause` 12: held for WS21-S6.

**C-B4. EP-1 vs EP-10 on the approval row.** EP-1 adds cause `browser_human_assist` (kind TOOL_CALL) to an approval EP-10 S4 makes editable (`edit=9`) and describes (`GetApprovalPreview`, descriptor revision). A takeover is not a tool call with arguments. Resolution: EP-10 S4 refuses `edit` for causes without a descriptor (`browser_human_assist` included) and EP-10 S1's preview returns the no-descriptor form for it; add one test in each design.

**C-B5. EP-1 takeover vs EP-7 goal pump.** EP-7 D5 says any pending approval stops the pump; a takeover waits up to 1800 s (EP-1 D5) on an approval row. Consistent, but state it: a goal turn waiting on `browser_human_assist` counts as a running turn for `max_turns` and wall clock, is not judged, and a takeover expiry reaches the goal as a normal failed turn (not "met"). Add to EP-7 S4 test list.

**C-B6. Same insertion point in `sessions.proto`.** EP-3 and EP-7 both say "append after `ThreadForkService` (`:1998`)"; EP-11 appends after `LearningGovernanceService` (`:303`); `space-w2` also grows the file. Resolution: put `ThreadGoalService` and `SkillVersionService` into new files (`thread_goals.proto`, `skill_versions.proto`) like EP-3's `artifact_revisions.proto`, and keep `ThreadReferenceService` (which needs sessions types) the only appender; add the new files to the mp-contracts `build.rs` list.

**C-B7. "Template" means two things, and both touch the skill-bundle store.** EP-5 brand templates are skill bundles (`templates/deck-theme.json`, `SKILL_DECK_TEMPLATES_ENABLED`); EP-11 `kind='template'` is plain text inserted in the composer. EP-11 S5 replaces the unique index on `skill_bundles` (state live/draft/superseded); EP-5 S6/S7 read bundles assuming one per `(org_id, skill_id)`. Resolution: name EP-5's objects "deck themes" in code and UI, keep EP-11's CHECK value `template`; EP-5 S6/S7 read only `state='live'` (add the filter now, a no-op until 0034 lands). EP-5 OD-4 and EP-11 D9 are the same decision (register D2-384): one owner ruling.

**C-B8. Deck manifest vs EP-3's artifact store.** EP-5 stores decks as artifact `kind='deck'` in message metadata and says EP-3 "may later host it". EP-3 S2 dual-writes every artifact into `artifact_revisions`. Resolution: EP-3 S2 treats `kind` as an open string, EP-5 S3 writes through the same persistence function (so the dual-write picks decks up), and EP-3 S4 refuses user editing of `deck` in W3 (edits go through the `deck` tool). Add to both test lists.

**C-B9. Erasure and retention of the new tables.** EP-3 `artifact_revisions`, EP-7 `thread_goals`/`thread_goal_turns`, EP-11 version rows, and EP-9's sweeper/legal hold must agree on one list. Resolution: every new thread-bound table has `ON DELETE CASCADE` from `threads` or an explicit `gdpr.rs` statement, and EP-9 C2's dry-run counts them (add a test that fails when a table with a `thread_id` column is missing from the list). EP-11 version rows follow skill delete / user erasure, not thread delete. EP-9 C4 (legal hold) must also name goals and revisions in its blocked scope.

**C-B10. Slash commands and triggers.** EP-3 S5 adds a `+ Kontekst` picker and `/kontekst`; EP-11 S6 adds slash templates and S7 `/learn`; EP-3 OD-5 defers templates to EP-11. Resolution: one slash-command registry in the composer, created by whichever lands first (expected EP-3 S5 FE half), EP-11 S6/S7 register into it; `@` stays reserved for Space.

**C-B11. Two registry-key additions.** EP-9 W2 (`web.*`, if kept, see C-B1) and EP-11 S9 (`ext.skills`) both add keys to the capability-core org agent policy schema (gated by schema status). Resolution: disjoint keys, but one schema-status bump; whoever lands second rebases. EP-9 T2's effective-policy view should list EP-11's key.

**C-B12. EP-5 and the sandbox.** EP-5 says the LibreOffice layer is in the image already (`Dockerfile:88-110`) while listing WS19-D5 (image vs sidecar) as open; the decision is effectively made for S9 only if the image layer is kept. EP-9 E3 puts the sandbox behind `--unshare-net`: deck build needs no network, and python-pptx is pinned in the image, so no conflict, but add "deck build passes under egress tier off" to EP-5's live proof.

**Stores for the same thing:** EP-3 artifact revisions vs the Data-owned Space canvas is already reconciled in EP-3 OD-1 (two stores, one-way promote). No other duplicates found beyond C-B1.

## 4. Spot-check results

Eight random `path:line` citations per design were opened (48 total) and the claim sentence compared to the line.

| Design | Result |
|---|---|
| EP-1 | 8/8 match (`GRANT_TTL_SECONDS`=30 min, `BrowserChrome` control-mode prop, `ApprovalCause` 1-10, `blocked_by_bot_wall`, `capability_policy` browser mapping). |
| EP-3 | 8/8 match (`ArtifactViewer`, `rehydrate_persisted_artifacts`, `InvokeRequest` without `context_refs`, `mergeArtifactVersion`, ZDR comment at `chat-client.ts:2785`). |
| EP-5 | 8/8 match (`conversation_tool_allowed`, `code_interpreter` tool def, `skill_bundles` migration, `check.py` pptx part, Dockerfile layers). |
| EP-7 | 8/8 match (`PLAN_EXECUTE_ON_APPROVE_ENV`, `aux_model`, cost-core `/cost/run`, `execute_scheduled_agent_step`, `composer-trays.ts` kinds, `run_ceiling.rs`). |
| EP-9 | 8/8 match; `OC/internal/http/server.go:134` and `:141` are the two explain routes (model entitlement, browser sites) as claimed; `fetch_url` in `builtin_tool_defs`, `CodeResult` struct, `capability_profile`. One note: the design never mentions the `w23/integ` org web-policy store (C-B1). |
| EP-11 | 8/8 match (`preset-agents.ts`, `ContentHash`, `RunContextStatus` 1-3, `authz.go:81` ZDR write refusal, `LearningGovernanceService` at `sessions.proto:303`, `skillbundle/store.go` Put). |

Proto "next free" claims re-checked against `HEAD`: `ApprovalCause` (10), `BrowserRunPaused/Resumed` (2), `AcquireLeaseResponse` (6), `OrgPolicyContext` (3), `AttentionReason` (3), `AgentSkill` (18), `ListAgentSkillsRequest` (3), `Skill` (6), `RunContextStatus` (3), `SkillBundleSummary` (6), `GetSkillBundleManifestRequest` (1), `ModelInfo` (8), `RunAgentRequest` (24). Correct against HEAD; `ThreadSummary` (22) is correct on this branch but stale against `main`/`space-w2` (C-B3). EP-1 says `ApprovalCause` verified 1-10: correct. Line-number mismatches: none.

## 5. Recommended build order

Rule applied: "startable now" means the design's own `blocks` field does not list the slice (default values apply), and the collision items above do not gate it. Flag flips and live proofs excluded.

Merge-prep first (no design depends on it, but every migration does): execute the C-B2 renumber plan and answer C-B1.

Startable now, no owner decision:
1. **EP-1 S1 (server half: pause/resume, approval cause, `control_epoch`), S3, S4.** The FE half and live proof wait on D1 (Quarry beneficiary). Needs `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` on for proof.
2. **EP-3 S1** (client drafts, no flag, no server). S5 server half (context refs on `InvokeRequest`, off) and **S7** (thread references; defaults refuse) can follow; S5 FE waits on OD-4.
3. **EP-5 S1, S2, S4** (S2 needs capability-core 0033). S3 after EP-3 S2 lands or OD-1 is accepted; S5 waits on OD-5 wording.
4. **EP-7 S1** (goal record, goal bar in the chat thread, manual Continue; D1 and D7 only affect placement and the refusal key, defaults exist).
5. **EP-9 W1, T1, E1, C1, C2, C5, E4** (all default-off; C1/C2 use D-C2 defaults).
6. **EP-11 S1** (owner/admin path), **S4 in report mode**.

Startable once a recommendation is accepted (listed decision):
- EP-3 S2, S3, S4, S8 (OD-1, OD-2); S6 (OD-3).
- EP-5 S3 and S8 (OD-1); S6, S7 (OD-2, OD-3, OD-4).
- EP-7 S2 (D2, D10), S3 (D3), S4 (D4, D5), S5; S6 needs DEC-16; S7 design-only.
- EP-9 W2, W3, T2, T3 (C-B1 first, then D-W1, D-W2); E2, E3 (D-E1, D-E2); C3 after C2; C4 (D-C1); E5 (D-E4).
- EP-11 S2, S3, S5 (D2, D10, D3); S6 (D1, D6); S7 (D5); S8 (D4, D9); S9 (D7).

Across-design sequencing:
1. EP-13 S3 (purpose tag) before EP-7 S3 and EP-14 S6; EP-14 S1-S3 (chat fold/strip) before EP-3 S3/S4 and EP-5 S4, which restructure the same panels.
2. EP-3 S2 (revision store) before EP-5 S3 and EP-3 S4 before EP-5 S4 edits; EP-5 S6/S7 add the `state='live'` filter before EP-11 S5.
3. EP-9 C2 before EP-7 S1's table list is final (C-B9), and C1/C2 before EP-3/EP-7/EP-11 tables are declared erasable.
4. EP-1 S1 before EP-10 S4 ships (C-B4); EP-7 S4 after EP-1 S1 (C-B5).
5. EP-11 S6 and S7 after EP-3 S5 FE (shared slash registry).

Owner decisions that unblock the most slices: EP-3 OD-1 and OD-2 (S2, S3, S4, S8, plus EP-5 S3/S8); EP-9 C-B1 plus D-W1 (W2, W3, T2, T3); EP-11 D2 and D10 (S2, S3, S5); EP-1 D1 (Quarry beneficiary: S1 FE, S2, S6); EP-7 D2 to D5 (S2 to S5); the bundle ZDR ruling shared by EP-5 OD-4 and EP-11 D9.
