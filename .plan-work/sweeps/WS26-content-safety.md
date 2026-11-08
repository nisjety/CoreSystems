# WS26-content-safety: code-map sweep for F-t3-033

Read-only sweep. No product code, config or docs were edited.

**Abbreviations.** WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). LIVE = `C:/dev/CoresSystem` (same HEAD plus uncommitted work; status list in `WT/.plan-work/live-tree-status.txt`). MP = `apps/Model Plane`. FP = `apps/Frontend Plane/verevonv3`. Line numbers are WT unless marked LIVE. "clean" = not listed in live-tree-status.txt.

**Audit source.** `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`
- Card F-t3-033: heading line 813, card body 814-822 (Verevon today and approach: line 820; dependencies: line 822). Tier Recommended, effort L, Model Plane, wave W2, confidence High, reference Perplexity.
- Section 9.1 W2 table row: line 1571. Summary table row: line 1850. W2 table is between the W1 and W3 wave headers (W3 header at line 1575).
- Appendix C (line 1918): no F-t3-033 entry (card is not Priority). Appendix D (line 2111): no line cites F-t3-033.
- Dependents and related cards: F-t3-007 card (437-452) says "Screening from F-t3-033 applies; depends on F-t3-020" (line 450). F-t3-097 depends on F-t3-033 (line 1658). Risk paragraph (line 1714) says F-t3-033 should land before or with F-t3-008, F-t3-010, F-t3-003, F-t3-044, F-t3-054. F-t3-020 (card 629-645) is the memory-controls dependency.

## 1. Summary

- Card claims checked: the 16-phrase substring scan with no normalisation, the shadow-only semantic pass wired at one call site, the marker-only Do-mode seam, the IndexMemory non-empty-only check, and the unscanned skill upsert all MATCH the WT code. Two citations drift (memory_grpc.rs and runtime_loop/mod.rs); see section 2.
- Two scanner copies confirmed, each with a 16-entry marker list that is identical in both: `MP/rust/services/model-gateway/src/moderation.rs:64-81` and `MP/rust/services/execution-core/src/provenance.rs:163-180`. Both carry "keep in sync by hand" comments (moderation.rs:222-226, provenance.rs:161-162). Neither copy normalises input (`to_lowercase()` only). A test pins only the TrustClass arm names (`MP/rust/services/execution-core/tests/compaction_parity_contract.rs:526-582`), not the marker list.
- Memory and skill write sites are more numerous than the card implies: three skill write paths and three memory entry points, with direct SQL writers that bypass any single chokepoint (section 5). None of them calls a screening function. `session-core/src` has no scan, screen or injection call outside test names.
- Live tree: the screening modules themselves (moderation.rs, semantic_screening.rs, provenance.rs, security_events.rs) are clean. The files that call them or write memory and skills are modified in LIVE (runtime_loop/mod.rs, tool_loop.rs, memory_grpc.rs, dreaming.rs, session-core grpc.rs, sessions.proto, and others). Their line numbers shift; see section 9.
- Safe-link gap: the on-screen chat renderer has no scheme check. `FP/src/features/chat/components/chat-media-markdown.tsx:1278-1279` passes any `[text](target)` target straight into `href`. The allowlist exists only in the export path (`ooxml.ts:115-120`).
- Shadow-telemetry risk: the card's evaluation set depends on `mp.v1.security.{org}` events. No publish grant for that subject exists in `MP/deploy/nats.conf`, and `MP/go/services/nats-provisioner/publishers_registry_test.go:150-152` records it as a gap. Postmortem 0002 row 272 says the broker drops these publishes silently.

## 2. Citation table (card and resolved code claims)

Verdict key: MATCH = behaviour present at cited place; DRIFT = present at other lines; ABSENT = not found. Live status: clean, M (modified), ?? (live-only), DIFFERS-IN-LIVE (M and cited lines moved).

