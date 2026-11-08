# WS07 code map: memory controls and skill-learning governance

Scope: F-t3-020 (user and org memory controls, W1, Priority, L) first, then F-t3-007 (skill-learning governance, W1, Priority, L, depends on F-t3-020).
Audit doc: `C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md`. Cards: F-t3-007 lines 436-451; F-t3-020 lines 628-645. Section 9.1 rows: 1539 (007), 1542 (020). Appendix B: 1825, 1837. Appendix C: 1972-1979 (007), 2079-2086 (020). Appendix D: 2126 (020, effort M to L); no F-t3-007 line (007 appears only as a dependency of F-t3-081 at 2153). Section 10 open question: 1729.

Trees and conventions:
- W = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). Paths without a prefix are relative to W.
- L = `C:/dev/CoresSystem` (live tree). Status from `.plan-work/live-tree-status.txt` (snapshot 20:56); `M` = tracked and modified, `??` = untracked (live-only), `clean` = not listed.
- Verdicts: MATCH (cited symbol or behaviour at cited lines), DRIFT (present at other lines; new range given), ABSENT (in neither tree), LIVE-ONLY (`??`, not in HEAD).
- The audit read the LIVE tree. Many card line numbers match L and not W. Both are given.

## 1. Summary

- F-t3-020 today: user memory state is only `extract_after` plus forgotten ids (`0039`). There is no on, paused or off state. `user-core` `memoryEnabled` (default false) occurs only in `apps/Control Plane/user-core/internal/http/handlers.go` (lines 1410, 1421) among Go, Rust, TS, SQL, proto and md files under `apps/`, so nothing enforces it. Org-core has no memory or learning term at all. Reset and forget primitives already exist (`memory_control.rs`, `gdpr.rs` erasure).
- F-t3-007 today: the sink writes every learned skill with `enabled: true` (`skillsink/sink.go:55`). Scope defaults to `org` in the DB (`0031`). Only provenance is guarded (`ON CONFLICT ... WHERE origin` in `session-core/src/grpc.rs`). The conflict arm overwrites `enabled` (grpc.rs:4940 in W; 5111 in L). `SessionRef` has no user id (`trigger.go:40-44`). The gateway skill cache is keyed by org with a 60 s TTL (`skills.rs`). No skill_revisions table, Learning inbox, seeded eval or ledger exists in either tree (ABSENT).
- Waived-run withholding (card cites `learning_events.rs` and `sessionreview/retention.go`) is LIVE-ONLY. W has ZDR withholding only. `retention.go` has no waiver logic in either tree.
- Deploy default `LEARNING_REVIEW_ENABLED=false` (`deploy/docker-compose.yml:1429`). Code default is on (`cmd/main.go:750-752`). The card's "loop runs by default" holds for code only.
- All F-t3-007 and F-t3-020 citations resolve. Corrected ranges are in section 2.

## 2. Citation verdicts

### 2a. F-t3-020

| card id | cited path:lines | claim | W verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| 020 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/memory.rs:48-60 | list, correct, delete routes | MATCH | clean | memory.rs:48-61 (router); list 68; correct_memory 116-151 (POST /v1/memories at 143-151; delete of original not read); delete 242 |
| 020 | MemorySection.tsx:1-80 (full: apps/Frontend Plane/verevonv3/src/features/settings/components/MemorySection.tsx) | list and per-item delete UI | MATCH, partial: list and delete only, no correct, pause or reset control in 1-85 | clean | MemorySection.tsx:69-85 (component; two-step delete 79-85); mounted WorkspaceSettingsPage.tsx:496 |
| 020 | apps/Model Plane/rust/services/session-core/migrations/0039_memory_user_control.sql:1-15 | controls hold only extract_after and forgotten ids | MATCH | clean | 0039:3-16 (user_memory_controls: org_id, user_id, extract_after; forgotten_memory_ids) |
| 020 | dreaming.rs:135-160 (full: apps/Model Plane/rust/services/session-core/src/dreaming.rs) | Dreaming configured by env only | MATCH | M, lines 135-160 unchanged in L (first hunk at :215) | dreaming.rs:135-143 (PendingMessage), 145-156 (run(); DREAMING_INTERVAL_SECS 146, DREAMING_BATCH_SIZE 150, DreamExtractor::from_env 155) |
| 020 | apps/Model Plane/rust/services/model-gateway/src/sse.rs:1648-1680 | recall gate: conversation_only, web-only scope, trivial prompts | DRIFT. W gate at 1581-1597 (condition at 1586; fetch at 1589). W 1648-1680 is the retrieval call and ZDR checkpoint | M. Gate at L:1655, which lies inside the card window | W: sse.rs:1581-1597. L: sse.rs:1655 |
| 020 | apps/Control Plane/user-core/internal/http/handlers.go:1395-1430 | memoryEnabled default false, no reader | MATCH | clean | handlers.go:1405-1430 (AISettings 1405-1411, memoryEnabled 1410; defaults 1419-1422, memoryEnabled false at 1421; updateAISettings 1433) |
| 020 | apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:625-642 (full path) | Ask turns cannot save | DRIFT. Exclusion is in `inline_tool_allowed` at 615-620 (condition 616). W 622-647 is the SNIPPET_USER_SUPPLIED_ARGS doc block, not the exclusion | M. Function at L:639-641, inside card window | W: tool_loop.rs:615-620. L: tool_loop.rs:639-641 |
| 020 | apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs:150-185 (full path) | ZDR turns neither read nor write | ABSENT in W (file not committed) | LIVE-ONLY (`??`). FEATURE_TREATMENTS 145-181; memory row 165-168 matches the claim | L: zdr_features.rs:145-181 (memory row 165-168). W structural skip: sse.rs:1062-1069 returns via zdr_direct_stream before the gate at 1586 |
| 020 | memory_erasure.rs (no lines) | reuse for Reset | MATCH (file exists) | clean | apps/Model Plane/rust/services/session-core/src/memory_erasure.rs (erase_semantic_copies called from gdpr.rs:445) |
| 020 | gdpr.rs:440-456 (full: apps/Model Plane/rust/services/session-core/src/gdpr.rs) | GDPR erasure reusable for Reset | MATCH. This is session-core gdpr.rs, not the Control Plane gdpr.go | clean | gdpr.rs:440-460 (post-commit semantic erasure); purge of forgotten_memory_ids 412, user_memory_controls 414 |
| 020 | chat/shared.rs:95-117 (full: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/shared.rs) | BFF ZDR stamp precedent | MATCH | clean | same (apply_org_zdr_posture 95-117) |
| 020 | org_quota.rs service-principal pull (no lines) | org policy pulled with service credential | MATCH | clean | apps/Model Plane/rust/services/model-gateway/src/org_quota.rs:99-140 (fetch_org_limits 112; x-service-id and x-service-token 135-136) |
| 020 | CLAUDE.md:23 | Control Plane owns users, entitlements | MATCH | clean | CLAUDE.md:23 (root). Same file: :27 Model Plane, :29 Frontend Plane, :32 no direct DB crossing, :35 ZDR propagation |