| Card id | Cited path:lines | Claim (short) | WT verdict | Live status | Corrected path:lines (WT; LIVE) |
|---|---|---|---|---|---|
| F-t3-033 | `MP/rust/services/model-gateway/src/semantic_screening.rs:1-60,93-116` | Shadow pass, 20% sample, chat-only | MATCH. Module doc 1-60; `DEFAULT_SAMPLE_RATE` :94; `semantic_sample_rate` :101-110 (env `CONTENT_SAFETY_SHADOW_SAMPLE_RATE` at :104); `should_sample` :116-118 | clean | same |
| F-t3-033 | `MP/rust/services/execution-core/src/runtime_loop/mod.rs:795-820` | Do-mode seam, marker scan only | MATCH, with a note. The seam is :797-801 (`provenance::assess` :797, `audit` :798, `render` :799). The marker scan itself is `execution-core/src/provenance.rs:182-187`, not in runtime_loop. A second seam for browser output exists at :252-254 (uncited). | DIFFERS-IN-LIVE (M, +18 lines). Seam at LIVE 811-815; `execute_step_inner` at LIVE 467 (WT 455) | runtime_loop/mod.rs:797-801 (WT); LIVE 811-815 |
| F-t3-033 | `MP/rust/services/session-core/src/memory_grpc.rs:444-500` | IndexMemory checks only non-empty content | MATCH, with DRIFT. `async fn index_memory` :441-513. Content check :453-455 (`trim().is_empty()` only). Authorize :456-459. Index call :468-477 (`dreaming::index_agent_memory`). | DIFFERS-IN-LIVE (M, 123 lines changed). Fn LIVE 444; index call LIVE 471 | memory_grpc.rs:441-513 (WT); LIVE 444-516 |
| (resolved, not cited) | `MP/rust/services/model-gateway/src/moderation.rs:61-81` and `:183-186` | `scan_injection`: 16 substrings, `to_lowercase()` only, no normalisation | MATCH (16 entries counted; `to_lowercase` at :184) | clean | same |
| (resolved) | `MP/rust/services/execution-core/src/provenance.rs:161-187` | Second scanner copy, identical 16-entry list | MATCH | clean | same |
| (resolved) | `MP/rust/services/model-gateway/src/tool_loop.rs:2386` | Semantic pass is wired only into the chat tool loop | MATCH. Call sits in `assess_external_payload` (:2358-2403). The shadow task's JoinHandle is bound to `_shadow` and discarded (:2386); the returned provenance is the deterministic one. Only non-test caller in `rust/services`. Call sites of `assess_external_payload`: :2925, :3039, :3115 | DIFFERS-IN-LIVE (M, +425 lines). Fn LIVE 2382; call LIVE 2410; call sites LIVE 2914, 3030, 3106 | tool_loop.rs:2358-2403 (WT); LIVE 2382- |
| (resolved) | `MP/rust/services/session-core/src/grpc.rs:4904-4977` | Learned skills upserted with no screen | MATCH. Auth `session:skills:write` :4911; origin check :4917-4923; direct INSERT :4928-4944 (ON CONFLICT guard `origin <> 'user'` at :4942). No screening call. | DIFFERS-IN-LIVE (M, +861 lines). Fn LIVE 5075 | grpc.rs:4904-4977 (WT); LIVE 5075- |
| (resolved, card "safe links") | `FP/src/features/chat/lib/chat-export/ooxml.ts:115-120` | Scheme allowlist exists in export path | MATCH. `SAFE_HREF_PATTERN = /^(?:https?:|mailto:)/i` (:115); `isSafeExportHref` (:118-120). Export tests `ooxml.test.ts:142-157`. | clean | same |
| (resolved, card "safe links") | `FP/src/features/chat/components/chat-media-markdown.tsx:1269-1279` | On-screen links have no scheme check | MATCH (gap confirmed). Bare URL :1269-1275 (regex `^https?://` only). Markdown link :1278-1279 passes `link[2]` to `href` unchecked. No DOMPurify or scheme guard in `features/chat/components` (grep). | clean | same |
| (resolved) | `MP/rust/services/execution-core/src/provenance.rs:17-29`, `:209-213` | Do-mode has no policy client; external classes always screened | MATCH. Doc says execution-core has no capability-core HTTP policy client, so there is no `PolicyDisabled` path. | clean | same |
| (resolved) | `MP/rust/services/model-gateway/src/moderation.rs:3-8` | capability-core owns policy; gateway enforces | MATCH (doc) | clean | same |
| (resolved, card "trust-class provenance") | `moderation.rs:205-239`; `provenance.rs:31-67` | Two TrustClass copies; ExternalWeb arm names | MATCH. Both classifiers name web_search, web_fetch, fetch_url, web.search, web.read (ExternalWeb); browser_agent (BrowserScraped); mcp__ prefix (ThirdPartyMcp). Moderation :232-237; provenance :60-65. | clean | same |
| (resolved, card "hash-only security events") | `MP/rust/services/model-gateway/src/security_events.rs:1-18`, `:31-32` | Metadata and BLAKE3 hashes only | MATCH. Event types `TOOL_RESULT_SCREENING_FLAGGED` (:31) and `TOOL_RESULT_SEMANTIC_SHADOW` (:32). | clean | same |
| (resolved, card "skipped under ZDR") | `semantic_screening.rs:61-69`, `:333-350` | Shadow pass skipped on ZDR turn | MATCH. ZDR skip `ShadowSkipReason::Zdr` at :347. `AnalyzeLanguageRequest` carries no zdr field (doc :63-65). | clean | same |
| (resolved, card "never regress") | `semantic_screening.rs:49-59` | Shadow outcome can never downgrade the deterministic verdict | MATCH (doc). Shadow result is metadata only (`ShadowScreeningOutcome`, :182-184). | clean | same |
| (resolved, card dependency) | `moderation.rs:612-683` | content_safety gate for the shadow pass | MATCH. Only an explicit `enabled: false` row turns the shadow pass off. On any read failure the shadow pass runs (:629-683). The injection_defense gate also keeps screening on under failure (:437-451, :493-552). An enforcing semantic posture would need its own fail-mode decision; the comment at :622-628 justifies this default only because the gate is non-enforcing. | clean | same |

Card dependency "false-positive evaluation set": not found. A grep for false-positive and shadow-telemetry terms under MP/docs returned four documents (`MP/docs/decisions/ledger.md`, `MP/docs/GATEWAY_DEDUPLICATION_PLAN.md`, `MP/docs/chat-parity-audit.md`, `MP/docs/MODEL_PLANE_IMPROVEMENTS_2026.md`), none read for an injection set. `MP/docs/skills/eval-harness.md` exists but was not read. Status: NOT DETERMINED.

## 3. Gaps the card asks to close, with code facts

| Card step | Code state (WT) | Evidence |
|---|---|---|
| (1) Shared normaliser crate for both copies | No normaliser in either copy. Only `to_lowercase()`. No NFKC, zero-width or tag-block (U+E0000) handling in the lines read (moderation.rs 1-200; provenance.rs 1-60 and 155-270). | `moderation.rs:183-186`; `provenance.rs:182-187`. Workspace crates (rust/Cargo.toml:4-17): mp-contracts, mp-ids, mp-events, mp-telemetry, mp-slo, mp-orchestration, mp-eventlog, mp-toon, mp-tokens, mp-authctx. No normaliser crate. |
| (2) Deterministic memory policy at IndexMemory and extractor output, typed refusal | No policy on any memory write. IndexMemory checks non-empty content and thread authorization only. Extractor path (`extract_memory_candidates` then `persist_candidates`) has no check. | `memory_grpc.rs:453-459`; `dreaming.rs:717-796`; `grpc.rs:2631-2642` |
| (3) Enforcing posture for ExternalWeb and Browser, chat first, Do needs a new pass | Today a posture only changes model-facing framing; flagged or degraded results are still returned with a warning. Neither copy blocks a result. `ScreeningPosture::requires_read_only` (moderation.rs:308) has no production caller (tests only). The semantic pass never changes posture. execution-core has no policy client, so Do mode cannot read the injection_defense gate. | `provenance.rs:265-` (render); `tool_loop.rs:4502` (append_provenance_note); `semantic_screening.rs:460-475` (shadow only); `provenance.rs:17-29` |
| (4) Chat link scheme allowlist reusing isSafeExportHref | Not present on-screen. Allowlist exists only in export. | `chat-media-markdown.tsx:1275, 1278-1279`; `ooxml.ts:115-120`; `chat-markdown.test.ts` has no javascript:, data: or file: negative case (grep). |
| (5) Skill-write scan before promotion | No scan on any of the three skill write paths (section 5). The promotion workflow runs a gate activity (step 2) but its registry step is "not supported" (step 3). | `orchestrator-core/cmd/workflows/skill_promotion.go:108-148` |

Ledger precedent (`MP/docs/decisions/ledger.md:2311-2334`): hand-synced copies had drifted three ways (web.search and web.read were classified OrgInternal). The fix added a mutation-tested contract test and kept the aliases unmerged. Ledger has no entry on the semantic shadow pass (grep: none). MP/CLAUDE.md requires checking the ledger before merging two copies. `provenance.rs:12-15` says the two copies cannot share code "across the process boundary" without a proto change. That reasoning covers runtime processes, not build-time crates.

## 4. Affected-file map

Columns: path | role | approx WT lines | live status | tests (cheap list).

### 4a. Screening and provenance (Rust)

| Path | Role | Approx lines (WT) | Live | Tests |
|---|---|---|---|---|
| `MP/rust/services/model-gateway/src/moderation.rs` | Deterministic injection scan, TrustClass, ToolProvenance, screen_tool_payload, injection_defense and content_safety gates, PII redaction | INJECTION_MARKERS 61-81; TrustClass 205-239; ToolProvenance::assess 340-364; screen_tool_payload 437-485; injection_defense_enabled 493-552; content_safety gate 612-683; tests 685-1237 | clean | scan_injection_flags_override_attempts (698), scan_injection_ignores_normal_prose (707), trust_class_classifies_the_three_untrusted_buckets (916), trust_class_defaults_unknown_and_org_tools_to_internal (935), screen_tool_payload_flags_injection_markers_in_the_full_payload (1080), screen_tool_payload_respects_an_explicitly_disabled_policy (1127), screen_tool_payload_fails_closed_to_screening_on (1160), screen_tool_payload_degrades_to_read_only_over_the_size_bound (1207), screen_tool_payload_degrades_when_no_permit_is_available_in_time (1237) |
| `MP/rust/services/model-gateway/src/semantic_screening.rs` | Sampled semantic shadow pass; ContentSafetyClassifier trait; verdict parser; out-of-band spawn and event publish | docs 1-81; sample 94-118; enums 143-200; trait 228; parse_semantic_verdict 302; shadow_screen 333-392; assess_with_semantic_shadow 417-~500 (policy gate 454-459, roll 461, PolicyDisabled 474, publish 481-490); tests 501-1100 | clean | should_sample_respects_the_boundary (581); org_internal_trust_is_skipped_before_anything_else (621); a_zdr_turn_never_calls_the_classifier (647); sampling_in/out (673, 696); concurrency_bound_exhaustion_is_unavailable (739); classifier_timeout_... (763); malformed_classifier_response_is_unavailable (817); disagreement_is_recorded_not_suppressed_or_reconciled (891); semantic_verdict_ignores_claims_embedded_in_the_payload_text (923); a_completed_shadow_pass_publishes_a_comparison_event (1020); a_zdr_turn_publishes_no_shadow_event... (1070) |
| `MP/rust/services/model-gateway/src/tool_loop.rs` | Chat tool loop. Sole caller of the semantic pass. Security event publish. | assess_external_payload 2358-2403; GrpcContentSafetyClassifier 2380; call 2386; call sites 2925/3039/3115; run_tool_rounds 6246; run_tool_rounds_for_model 6334 (security publish 7360-7369) | M (+425). LIVE: fn 2382, call 2410, publish 7474 | Not enumerated. Integration: `MP/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` (exists; M in live) |
| `MP/rust/services/model-gateway/src/security_events.rs` | Builds hash-only security envelopes for tool-result screening and shadow comparisons | 1-18 (doc); event types 31-32; `envelope_for` and `shadow_envelope_for` (line not read) | clean | Test module not read. Publish assertions at `semantic_screening.rs:1060-1063` |
| `MP/rust/services/model-gateway/src/http_routes.rs` | HTTP routes; `/v1/skills` CRUD proxied to capability-core | 321 (doc); 371 (`/v1/skills`); 375 (`/v1/skills/:id`) | M (+237), but hunks start at 824, so 371-375 are unchanged in LIVE | Not determined |
| `MP/rust/services/execution-core/src/provenance.rs` | Second scanner copy. TrustClass, INJECTION_MARKERS, scan_injection, assess, render, audit | classify 53-67 (arms 61-64); INJECTION_MARKERS 163-180; scan_injection 182-187; SCREENING_MAX_BYTES 193; assess 215-251; render 265-; tests 325-451 | clean | classifies_the_three_untrusted_buckets_and_defaults_the_rest_internal (325); external_content_is_scanned_over_its_full_untruncated_text (379); a_marker_past_the_size_bound_still_degrades... (393); render_* (409-429); a_payload_cannot_forge_its_own_screening_verdict_by_claiming_it_in_text (440) |
| `MP/rust/services/execution-core/src/runtime_loop/mod.rs` | Do-mode tool-result seam; browser-output seam; `save_memory` tool (IndexMemory caller) | BROWSER_AGENT_TOOL 41; browser seam 244-254 (provenance at 252); execute_step_inner 455-812 (seam 797-801); execute_subagent 813; execute_save_memory 2154 (IndexMemory request 2187, call 2207); execute_recall_memory 2347 | M (+18). LIVE: execute_step_inner 467, seam 811-815, save_memory 2172 (request 2205, call 2225), recall 2365 | Not enumerated |
| `MP/rust/services/execution-core/src/runtime_loop/agent.rs` | Agent loop; second IndexMemory caller | IndexMemoryRequest 3366; index_memory call 3385 | M (+207). LIVE: 3373 and 3392 | Not enumerated |
| `MP/rust/services/execution-core/src/grpc.rs` | execute gRPC surface (adjacent) | not read | M (+98) | Not determined |
| `MP/rust/services/execution-core/tests/compaction_parity_contract.rs` | Source-level contract: both TrustClass classifiers name the same external tools; pins web.search and web.read | 526-582 (test fn at 542) | clean | both_trust_classifiers_treat_the_same_tools_as_external_web (542) |
| `MP/rust/crates/mp-events/src/subjects.rs` | NATS subject builders | security_subject 114-124 (`mp.v1.security.{org_id}`); test 597-599 | clean | security_subject_format (597) |