### 2b. F-t3-007

| card id | cited path:lines | claim | W verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| 007 | apps/Model Plane/go/services/capability-core/cmd/main.go:743-800 | loop on unless LEARNING_REVIEW_ENABLED set | MATCH. Unset or blank means enabled (750-752). Gate 767-775; consumer start 786-803 | clean | main.go:742-803 (const 747). Compose default off: apps/Model Plane/deploy/docker-compose.yml:1429 (clean) |
| 007 | apps/Model Plane/proto/model_plane/v1/sessions.proto:842-900 | UpsertAgentSkill carries only origin | DRIFT. W UpsertAgentSkillRequest 831-843 (origin 842). W ListAgentSkills and AgentSkill 855-886 | M (+42). Request at L:842-853 (origin 853); AgentSkill origin at L:886. Card window matches L | W: sessions.proto:831-843. L: sessions.proto:842-853 |
| 007 | apps/Model Plane/rust/services/session-core/src/grpc.rs:5075-5148 (full path) | UpsertAgentSkill write; conflict overwrites enabled | DRIFT. W upsert_agent_skill 4904-4979 (insert 4929-4933 has no scope; ON CONFLICT 4934; enabled=EXCLUDED.enabled 4940; provenance WHERE 4942) | M (+817/-44). upsert at L:5075-5148 (ON CONFLICT 5105; enabled 5111). Card matches L | W: grpc.rs:4904-4979. L: grpc.rs:5075-5148 |
| 007 | apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go:100-260 | capability-core list omits origin | MATCH | clean | skillRow 81-99 (no Origin); listOrCreate 134-171 (SELECT 144-146 has no origin); get 236-256 |
| 007 | apps/Model Plane/go/services/capability-core/internal/sessionreview/trigger.go:40-44 | SessionRef drops envelope user_id | MATCH | clean | SessionRef 40-44 (OrgID, RunID, ThreadID; no user); ParseRunCompleted sets OrgID and RunID only (69) |
| 007 | go/pkg/envelope/envelope.go:30-41 | envelope carries user_id | MATCH | clean | apps/Model Plane/go/pkg/envelope/envelope.go:30-42 (UserID 40) |
| 007 | orchestrator-core/cmd/workflows/skill_promotion.go:1-60 | SkillPromotionWorkflow persists nothing | MATCH | clean | apps/Model Plane/go/services/orchestrator-core/cmd/workflows/skill_promotion.go:53-62 (step 3 not implemented, 59-61); ErrRegistryUpdateNotSupported 25-30 |
| 007 | migrations/0031_agent_skills_ownership.sql | scope, owner_user_id, shared_with columns | MATCH | clean | apps/Model Plane/rust/services/session-core/migrations/0031_agent_skills_ownership.sql:11-25 (defaults org, empty, []; CHECK agent_skills_scope_chk 16-25) |
| 007 | agent_memory review_state and 0037 lineage (no lines) | reuse for memory lineage and review | MATCH | clean | review_state in session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:226 (default 'accepted'); 0037_agent_memory_lineage.sql:66-74 (supersedes_id, index) |
| 007 | apps/Model Plane/rust/services/session-core/src/learning_events.rs (ZDR and waived withholding) | ZDR and waived runs withheld | ZDR: MATCH (enqueue_run_completed 185; ZDR skip counter and log 199-202; identity.zdr check 257-263). Waived: ABSENT | M (+66/-11). Waived withholding at L:158-178, 228-232, 254; tests L:543-573 | W (ZDR): learning_events.rs:185-205, 257-263. L (waived): learning_events.rs:158-178 |
| 007 | apps/Model Plane/go/services/capability-core/internal/sessionreview/retention.go (waived withholding) | waived runs withheld in retention.go | DRIFT or ABSENT for waived. W has only the fail-closed ZDR posture (RetentionPosture 31-95). The token "waive" does not occur in W or L | clean | retention.go:31-95 (ZDR posture; AllowsDerivedPersistence 76-77) |
| 007 | OwnershipStore (no path) | reuse gateway ownership store | MATCH | clean | apps/Model Plane/rust/services/model-gateway/src/ownership.rs:59-260 (Ownership 59; OwnershipStore 136; set_shares 249). Skill use of it not verified |
| 007 | MatchSkills and fetch_skill_context pull ListAgentSkills per org per TTL, then prune (no path) | per-org TTL cache with prune | MATCH | sse.rs M (fetch_skill_context L:5662, list call L:5689); grpc.rs M; skills.rs clean | skills.rs:29 (LEARNED_SOURCE_PREFIX), 37 (TTL 60), 39-48 (env SKILL_CACHE_TTL_SECONDS), 58 (loaded keyed by org), 81-91, 96-101 (upsert then prune). grpc.rs:1693-1726 (match_skills; list call 1708). sse.rs:5528-5602 (fetch_skill_context; list 5557; warn 5581) |
| 007 | CHECK constraint on skill scope | org or user only | MATCH | clean | 0031:16-25; registry_apis.go:101-103 (validSkillScope) |
| 007 | do not revive PromoteSkill | RPC removed | MATCH | clean | capability-core/internal/server/server.go:606-621 (deliberately unimplemented); server_test.go:364 (TestPromoteSkillIsUnimplemented). Proto still declares it: proto/model_plane/v1/capabilities.proto:30, 194-200 |
| 007 | plan I23 deferred | deferred | MATCH | clean | apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:274 (I23 row), 616 (deferred to Wave 6), 674 |
| 007 | OD4 open | owner decision pending | MATCH (plan text as of 2026-10-01) | clean | same plan file :432-435 (OD4 question; blocks A0 and I23), :716 (A0 fix) |

### 2c. Claims with no code citation

| card id | claim | verdict | evidence |
|---|---|---|---|
| 020 | on, paused, off user state | ABSENT | 0039 has no state column; memory_control.rs has no pause. No match for memory_paused or memoryPause in apps/ |
| 020 | org memory policy in Control Plane | ABSENT | org-core/internal: 0 matches for memory or learning |
| 020 | memoryEnabled enforced anywhere | ABSENT (no reader) | memoryEnabled occurs only in user-core handlers.go (1410, 1421) among Go, Rust, TS, SQL, proto and md files under apps/. Other hits are user_memory_controls, not memoryEnabled |
| 020 | Ask-mode remember path | ABSENT | inline_tool_allowed refuses save_memory (W tool_loop.rs:616; test at 9107) |
| 020 | skill review honours user memory state | ABSENT | review consumer is capability-core (sessionreview/trigger.go, consumer.go, llmreviewer). No user-memory check exists in Model Plane outside session-core |
| 007 | Learning inbox (diff, reason, source run) | ABSENT | SkillsSection.tsx has an enable toggle (103) and status chip (183-184, aktiv/deaktivert). No origin, learned or proposal label. Mounted WorkspaceSettingsPage.tsx:478 |
| 007 | skill_revisions table, ledger, undo | ABSENT | no match for skill_revisions, learning_ledger or Learning inbox in apps/Model Plane or verevonv3/src |
| 007 | seeded end-to-end eval, cost controls | ABSENT | no eval or seed match in capability-core/internal/learning |
| 007 | origin in capability-core list | ABSENT in list (claim MATCH) | registry_apis.go:144-146 SELECT lacks origin; skillRow lacks Origin |
| 007 | gateway skill cache per user | not present; keyed by org | skills.rs:58, 81-91 |
| 007 | admin promotion of a proposal into an org skill | ABSENT | no promotion route; PromoteSkill removed (server.go:606-621). Org-scope creation is admin-only at registry_apis.go:198-201 (POST) |

## 3. Card-versus-code discrepancies (for the planner)

1. Loop default. Code is on (`cmd/main.go:750-752`). The compose stack defaults it off (`deploy/docker-compose.yml:1429`, cost reason in comment 1417-1428). `docs/AGENT_QUALITY_PLAN_2026-07-29.md:152` says "default on", which is stale against compose.
2. Waived withholding. Card places it in `learning_events.rs` and `sessionreview/retention.go`. Only L has it (learning_events.rs L:158-178; dreaming.rs L:216-219, 582-617, 1248-1260; memory_dedup.rs L:+5 at ~794; auth.rs L:+396). Design source: `docs/decisions/ledger.md` about line 238-246 ("waived run" decision D-B: memory indexing, dreaming extraction and learning review are skipped for it).
3. Line numbers. Card lines for grpc.rs, sessions.proto, sse.rs and tool_loop.rs match L. W is shifted (for example upsert_agent_skill 4904 in W vs 5075 in L). Use the corrected ranges in section 2.
4. ZDR memory. Card cites zdr_features.rs, which is LIVE-ONLY. W enforces the ZDR branch structurally: zdr_direct_stream returns at sse.rs:1069, before the recall gate at 1586. W zdr_precheck.rs has no memory term.
5. Enable state. The sink hardcodes `Enabled: true` (`skillsink/sink.go:55`), and the conflict arm overwrites `enabled` (grpc.rs:4940 W; 5111 L). So any later background upsert re-enables a row an admin disabled. The reversible pause RPC `set_agent_skill_enabled` exists (W grpc.rs:4849-4902; L 5020) but the sink does not use it.
6. Origin default. `0006_agent_skills_origin.sql:11-12` sets origin default 'user' (protected). The proto default "" maps to background_review (grpc.rs:4917-4918). The card does not mention either.
7. Third ListAgentSkills consumer. `model-gateway/src/tool_loop.rs:3291-3325` (reattach_skill, enabled_only false) is not in the card's "How to build it". Any per-user cache change must cover it.
8. Second memory API. capability-core serves `/api/v1/memory`, `/api/v1/memory/resolve` and `/api/v1/memory/{id}` (`internal/api/workplane_apis.go:25, 181-184`), with private-scope logic and review_state PATCH tests (`memory_auth_test.go:108-555`). Ownership rule: session-core owns the canonical memory index (`docs/capability-ownership-matrix.md:57`). Callers not determined.
9. Ownership split for agent_skills. capability-core writes and reads the table directly through its own pool (`registry_apis.go:143-152, 214-220`). The learning path writes through session-core gRPC (`skillsink` to UpsertAgentSkill). DDL lives in session-core migrations (0004:191, 0006, 0031).
10. Correct-memory mechanics. `memory.proto` has no CorrectMemory RPC (service at line 12 with IndexMemory 17, ListMemory 22, DeleteMemory 27). The gateway composes a create (memory.rs:143-151) and a delete.
11. Stale docs. `docs/core-research/capability-core.md:132` says the promotion workflow is "genuinely wired". `docs/ARCHITECTURE.md:63` lists PromoteSkill as implemented. Code (server.go:606-621) and `orchestrator-core/README.md:81` say removed (SKILL-2).

## 4. Affected-file map (depth 2 from card symbols)

Column "L" gives the live status for the file. "Tests" lists existing coverage where cheap.