### 4b. Memory and skill writers (Rust, session-core)

| Path | Role | Approx lines (WT) | Live | Tests |
|---|---|---|---|---|
| `MP/rust/services/session-core/src/memory_grpc.rs` | IndexMemory gRPC handler; list, correct and delete memory | index_memory 441-513; tests 1070-1100 (`#[ignore]`, need DATABASE_URL) | M (+123). LIVE fn 444, call 471 | delete_memory_reports_what_happened_to_the_durable_row (1072, ignored) |
| `MP/rust/services/session-core/src/dreaming.rs` | Memory extraction, grounding and lineage writes; dream loop | dream_once 197-361; extract_memory_candidates 717-737; persist_candidates 738-796; apply_grounded_candidates 797-903; write_lineage_memory 904-1020 (UPDATE 940; INSERT 955, 988); write_on_retired_key 1127-1198 (UPDATE 1144, 1171); index_agent_memory 1516-1577; upsert_agent_memory 1578-1593; upsert_current_key 1594-1665 (INSERT 1603, 1633); tests 2035-2459 | M (+52). LIVE: persist 760, lineage 926, index_agent_memory 1558, upsert_agent_memory 1620 | durable_user_memory_is_owner_scoped_in_queries_and_uniqueness (2079); a_stated_memory_always_beats_an_inferred_one_on_the_same_key (2176); only_inferred_candidates_carry_the_extractor_marker (2220); persist_candidates_itself_has_no_grounding_dependency (2242); write_lineage_memory_flips_the_old_row_and_never_deletes_it (2278); grounded_writes_only_ever_come_from_a_validated_classifier_action (2317) |
| `MP/rust/services/session-core/src/dream_extractor.rs` | LLM memory extractor (`extract` 427). ZDR.md says a test pins it against the gateway source. | extract 427; tests 782-1170 | clean (not in status) | extraction_is_off_without_an_inference_address (1146); extraction_is_off_when_its_credential_is_missing (1155) |
| `MP/rust/services/session-core/src/grpc.rs` | (a) AppendMessage inline memory extraction; (b) skill writes | append_message_inner 2384-2400; append_message_in_tx 2430 (extract 2631, persist 2640); set_agent_skill_enabled 4842-4902 (UPDATE 4867); upsert_agent_skill 4904-4977 (INSERT 4929); tests from 2706; assert_upsert_skill_guards 9288 (G7 test helper); test INSERT 11770 | M (+861). LIVE: append_in_tx 2432, extract 2633, persist 2642, set_agent_skill_enabled ~5020, upsert 5075 | assert_upsert_skill_guards (9288); append tests around 2802-2809 |
| `MP/rust/services/session-core/src/memory_dedup.rs` | CLI dedup planner; UPDATEs agent_memory is_latest and supersedes_id | UPDATE 1517, 1708 (production); test INSERT 2525 (tests from 1830) | M (+5) | tests module at 1831 |
| `MP/rust/services/session-core/src/learning_events.rs` | Skill-learning review trigger. Doc names UpsertAgentSkill as the write. | 1-10 (doc); tests 295+ | M (+77) | Not enumerated |
| `MP/rust/services/session-core/src/memory_control.rs` | Memory user controls (module name; only its test was read). Test-only INSERT at 177. | 160-181 (test) | clean | a_retired_rows_mirror_is_filtered_but_a_current_or_unknown_one_is_not (164) |
| `MP/rust/services/session-core/migrations/` | agent_skills and agent_memory schema | See section 6 | live adds ?? 0044, 0045 and revert/ | n/a |

### 4c. Skill registry and policy (Go)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `MP/go/services/capability-core/internal/api/registry_apis.go` | HTTP skill CRUD that writes `agent_skills` directly. Comment at :110 calls capability-core "the sole write path", which conflicts with session-core grpc.rs:4929. | SkillsHandler 46-53; normalizeSkillShares doc 105-110, fn 111; listOrCreate 134 (INSERT 215-220; reconcile emit 225); get 236; update 258 (UPDATE 316); delete 328 (DELETE 329) | clean | `skills_ownership_test.go` (exists; not read) |
| `MP/go/services/capability-core/internal/skillsink/sink.go` | Learned-skill sink. Persist calls session-core UpsertAgentSkill. | 1-61 (Persist 41-61; RPC call 49) | clean | `loop_composition_test.go:70,109,130` in internal/learning |
| `MP/go/services/capability-core/internal/learning/reviewer.go` | Skill review producer. Reply parser has a scan budget but no content screen. | 96 (doc); 171-197 (scanBudget) | clean | `reviewer_markdown_test.go` (exists) |
| `MP/go/services/capability-core/migrations/0003_capabilities_registry.up.sql` | `safety_policies.kind` values: pii_filter, content_safety, injection_defense | 298 | clean | n/a |
| `MP/go/services/capability-core/internal/watch/watch.go` | Adjacent, not in card. Watch payloads are labelled `unscreened_source_payload`. | 111-123 (TrustUnscreened 119) | clean | `sweeper_test.go:53,208` |
| `MP/go/services/capability-core/internal/sessionreview/retention.go` | Adjacent ZDR note: skill rows are a content-persisting boundary | 11 (doc) | clean | n/a |
| `MP/go/services/orchestrator-core/cmd/workflows/skill_promotion.go` | SkillPromotionWorkflow. Step 2 is a gate activity. Step 3 registry update is "not supported". | 53-65 (workflow); 108-148 | clean | `skill_promotion_test.go` (exists) |
| `MP/go/services/orchestrator-core/cmd/activities/quarantine.go` | Quarantine (disable) for skills. Comment only read. | 43 (comment) | clean | `integration_test.go` (exists) |
| `MP/go/services/nats-provisioner/publishers_registry_test.go` | Publish-grant registry. Records `mp.v1.security.*` as a gap. | 148-153 (gap at 152) | clean | itself |
| `MP/scripts/verify-durable-layer.sh` | Throwaway Postgres plus all migrations. G7 upsert provenance-guard section. | 1-5 (doc); 51-66 (G7 guard) | clean | itself |

### 4d. Frontend (FP)

| Path | Role | Approx lines | Live | Tests |
|---|---|---|---|---|
| `FP/src/features/chat/lib/chat-export/ooxml.ts` | Export allowlist isSafeExportHref | 105-120 | clean | `ooxml.test.ts:142-157` |
| `FP/src/features/chat/lib/chat-export/docx.ts` | Export hyperlinks gated by isSafeExportHref | 32 (import); 253-256 (use) | clean | `docx.test.ts` (exists; not read) |
| `FP/src/features/chat/components/chat-media-markdown.tsx` | On-screen markdown and inline parser. Unguarded link href. Imported by chat-normalizers.ts:41, ChatArtifactPanel.tsx:69, chat-export/index.ts:15. | Inline parser 1262-1285; bare URL 1269-1276; markdown link 1278-1279 | clean | `chat-markdown.test.ts:370-392` (bare URL), `:154-219` (citation links). No scheme-negative test. |
| `FP/src/features/chat/components/ChatMessages.tsx` | Exports ChatMarkdown (host). Mermaid securityLevel strict (:1398). | 1398 | M (+229) | `chat-markdown.test.ts:5` imports ChatMarkdown; `ChatMessages.codeblock.test.tsx` (exists) |
| `FP/apps/gateway/src/domains/memory.rs` | Public `/api/v1/memory` GET, PATCH, DELETE proxied to session-core MemoryService | 50-53 | clean | Not determined |
| `FP/apps/gateway/src/domains/agent_actions.rs` | Public `/api/v1/skills` CRUD, admin-gated, proxied to model-gateway `/v1/skills` | 3-11 (doc); 41-43 (routes); 134, 175, 214, 255 (proxy URLs) | clean | Not determined |
| `FP/src/shared/actions/` | Action contracts | No memory, skill or link contract (glob `**/*memor*` and `**/*skill*` empty) | n/a | n/a |

### 4e. Docs

| Path | Role | Lines | Live |
|---|---|---|---|
| `MP/docs/decisions/ledger.md` | Precedent: hand-synced copies drifted; aliases not merged; contract test | 2311-2334 | clean |
| `MP/docs/ZDR.md` | ZDR boundary table. No row for the screening pass (grep of WT; LIVE not grepped). | 90-102 | M (64 lines) |
| `MP/docs/sse-event-taxonomy.md` | SSE taxonomy. No screening event (grep: none). | n/a | M (+4) |
| `MP/docs/postmortem/0002-session-audit-trail-never-delivered.md` | Security-publish grants dropped by broker | 272 | clean |

## 5. Memory and skill write sites (no screening at any of them)

**Memory writers (WT)**
1. IndexMemory gRPC: `memory_grpc.rs:441-513` calls `dreaming::index_agent_memory` (`dreaming.rs:1516`), which calls `upsert_agent_memory` (`dreaming.rs:1560`). Callers of IndexMemory: `runtime_loop/mod.rs:2154-2225` (save_memory tool, model-driven) and `runtime_loop/agent.rs:3366-3385`.
2. AppendMessage inline extraction: `grpc.rs:2631-2642` (inside append_message_in_tx at 2430) runs `dreaming::extract_memory_candidates` then `persist_candidates` on every appended message whose thread is not conversation-only (guard at :2630), inside the caller's transaction. The role filter in `extract_memory_candidates` was not read.
3. Dream loop: `dreaming.rs:197` (`dream_once`) calls `apply_grounded_candidates` (:272) and `persist_candidates` (:297).
4. Direct SQL that bypasses `upsert_agent_memory`: `dreaming.rs` 940 (UPDATE retire, in write_lineage_memory), 955 and 988 (INSERT, in write_lineage_memory), 1144 and 1171 (UPDATE, in write_on_retired_key), 1603 and 1633 (INSERT, in upsert_current_key). Also `memory_dedup.rs` 1517 and 1708 (UPDATE). So there is no single memory-write chokepoint today.