### 4a. session-core (Rust, Model Plane)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| rust/services/session-core/src/memory_control.rs | user memory controls: lock_user 5, permits_extraction 25 (extraction gate), record_forgetting 36, filter_forgotten 61, retain_recallable 88, erase_retired_mirrors 95, erase_late_mirror 112 | 1-200 | clean | hidden_ids_are_dropped_and_everything_else_keeps_its_order (149); a_retired_rows_mirror_is_filtered_but_a_current_or_unknown_one_is_not (164) |
| rust/services/session-core/migrations/0039_memory_user_control.sql | user_memory_controls, forgotten_memory_ids | 1-16 | clean | none |
| rust/services/session-core/src/dreaming.rs | Dreaming loop; env config 145-156; extraction gates via permits_extraction at 746 and 807; user_memory_controls extract_after gate 1235, 1283; forgotten check 2627; record_forgetting 1759, 1792; erase_retired_mirrors 334 | 135-2650 | M (+47/-5). L adds thread_has_waived_run 1248-1260 and waived-row exclusion 582-617 | forgotten-id SQL check at 2627 (test or helper not read) |
| rust/services/session-core/src/memory_grpc.rs | ListMemory filter 428 and 593; DeleteMemory mirror erasure 510; test at 1132 | ~1-1140 | M (+107/-16). L adds ZDR list no-op test and zdr-receipt delete test | list_is_a_noop_not_an_error_for_a_zdr_caller (L only); zdr_receipt_memory_delete_is_open_to_the_zdr_owner_and_stays_scoped (L only) |
| rust/services/session-core/src/memory_dedup.rs | forgotten filters 795, 1343, 1677, 1711 | 1-1800 | M (+5 at ~794 waived clause, L only) | none cheap |
| rust/services/session-core/src/memory_grounding.rs | GroundingClassifier used by Dreaming | - | clean | not determined |
| rust/services/session-core/src/memory_erasure.rs | semantic-copy erasure (erase_semantic_copies) | - | clean | not determined |
| rust/services/session-core/src/gdpr.rs | purge of memory controls; erasure completeness 440-460 | 159-201, 404-414, 440-460, 624-625 tests | clean | gdpr tests in module near 624 (names not determined) |
| rust/services/session-core/src/learning_events.rs | RUN_COMPLETED producer; kill switch 61-68; enqueue 185; ZDR skip 199-202, 257-263 | 1-600 | M (+66/-11). L adds waived class 158-178 | kill_switch_defaults_on 464; zdr_runs_are_never_announced 513; both_terminal_paths_enqueue_the_learning_event 533; L adds zdr_and_waived_runs_are_never_announced 547 |
| rust/services/session-core/src/grpc.rs | upsert_agent_skill W 4904-4979 / L 5075-5148; set_agent_skill_enabled W 4849-4902 / L 5020; list_agent_skills W 5114 / L 5285 (SKILL-1 filter, comment 5142-5152 in W) | 2784 and 11738 (W tests) | M (+817/-44) | forgotten_memory_cannot_return_from_pending_or_inflight_extraction (W 2784); list_agent_skills_scope_enforcement_against_real_pg (W 11738, L 11985) |
| rust/services/session-core/src/main.rs | module list | 34 | M (+1: `mod retention_receipt;`) | none |
| rust/services/session-core/src/auth.rs | ZdrWaiver credential type and claim parsing | 56-110 | M (+396, L only) | not determined |
| rust/services/session-core/src/terminalization.rs | run terminal write path with retention receipt | - | M (+108/-15, L only) | not determined |
| rust/services/session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql | agent_skills DDL (line 191); agent_memory.review_state (226) | 191-234 | clean | none |
| rust/services/session-core/migrations/0006_agent_skills_origin.sql | origin column, default 'user', CHECK | 11-25 | clean | none |
| rust/services/session-core/migrations/0031_agent_skills_ownership.sql | scope, owner_user_id, shared_with, CHECK | 11-25 | clean | list_agent_skills_scope_enforcement_against_real_pg |
| rust/services/session-core/migrations/0037_agent_memory_lineage.sql | supersedes_id and index | 66-74 | clean | none |
| rust/services/session-core/migrations/0044_run_retention_receipt.sql | retention receipt (waiver plumbing) | - | LIVE-ONLY (??) | none in W |
| rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql | waived-run index | - | LIVE-ONLY (??) | none in W |
| rust/services/session-core/src/retention_receipt.rs | retention receipt module | - | LIVE-ONLY (??) | not determined |

### 4b. model-gateway (Rust, Model Plane)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| src/sse.rs | chat SSE path. Memory recall gate 1581-1597; ZDR early return zdr_direct_stream 1062-1069; memory_recall event 2101; fetch_chat_memory_context def 5060; fetch_skill_context 5528-5602 | as listed | M (+814/-69). Gate L:1655; fetch_chat_memory_context L:5191; fetch_skill_context L:5662 | source-scan test in memory_prefetch.rs:178-200 (looks for "async fn fetch_chat_memory_context") |
| src/memory_prefetch.rs | is_trivial_prompt 90 | 90-207 | clean | 5 tests: empty_slash_and_bare_acknowledgements_are_trivial 112; norwegian_acknowledgements_are_trivial_too 131; real_questions_are_never_trivial 147; every_prefetch_exit_records_an_outcome 178; empty_and_timeout_are_separate_labels 207 |
| src/memory_provenance.rs | memory_context_block (used at sse.rs:1597) | - | clean | not determined |
| src/tool_loop.rs | inline_tool_allowed 615-620 (save_memory refused, 616); recall_memory inline arm 3151; reattach_skill ListAgentSkills 3291-3325 | as listed | M (+370/-55). fn at L:639 | inline_loop_rejects_side_effects_that_require_governed_agentic_approval (assertion at 9107) |
| src/skills.rs | SkillStore: TTL 37-48; per-org loaded map 58; is_org_loaded 81-85; mark_org_loaded 88-91; replace_learned 102 (upsert then prune) | 29-120 | clean | TTL test near 453-458 |
| src/grpc.rs | match_skills 1693-1726 (ListAgentSkills call 1708); test stub list_agent_skills 3639 | as listed | M (+31/-16) | stub tests |
| src/http_routes.rs | /v1/memories 192-193; /v1/skills 371-375; list_memories 8618; delete_memory_entry 8755; proxies 5557, 5593 | as listed | M (+190/-47) | not determined |
| src/ownership.rs | OwnershipStore for MCP ownership (reuse pattern in card) | 59-260 | clean | unit tests near 339-358 |
| src/org_quota.rs | fetch_org_limits, service credential pull | 79-140 (tests 164-197) | clean | micro-dollar and limits tests |
| src/zdr_precheck.rs | ZDR precheck (no memory term in W) | - | M (+58/-226; feature table moved out) | not determined |
| src/zdr_features.rs | ZDR feature treatment table (memory row 165-168) | - | LIVE-ONLY (??) L:145-181 | tests/zdr_feature_table.rs (live-only) |
| src/zdr_posture.rs | ZDR posture | - | LIVE-ONLY (??) | tests/zdr_posture_matrix.rs (live-only) |
| src/audit.rs | recall_memory audit mapping | 18, 44 | clean | not determined |
| tests/skill_budget_contract.rs | skill injection budget contract (ledger 2026-08-22) | - | clean | itself |

### 4c. execution-core (Rust, Model Plane agent loop)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| src/runtime_loop/mod.rs | SAVE_MEMORY_TOOL 71; RECALL_MEMORY_TOOL 77; comments 64, 2150 | 71-77 | M | save_memory_is_refused_on_zdr_with_an_honest_explanation (W 1840) |
| src/runtime_loop/agent.rs | memory tool offers and prompt composition | 3639, 3708-3734, 4228-4233 | M | fixtures and prompt tests in same file |
| src/runtime_loop/skill_budget.rs | skill budget in agent loop | - | clean | not determined |
| src/capability_policy.rs | save_memory maps to cap.memory.index (808); recall_memory to cap.memory.search (809) | 808-809 | clean | not determined |
| src/permission/mod.rs | is_restricted_context_write("save_memory") | 182; tests 304-311, 595-685 | clean | in-file tests |

### 4d. capability-core (Go; module at services/capability-core/go.mod)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| cmd/main.go | learning-review env gate and consumer start | 742-803; service token reason 893 | clean | servicetoken_wiring_test.go (exists) |
| internal/sessionreview/trigger.go | SessionRef; ParseRunCompleted; OnRunCompleted | 40-70, 105 | clean | TestParseRunCompleted_ValidWithThreadId (23); TestOnRunCompleted_FetchesThenReviewsAndPersists (93); TestOnRunCompleted_SourceSkipNeverReachesReviewer (126) |
| internal/sessionreview/consumer.go | SessionClient interface with UpsertAgentSkill (46); run consumer | 40-60 | clean | TestHandleRunCompleted_EndToEnd (263); TestHandleRunCompleted_RefusesRunWithoutRetentionPermission (315); TestParseRetentionPostureDistinguishesAbsentFromFalse (355) |
| internal/sessionreview/transcript.go | Fetch(SessionRef) reads the transcript | 43 | clean | transcript_test.go |
| internal/sessionreview/retention.go | RetentionPosture, fail-closed ZDR | 31-95 | clean | consumer_test.go:355 |
| internal/skillsink/sink.go | Persist: writes UpsertAgentSkill with Enabled true (55), origin (56); no scope or owner | 46-68 | clean | TestPersist_MapsFieldsAndCountsWritten (35); TestPersist_SkippedProtectedNotCounted (65); TestPersist_RpcErrorPropagates (76) |
| internal/learning (reviewer and SelectForPersistence) | persistence policy, origin forced to background_review | - | clean | learning_test.go: 13, 29, 42, 51, 62, 74; loop_composition_test.go: 57, 102, 123 |
| internal/llmreviewer/reviewer.go | LLM reviewer (origin forced) | - | clean | reviewer_test.go |
| internal/api/registry_apis.go | SkillsHandler routes 65-67; skillRow 81-99; listOrCreate 134-171 (visibility filter); POST insert 214-220 (direct table write); get 236-256 | as listed | clean | skills_ownership_test.go: TestSkillCreateOrgScopeRequiresAdmin (40), TestSkillCreateAdminCanCreateOrgScope (61), TestSkillCreateBindsOwnerToCallerNotClientSuppliedValue (88), TestSkillListPassesCallerAndAdminForVisibilityFiltering (110), TestSkillShareRequiresOwner (144), TestSkillShareRejectsOrgScope (166), TestSkillShareByOwnerCleansGrantees (185), TestSkillShareUnknownSkillIsNotFound (216) |
| internal/api/workplane_apis.go | MemoryHandler (25); routes /api/v1/memory, /resolve, /{id} (181-184) | as listed | clean | memory_auth_test.go: 10+ tests from 108 to 702 (grep output truncated) |
| internal/server/server.go | PromoteSkill deliberately unimplemented | 606-621 | clean | server_test.go:358-371 (TestPromoteSkillIsUnimplemented); grpc_auth_test.go:100-104 |
| internal/authz/authz.go | PromoteSkill special case | 144 | clean | authz_test.go:132-148 |

### 4e. orchestrator-core (Go, Model Plane)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| cmd/workflows/skill_promotion.go | SkillPromotionWorkflow; step 3 not implemented | 14-62 | clean | skill_promotion_test.go (exists, not read) |
| cmd/workflows/feedback_promotion.go | feedback promotion (related producer) | - | clean | feedback_promotion_test.go |
| cmd/activities/activities.go | skill activities (ValidateSkillBundle, CheckSkillPromotion, PromoteSkill refs) | - | clean | integration_test.go |