**Skill writers (WT)**
1. session-core `upsert_agent_skill` (`grpc.rs:4904-4977`). Callers: capability-core `skillsink/sink.go:49` (learning path) and any direct gRPC client (others NOT DETERMINED).
2. session-core `set_agent_skill_enabled` (`grpc.rs:4842-4902`, UPDATE at 4867). Flips the enabled flag only.
3. capability-core HTTP CRUD (`registry_apis.go` INSERT 215, UPDATE 316, DELETE 329). Reached through model-gateway `/v1/skills` (`http_routes.rs:371-375`) from the gateway (`agent_actions.rs:41-43`). Path per the doc comments at `agent_actions.rs:3-11`; the model-gateway proxy code was not read.
4. Quarantine (orchestrator-core `quarantine.go:43`) is a comment only; its RPC is NOT DETERMINED.

## 6. Related contracts

**Proto (WT line; LIVE line)**
- `MP/proto/model_plane/v1/sessions.proto`: `rpc UpsertAgentSkill` :80 (LIVE 80); `message UpsertAgentSkillRequest` :831 (LIVE 842); `message UpsertAgentSkillResponse` :845 (LIVE 856). File is M in LIVE (+42; hunks at 196 +11, and 1219 +31).
- `MP/proto/model_plane/v1/memory.proto`: `rpc IndexMemory` :17; `IndexMemoryRequest` :126; `IndexMemoryResponse` :150 (clean).
- `MP/proto/model_plane/v1/inference.proto`: `rpc AnalyzeLanguage` :63; `AnalyzeLanguageRequest` :1063; `AnalyzeLanguageResponse` :1138 (clean). Used by GrpcContentSafetyClassifier (`tool_loop.rs:2380`).
- Generated outputs (checked in): `MP/go/gen/model_plane/v1/sessions.pb.go`, `sessions_grpc.pb.go`; `MP/python/mp-events-py/src/mp_events/_gen/model_plane/v1/sessions_pb2.py`. Regenerate with `buf generate` (MP/CLAUDE.md). Rust types come from the mp-contracts crate (`session-core/Cargo.toml` dependency key `mp-contracts`).

**NATS**
- Subject `mp.v1.security.{org_id}` (`mp-events/src/subjects.rs:114-124`). Event types `TOOL_RESULT_SCREENING_FLAGGED` and `TOOL_RESULT_SEMANTIC_SHADOW` (`security_events.rs:31-32`).
- Publish grant: none in `MP/deploy/nats.conf` (grep "security": no match). Recorded as a gap at `publishers_registry_test.go:150-152`. Postmortem 0002 row 272.
- Production consumer of `mp.v1.security.*`: none found (producers, a registry test and a unit test only). Control Plane audit consumer NOT DETERMINED.

**SSE**
- No screening SSE event in `MP/docs/sse-event-taxonomy.md` (grep). LIVE +4 lines not read. `sse.rs` and `sse_events.rs` are M in LIVE and were not inspected for screening events.

**Config and env**
- `CONTENT_SAFETY_SHADOW_SAMPLE_RATE` read at `semantic_screening.rs:104` (default 0.2 at :94). No other reference under MP (grep).
- capability-core policy read: `GET /api/v1/safety` (`moderation.rs:148, 653`). Policy kinds in `capability-core/migrations/0003_capabilities_registry.up.sql:298`.

**Gateway routes (FP apps/gateway)**
- `/api/v1/memory` GET, `/api/v1/memory/{id}` PATCH and DELETE (`domains/memory.rs:50-53`).
- `/api/v1/skills` GET and POST, `/api/v1/skills/{skill_id}` (`domains/agent_actions.rs:41-43`).
- No gateway route for security events or screening results.
- Chat streams carry a capability bearer minted by the gateway, so model-gateway can read the injection_defense policy (`FP/apps/gateway/src/domains/chat/shared.rs:270-285`, `upstream.rs:1706-1707`, `domains/chat/streams.rs:28`). Without the bearer, model-gateway keeps screening on (moderation.rs:493-503). Any change to policy reads must keep this contract.

**Action contracts (FP src/shared/actions)**
- None for memory, skill, or links (glob `**/*memor*` and `**/*skill*` under `FP/src/shared/actions` returned nothing). No content-safety action found.

**Migrations (WT `MP/rust/services/session-core/migrations/`, 43 files)**
- Latest by name: `0039_memory_user_control.sql`, `0040_direct_run_completion_key.sql`, `0041_mail_task_thread_origin.sql`, `0042_thread_persona_toggle.sql`, `0043_audit_outbox_requeue.sql`.
- Skill and memory schema: `0004_tasks_cron_hooks_skills_memory.sql`, `0006_agent_skills_origin.sql`, `0031_agent_skills_ownership.sql`, `0037_agent_memory_lineage.sql`, `0038_thread_source_scope.sql` (name only; IndexMemory reads `threads.source_scope` at memory_grpc.rs:461-467; migration not read), `0039_memory_user_control.sql`.
- LIVE only (??): `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/`. None is needed by F-t3-033 as cited. Any new migration would follow 0045 in LIVE, or 0043 in WT. Numbering must be reconciled before merge.
- capability-core migrations: `0003_capabilities_registry.up.sql` (safety_policies). Latest migration not read.

## 7. Test and build commands