### 4f. Frontend Plane, verevonv3 (gateway and UI)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| apps/gateway/src/domains/memory.rs | /api/v1/memory routes 48-61; list 68; correct 116-151 (create then delete); delete 242 | as listed | clean | none (no #[test] in file) |
| apps/gateway/src/domains/agent_actions.rs | /api/v1/skills routes 41-43; list 122; create 154 (admin via can_author_skills 66); update 191; delete 233 | as listed | clean | none (no #[test]) |
| apps/gateway/src/domains/chat/shared.rs | apply_org_zdr_posture 95-117 | 95-117 | clean | none in file |
| apps/gateway/src/domains/actions/dispatchers.rs | workflow policy PUT to user-core /api/v1/settings/ai (666-669, workflowPolicies only; no memoryEnabled) | 655-684 | clean | not determined |
| src/features/settings/components/MemorySection.tsx | list and delete UI | 69-85 | clean | none (no MemorySection test file) |
| src/features/settings/components/SkillsSection.tsx | skill list, enable toggle (103), status chip (183-184) | 34-310 | clean | none (no SkillsSection test file) |
| src/features/settings/components/WorkspaceSettingsPage.tsx | mounts SkillsSection (478) and MemorySection (496) | 7, 13, 478, 496 | clean | not determined |
| src/shared/api/memory-client.ts | listMemories 69; deleteMemory 101; correctMemory 169 | as listed | clean | memory-client.test.ts |
| src/shared/api/skills-client.ts | /api/v1/skills list 41-46, create 56, update 74, delete 86; MAX_PICKED_SKILLS 34 | as listed | clean | none |
| src/shared/actions/ (action-registry.ts, agent-tools.ts matched the term search) | no dedicated memory or skill action contract found | - | clean | not determined |
| tests/e2e/product-memory.spec.ts | Q06 correction and forgetting persist (22); Q06/Q07 isolated task creates no personal memory (77) | 22, 77 | clean | Playwright, not run |

### 4g. Control Plane (user-core, Go)

| path | role | approx lines (W) | L | existing tests |
|---|---|---|---|---|
| internal/http/handlers.go | AISettings (memoryEnabled 1410, personalizationEnabled 1409); getAISettings defaults 1419-1422; updateAISettings 1433 | 1405-1440 | clean | gdpr_handlers_test.go, inbox_workspace_handlers_test.go, mail_task_handlers_test.go, delegated_user_proof_test.go. No settings or AI test file found |
| internal/users/gdpr.go | user erasure (Control Plane side; not the card's gdpr.rs) | - | clean | gdpr_test.go |
| migrations/ | latest 029_mail_task_addresses.up.sql | - | clean | database contract tests |
| org-core/internal | no memory or learning policy (0 matches) | - | clean | - |

### 4h. Model Plane deploy and docs

| path | role | approx lines (W) | L | notes |
|---|---|---|---|---|
| deploy/docker-compose.yml | capability-core service 1378; LEARNING_REVIEW_MODEL 1416 (claude-sonnet-4-5); LEARNING_REVIEW_ENABLED default false 1429; AUTH_CORE_URL 1406; CAPABILITY_CORE_SERVICE_API_KEY 1408 | 1378-1432 | clean | - |
| docs/decisions/ledger.md | settled rulings: memory-adapter registry deferred (about 1390); semantic-memory erasure (1429); skill budget (1470); memory provenance (1694); disabled is not not_found (about 1903); waived run D-B (about 238-246) | as listed | clean | - |
| docs/capability-ownership-matrix.md | canonical owners (see section 7) | 57-59, 109-114, 130, 143, 157 | clean | - |
| docs/sse-event-taxonomy.md | memory_recall row 28 | 28 | M | - |
| docs/AGENT_QUALITY_PLAN_2026-07-29.md | stale: says LEARNING_REVIEW_ENABLED default on (152); cites PromoteSkill at server.go:532 (184) | 152, 184 | clean | - |

## 5. Related contracts

- Proto (Model Plane `proto/model_plane/v1/`): `sessions.proto` UpsertAgentSkillRequest W 831-843 / L 842-853 (origin 9 only; no scope, owner or enabled-state field beyond enabled); ListAgentSkillsRequest W 857-863 (enabled_only); AgentSkill W 865-882 (scope 10, owner_user_id 11, shared_with 12); ListAgentSkillsResponse W 884-886; SetAgentSkillEnabledResponse W 822-829. L adds SessionRetentionService.CountWaivedRuns (`sessions.proto` L:196-206, messages about L:1219-1249). `memory.proto`: MemoryService (12) with IndexMemory (17), ListMemory (22), DeleteMemory (27); no CorrectMemory. `capabilities.proto`: PromoteSkill (30, 194-200), unimplemented. `gateway.proto`: MatchSkills (line not checked).
- SQL: session-core migrations latest in W is `0043_audit_outbox_requeue.sql` (43 files). L adds `0044_run_retention_receipt.sql` and `0045_zdr_waived_runs_org_index.sql` (both `??`) plus `revert/*.down.sql`. Relevant W files: 0004 (agent_skills DDL 191; agent_memory.review_state 226), 0006 (origin default 'user'), 0031 (scope, owner_user_id, shared_with; CHECK), 0037 (supersedes_id), 0039 (user_memory_controls, forgotten_memory_ids). user-core latest: `029_mail_task_addresses.up.sql`.
- Config and env: `LEARNING_REVIEW_ENABLED` (main.go:747; compose 1429, default false); `LEARNING_REVIEW_MODEL` (main.go:802; compose 1416); `SKILL_CACHE_TTL_SECONDS` (skills.rs:42, default 60); `DREAMING_INTERVAL_SECS` (dreaming.rs:146) and `DREAMING_BATCH_SIZE` (150); compose service credentials `CAPABILITY_CORE_SERVICE_ID` and `CAPABILITY_CORE_SERVICE_API_KEY` (1407-1408); L adds retention and waiver env if any (not determined).
- SSE: `memory_recall` event (sse_events.rs:303 variant, 364 category, 413 name, 557, 1847 tests; `docs/sse-event-taxonomy.md:28`). No skill or learning SSE event exists (ABSENT).
- Gateway routes (Frontend gateway, public): GET `/api/v1/memory`; PATCH and DELETE `/api/v1/memory/{id}` (`memory.rs:50-53`); GET and POST `/api/v1/skills`; PATCH and DELETE `/api/v1/skills/{skill_id}` (`agent_actions.rs:41-43`). GET and PUT `/api/v1/settings/ai` on user-core (`handlers.go:1413-1433`).
- Model-gateway routes: GET and POST `/v1/memories`; DELETE `/v1/memories/:memory_id` (`http_routes.rs:192-193`); GET and POST `/v1/skills`; `/v1/skills/:id` (`http_routes.rs:371-375`).
- capability-core routes: `/api/v1/skills`, `/api/v1/skills/` (`registry_apis.go:65-67`); `/api/v1/memory`, `/api/v1/memory/resolve`, `/api/v1/memory/` (`workplane_apis.go:181-184`).
- gRPC: session-core MemoryService (memory.proto); SessionCore UpsertAgentSkill, ListAgentSkills, SetAgentSkillEnabled (sessions.proto); CapabilityCore PromoteSkill (unimplemented).
- Action contracts (`src/shared/actions`): none dedicated to memory or skills found.

## 6. Test and build commands

Rust, Model Plane (`apps/Model Plane/rust`, workspace members include `crates/mp-contracts`, `crates/mp-ids`, `crates/mp-events`). Crate names are from `services/*/Cargo.toml` line 2.
- `cargo test -p session-core --all-targets` (crate `session-core`)
- `cargo test -p model-gateway --all-targets` (crate `model-gateway`; includes `tests/skill_budget_contract.rs`)
- `cargo test -p execution-core --all-targets` and `cargo test -p inference-core --all-targets`
- CI form (`.github/workflows/model-plane-rust-ci.yml`): `cargo fmt --all -- --check` (line 99), `cargo check --workspace` (105), `cargo check --workspace --all-targets` (112), `cargo test --workspace --no-fail-fast` (119). This CI file is M in L.
- Windows caveat (user memory note model-plane-build-and-deploy): two cargo targets fail pre-existing on Windows; use `--lib`.

Go, Model Plane (no `go.work`; one `go.mod` per service; no `go.mod` at `apps/Model Plane/go` root):
- `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`
- `cd "apps/Model Plane/go/services/orchestrator-core" && go test ./...`
- `cd "apps/Model Plane/go/pkg/envelope" && go test ./...`
- Durable Postgres check: `apps/Model Plane/scripts/verify-durable-layer.sh` (throwaway Postgres with all migrations; G7 upsert provenance guard; `docs/capability-ownership-matrix.md:157`).
- `-race` needs cgo (user memory note).
- Module path of capability-core not read (imports use `github.com/triodelab/model-plane/...`).

Control Plane, user-core (`apps/Control Plane/user-core`, module `github.com/I-Dacosta/AquatiqCMS/apps/user-service-go`, go.mod:1):
- `go test -v ./...` (Makefile:44-46); `make test-coverage` (Makefile:48-50).

Frontend, verevonv3 (`apps/Frontend Plane/verevonv3`, package.json):
- `pnpm lint` (line 11, `eslint .`); `pnpm typecheck` (17, `tsc -b`); `pnpm test` (13: node --test on gateway scripts, then `vitest run`); `pnpm test:e2e` (15, Playwright, includes `tests/e2e/product-memory.spec.ts`).
- Targeted: `pnpm vitest run src/shared/api/memory-client.test.ts`.
- Gateway crate `verevon-gateway-rs` (`apps/gateway/Cargo.toml`): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md build block).
- Caveat (user memory note verevonv3-worktree-verification): pre-existing typecheck, lint and vitest failures. Diff the failing set against HEAD, not the exit code.

Stack: `./scripts/compose.sh up -d` in `apps/Model Plane` (Model Plane CLAUDE.md); compose file `apps/Model Plane/deploy/docker-compose.yml`.

## 7. Owners

- Memory preference, org policy, entitlements: Control Plane (root `CLAUDE.md:23`: "Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements"). The card's plan keeps the preference in user-core and org policy in Control Plane (card line 642).
- Memory enforcement (Dreaming, prefetch, save, recall, forgetting, memory index): Model Plane, session-core canonical (`docs/capability-ownership-matrix.md:57`, 109-110; `apps/master-ownership-matrix.md:126`). Root `CLAUDE.md:27` assigns reasoning and sessions and runs to Model Plane.
- Learned skills registry: capability-core (`docs/capability-ownership-matrix.md:59`, 113-114; `apps/master-ownership-matrix.md:130`). SkillPromotionWorkflow writes into it (114). The learning loop is a producer of candidates, not a store (114; G7 at 130 and 143).
- Ownership tension: capability-core owns the registry in the matrix, but its HTTP handler writes `agent_skills` directly (`registry_apis.go:214-220`), and the learning path writes through session-core gRPC. Flag for the owner decision (card OD4 and the three-tier library decision).
- Gateway and BFF: Frontend Plane (root `CLAUDE.md:29`; ZDR stamp pattern in `chat/shared.rs:95-117`).
- Cross-plane data: no direct DB crossing (root `CLAUDE.md:32`). The org-policy pull must go through a service principal (pattern `org_quota.rs:99-140`), not a DB read.
- ZDR: must propagate through every content-persisting boundary (root `CLAUDE.md:35`; ledger waived-run decision about line 238-246).
- One canonical owner per capability (`apps/Model Plane/CLAUDE.md`, Code Style section; check `docs/capability-ownership-matrix.md` before adding a registry or store). Relevant to the second `/api/v1/memory` surface in capability-core (section 3, item 8).

## 8. Uncommitted-only (live tree)

Tracked and modified (`M`), with the diff stat from `git --no-optional-locks diff --stat` in L:
- `proto/model_plane/v1/sessions.proto` (+42): adds SessionRetentionService.CountWaivedRuns and its messages. UpsertAgentSkillRequest shifted by +11, contents unchanged.
- `rust/services/session-core/src/grpc.rs` (+817/-44): retention_class, waiver and retention-receipt references; upsert and list shifted about +171; SKILL-1 test at L:11985.
- `rust/services/session-core/src/learning_events.rs` (+66/-11): waived-run class maps to SkippedWaived (L:158-178); metric and log (228-232); tests (543-573).
- `rust/services/session-core/src/dreaming.rs` (+47/-5): thread_has_waived_run (L:1248-1260); waived rows excluded from grounding candidates (L:582-617); run() config at lines 135-160 unchanged in L (first hunk at :215).
- `rust/services/session-core/src/memory_grpc.rs` (+107/-16): ZDR caller list no-op and zdr-receipt delete behaviour and tests.
- `rust/services/session-core/src/memory_dedup.rs` (+5): NOT EXISTS clause on zdr_waived runs (about L:794-799).
- `rust/services/session-core/src/main.rs` (+1): `mod retention_receipt;`.
- `rust/services/session-core/src/auth.rs` (+396): ZdrWaiver credential type and claims.
- `rust/services/session-core/src/terminalization.rs` (+108/-15): retention receipt write path.
- `rust/services/session-core/src/orchestration_grpc.rs`, `orchestration_store.rs`, `run_service_grpc.rs`: M, not diffed.
- `rust/services/model-gateway/src/sse.rs` (+814/-69): memory gate moved to L:1655; ZDR feature wiring. Not enumerated further.
- `rust/services/model-gateway/src/tool_loop.rs` (+370/-55): inline_tool_allowed moved to L:639. Not enumerated.
- `rust/services/model-gateway/src/zdr_precheck.rs` (+58/-226): ZDR feature table moved to zdr_features.rs (`??`).
- `rust/services/model-gateway/src/http_routes.rs` (+190/-47), `grpc.rs` (+31/-16), `state.rs` (+5/-1): not enumerated.
- `rust/services/execution-core/src/runtime_loop/agent.rs`, `mod.rs`, `session_core_rules_double.rs`: M, not diffed.
- `docs/sse-event-taxonomy.md`, `docs/ZDR.md`, `api/contracts/README.md`, `.github/workflows/model-plane-rust-ci.yml`: M, not diffed.

Untracked, live-only (`??`):
- `rust/services/model-gateway/src/zdr_features.rs`, `zdr_posture.rs`, `relevance_calibration.rs`, `calibration/`, `tests/zdr_feature_table.rs`, `tests/zdr_posture_matrix.rs`, `tests/fixtures/zdr-posture-matrix.json`
- `rust/services/session-core/src/retention_receipt.rs`; `migrations/0044_run_retention_receipt.sql`; `migrations/0045_zdr_waived_runs_org_index.sql`; `migrations/revert/` (0044 and 0045 down files)
- `rust/crates/mp-contracts/src/retention.rs`, `zdr_refusal.rs`, `tests/zdr_retention_matrix.rs`; `api/contracts/zdr-refusals.json` (+ `.sha256`)
- `rust/services/execution-core/tests/zdr_refusal_inventory.rs`; `rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs` and related inference-core tests

Clean in L (same as W): all capability-core and orchestrator-core files cited above; `envelope.go`; `memory.proto`; `session-core/src/memory_control.rs`, `memory_erasure.rs`, `memory_grounding.rs`, `gdpr.rs`; migrations 0004, 0006, 0031, 0037, 0039; `model-gateway/src/memory_prefetch.rs`, `memory_provenance.rs`, `skills.rs`, `ownership.rs`, `org_quota.rs`; `execution-core/src/capability_policy.rs`, `permission/mod.rs`, `skill_budget.rs`; all Frontend gateway memory and skills files; `memory-client.ts`, `skills-client.ts`, `MemorySection.tsx`, `SkillsSection.tsx`, `WorkspaceSettingsPage.tsx`; user-core `handlers.go`; `deploy/docker-compose.yml`; `docs/decisions/ledger.md`; `docs/capability-ownership-matrix.md`; `apps/master-ownership-matrix.md`; `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md`; root and Model Plane `CLAUDE.md`.

## 9. Absent surfaces (for the plan's "build" items)

- Learning inbox (Settings page): no component. SkillsSection is the nearest surface and has no origin, learned or proposal state.
- skill_revisions table, "What Verevon learned" ledger, undo: none in either tree.
- Seeded end-to-end eval with cost controls: none.
- Org-level memory policy in Control Plane: none.
- Pause state for memory: none.
- Origin label in list responses to the Frontend: not in capability-core list or skillRow.
- `set_agent_skill_enabled` exists (W 4849-4902). Its callers were not traced, so it is not known whether it is used by any quality policy.

## 10. NOT DETERMINED

1. Whether any live environment sets `LEARNING_REVIEW_ENABLED=true` (only the compose default was read).
2. Whether capability-core and orchestrator-core Go tests pass in W or L (not run).
3. Callers of capability-core `MemoryHandler` (`/api/v1/memory`). None found from the Frontend or model-gateway, but not exhaustively searched.
4. Callers of `set_agent_skill_enabled` (quality policy) and whether the sink or any path uses it.
5. Live line numbers for model-gateway grpc.rs match_skills, tool_loop reattach_skill, and http_routes.rs skill and memory routes. Only W was re-derived for these.
6. Whether every ZDR turn takes the zdr_direct_stream branch before the recall gate. The branch condition above sse.rs:1062 was not read in full.
7. Whether live (M) Dreaming and recall paths keep the same extract_after gate. Only line ranges were compared.
8. Whether `src/shared/actions` contains a memory or skill action contract. action-registry.ts and agent-tools.ts matched the term search but were not read.
9. Test names and results for orchestrator-core `skill_promotion_test.go`, the dreaming forgotten-id check near 2627 (test or helper not determined), and gdpr tests near 624.
10. Whether `agent_skills` has any status or revision column beyond enabled, origin, scope, owner_user_id and shared_with (0004 DDL at 191 read only as a header).
11. Playwright coverage of the Skills settings page (product-memory.spec.ts covers memory only; not executed).
12. OD4 status after 2026-10-01 (plan text only).
13. Whether the gateway or BFF reads user-core memoryEnabled anywhere (none found in verevonv3 or Model Plane by name).
14. Whether correctMemory deletes the original: only the create half (memory.rs:143-151) was read.
15. Whether card's "Space-audience runs excluded or held as proposals" has any code representation. Not checked.