| Area | Command | Source |
|---|---|---|
| model-gateway (Rust) | `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib` (add `--all-targets` only where it works) | crate name `model-gateway` (`services/model-gateway/Cargo.toml:2`); MP/CLAUDE.md (`cargo test -q -p <service> --all-targets`); user auto-memory "Model Plane build and deploy" (two targets fail on Windows; use --lib) |
| execution-core (Rust) | `cd "apps/Model Plane/rust" && cargo test -p execution-core --lib` and `--test compaction_parity_contract` | crate `execution-core` (`Cargo.toml:2`); same memory note |
| session-core (Rust) | `cargo test -p session-core --lib`. DB-backed tests are `#[ignore]` and need `DATABASE_URL` to a disposable Postgres (e.g. memory_grpc.rs:1070-1072, dreaming.rs `disposable_pool` 2396) | crate `session-core` (`Cargo.toml:2`) |
| mp-events (Rust) | `cargo test -p mp-events` | crate `mp-events` (`rust/crates/mp-events/Cargo.toml:2`); workspace members `rust/Cargo.toml:4-17` |
| Rust workspace | `cd "apps/Model Plane/rust" && cargo test --workspace` | MP/CLAUDE.md |
| capability-core (Go) | `cd "apps/Model Plane/go/services/capability-core" && go test ./internal/skillsink/... ./internal/learning/... ./internal/api/... && go vet ./...` (internal/api has DB-backed tests; not run) | module `github.com/triodelab/model-plane/services/capability-core`, go 1.25.1 (`go.mod:1,3`); MP/CLAUDE.md |
| orchestrator-core (Go) | `cd "apps/Model Plane/go/services/orchestrator-core" && go test ./cmd/workflows/... ./cmd/activities/...` | module `github.com/triodelab/model-plane/services/orchestrator-core` (`go.mod:1`) |
| nats-provisioner (Go) | `cd "apps/Model Plane/go/services/nats-provisioner" && go test ./...` | `publishers_registry_test.go`; go.mod NOT read |
| Durable layer (DB) | `apps/Model Plane/scripts/verify-durable-layer.sh` (throwaway Postgres, all migrations, G7 guard at :51-66) | script header lines 1-5 |
| Proto | `buf generate` (regenerates checked-in go/gen and python _gen); `buf lint` has pre-existing debt | MP/CLAUDE.md |
| Frontend unit (FP) | `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/features/chat/components/chat-markdown.test.ts src/features/chat/lib/chat-export/ooxml.test.ts src/features/chat/lib/chat-export/docx.test.ts` | package.json scripts: "test" = node --test (apps/gateway scripts) && vitest run; "lint" = eslint .; "typecheck" = tsc -b. Repo CLAUDE.md |
| Frontend gate | `pnpm test`, `pnpm typecheck`, `pnpm lint` | package.json |
| Gateway (Rust, if routes change) | `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (under FP) | repo CLAUDE.md |
| Frontend baseline | Pre-existing typecheck, lint and vitest failures exist. Compare failing sets against HEAD, do not trust exit codes. | user auto-memory "Verevon v3 worktree verification" |
| Go race | Not available (cgo). Do not use `-race`. | user auto-memory "Model Plane build and deploy" |
| Compose (runtime) | `cd "apps/Model Plane" && ./scripts/compose.sh up -d` | MP/CLAUDE.md |
| Release | Use `scripts/release-artifact.sh` only | MP/CLAUDE.md |

CI workflow for MP was not read. Status: NOT DETERMINED.

## 8. Owners

| Area | Owner (plane, service) | Rule (cite) |
|---|---|---|
| Screening enforcement at the gateway (moderation, tool_loop, security_events, semantic_screening) | Model Plane, model-gateway | Repo CLAUDE.md: Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants, cost. master-ownership-matrix.md:125 (model-gateway: ZDR and cross-plane guards). moderation.rs:3-8 (policy in capability-core, enforcement at gateway) |
| Do-mode screening seam (execution-core provenance, runtime_loop) | Model Plane, execution-core | Repo CLAUDE.md: Model Plane owns execution loops. MP/CLAUDE.md Code Style: execution-core `execute_step_inner` is the deployed-agent loop and must stay separate from model-gateway `dispatch_tool` |
| Memory index and writes (session-core memory, dreaming, dream_extractor) | Model Plane, session-core | master-ownership-matrix.md:126; capability-ownership-matrix.md:57 and :110 (session-core owns the canonical memory index). master-ownership-matrix.md:41 (Model Plane writes memory) |
| Skill registry, skill CRUD, safety policies, skillsink | Model Plane, capability-core (Go) | master-ownership-matrix.md:130 (skills, safety, memory adapters); capability-ownership-matrix.md:59 (safety registries), :114 (skills registry is capability-core agent_skills; promotion writes into it; learning is a producer), :106 (no second registry implementation) |
| Skill promotion workflow | Model Plane, orchestrator-core | capability-ownership-matrix.md:114 |
| Security event subject and envelope (mp-events) | Model Plane, shared crate | mp-events/src/subjects.rs:114-117 (metadata and hashes only; ZDR discipline) |
| NATS publish grants | Model Plane deploy | deploy/nats.conf; nats-provisioner publishers_registry_test.go |
| Chat link rendering and export allowlist | Frontend Plane, verevonv3 (chat) | Repo CLAUDE.md: Frontend Plane owns Verevon v3 UI |
| Public memory and skill routes | Frontend Plane, gateway BFF | Repo CLAUDE.md: Frontend Plane owns same-origin gateway/BFF normalization. domains/memory.rs, agent_actions.rs |
| Audit and security event consumers | Control Plane (audit) | Repo CLAUDE.md: Control Plane owns audit. Consumer NOT DETERMINED |
| ZDR across content-persisting boundaries | Cross-plane rule | Repo CLAUDE.md: ZDR must propagate through any content-persisting boundary. MP/docs/ZDR.md (no screening row) |

## 9. Uncommitted-only (live tree)

Status key from `live-tree-status.txt`. Diff stats from `git --no-optional-locks diff --stat` in LIVE, run for every file below.

| Path (under MP unless shown) | Live status | One-line summary of live change |
|---|---|---|
| `rust/services/execution-core/src/runtime_loop/mod.rs` | M (+18) | Hunks at WT 318, 352, 377, 416, 441, 471, 638 and knowledge-search 1193, 1226. Shifts the seam to 811-815. No added line matches screen, scan, injection, policy or provenance. |
| `rust/services/execution-core/src/runtime_loop/agent.rs` | M (+207) | Hunks at 571-769 (run_agent), 1937 (run_rounds), 4566 (continuation_descriptor), tests 7185+. IndexMemory caller moves to 3373. |
| `rust/services/execution-core/src/grpc.rs` | M (+98) | Not read in detail. Adjacent. |
| `rust/services/model-gateway/src/tool_loop.rs` | M (+425) | Hunks at 345 (+24), 2231/2241, 2487/2513, a 43-line block at 2876-2918 replaced by 1 line (call site moves to 2914), 3349, 3431 (+15), 5180-5300, 5711-5782 (grounding labels, +73), 5940. Keyword check (screen, scan, injection, policy, provenance) was capped at 25 matches, and none of those 25 was an added line; the rest of the diff was not checked. |
| `rust/services/model-gateway/src/http_routes.rs` | M (+237) | Hunks from 824 (require_non_zdr_durable_mutation), plan and approval routes 1400-1756, browser routes 2032 and 2106. `/v1/skills` at 371-375 unchanged. |
| `rust/services/model-gateway/src/sse.rs` | M (+883) | Not read. Adjacent. No screening SSE event found in taxonomy doc. |
| `rust/services/model-gateway/src/sse_events.rs` | M (+203) | Not read. Adjacent. |
| `rust/services/session-core/src/memory_grpc.rs` | M (+123) | Import hunk at 5, changes at 493 (+7), 619 (+4), test module rewrites 981-1099. Index path lines shift +3. No added line matches screen, scan, injection or policy. |
| `rust/services/session-core/src/dreaming.rs` | M (+52) | dream_once +7 at 216; grounding pool 561-615; record_dream_run +15 at 1248; load_pending_messages +5 at 1278. Writers shift +22 to +42. |
| `rust/services/session-core/src/grpc.rs` | M (+861) | Not read in detail. upsert_agent_skill at LIVE 5075. Keyword check not run. |
| `rust/services/session-core/src/memory_dedup.rs` | M (+5) | Five added lines. Not read. |
| `rust/services/session-core/src/learning_events.rs` | M (+77) | Not read. Skill-learning trigger. |
| `proto/model_plane/v1/sessions.proto` | M (+42) | +11 at 196 and +31 at 1219. UpsertAgentSkillRequest moves 831 to 842. |
| `docs/ZDR.md` | M (+64) | Not read in full. No screening row in WT (grep); LIVE text not grepped. |
| `docs/sse-event-taxonomy.md` | M (+4) | Not read. |
| `FP/src/features/chat/components/ChatMessages.tsx` | M (+229) | Host of ChatMarkdown. Parser file chat-media-markdown.tsx is clean. |
| `FP/src/features/chat/components/ChatPage.tsx`, `use-chat-controller.ts` | M | Adjacent. Not read. |
| `rust/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/` | ?? (live only) | New migrations, adjacent to ZDR. Not cited by F-t3-033. |
| `rust/services/model-gateway/src/zdr_posture.rs`, `zdr_features.rs`, and zdr_* tests | ?? (live only) | Adjacent to the ZDR flag reaching tool_loop. Not read. |

Clean in LIVE (not in status list): moderation.rs, semantic_screening.rs, provenance.rs, security_events.rs, dream_extractor.rs, memory_control.rs, memory.proto, inference.proto, mp-events subjects.rs, deploy/nats.conf, publishers_registry_test.go, compaction_parity_contract.rs, registry_apis.go, skillsink, learning, watch.go, sessionreview, capability-core migration 0003, skill_promotion.go, quarantine.go, verify-durable-layer.sh, ooxml.ts, ooxml.test.ts, docx.ts, chat-media-markdown.tsx, chat-markdown.test.ts, gateway domains memory.rs and agent_actions.rs, ledger.md, master and capability ownership matrices, postmortem 0002.

## 10. NOT DETERMINED

1. Whether live (uncommitted) changes alter screening inputs in tool_loop.rs, session-core grpc.rs, sse.rs or sse_events.rs. Only hunk headers and a partial keyword check (tool_loop.rs, first 25 matches) were read.
2. Whether any production consumer of `mp.v1.security.*` exists outside the repo (Control Plane audit). No consumer found in repo.
3. Whether the publish gap for `mp.v1.security.*` is closed in any live NATS config outside `deploy/nats.conf` (for example nats-provisioner output). Not checked live.
4. Whether an injection false-positive evaluation set exists. Four MP docs matched a broad grep and were not read (section 2). `docs/skills/eval-harness.md` not read.
5. Whether `extract_memory_candidates` filters by role (AppendMessage path, grpc.rs:2631). Function body not read.
6. Whether skill writes come from other gRPC callers besides capability-core skillsink. Not exhaustively searched.
7. Whether quarantine.go writes skills through UpsertAgentSkill or SetAgentSkillEnabled. Comment only read.
8. Whether registry_apis.go:110 ("sole write path") or session-core grpc.rs:4929 (direct INSERT) is authoritative. Both exist in WT and LIVE; not resolved.
9. Whether browser_agent output reaches the chat tool loop (where the semantic pass runs). The browser seam (runtime_loop/mod.rs:252-254) is in execution-core; the chat path was not traced.
10. Whether a shared normaliser crate is acceptable under the ledger rule. provenance.rs:12-15 says the copies cannot share code "across the process boundary"; a build-time crate in the same workspace (as mp-events is shared) would not cross a process boundary. Decision not recorded in ledger (grep).
11. Whether the verevonv3 pnpm commands in section 7 run in this worktree. Dependencies and node_modules were not checked.
12. Frontend test and typecheck results. Not run. Pre-existing failures exist per user memory.
13. CI workflow for Model Plane (not read).
14. Whether ZDR turns reach `tool_loop.rs` with `zdr=true` in every path (live `zdr_posture.rs` is ??). Not traced.
15. Exact behaviour of the live `sessions.proto` additions (+11 at 196, +31 at 1219). Only hunk headers read.
