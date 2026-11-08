# WS23 Connected apps: code-map sweep (F-t3-010)

Mechanical, read-only sweep, 2026-10-15. Only this file was written.

- **WT** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean). Line numbers are WT unless marked LIVE.
- **LIVE** = `C:/dev/CoresSystem` (same HEAD plus uncommitted work). Status from `git --no-optional-locks status --porcelain` and `diff --stat` run 2026-10-15, cross-checked against `.plan-work/live-tree-status.txt` (snapshot 2026-10-14 20:56).
- Every path below exists in WT and in LIVE. None is deleted (D) in LIVE.
- **Card** = `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` lines 484-498 (anchor 484, heading 485, next anchor F-t3-011 at 500). Card path:line citations appear only in "Verevon today" (496) and "How to build it" (498).
- Other F-t3-010 locations: 9.1 row 1560 (under `#### Wave W2`, line 1551); Priority row 1828 (Appendix B, W2); Appendix C reference check 1999-2006 (partial, medium confidence); no Appendix D entry; open question "F12" at 1734 (9.4), which is plan row F12.
- **Plan** = `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (tracked, clean). Rows: F2-F5 at 126-129 (Wave 1); F6 160, F8 161, F9 162, F10 163 (Wave 2); F7 188, F13 190 (Wave 3); G7-G10 241-244 (Wave 5); F12 268 (Wave 6, "do not start"); owner decisions OD10 462, OD11 467, OD12 472. The plan's wave numbers are not the audit's "W2".

## 0. Verdict

Most card claims hold. Corrections and new findings:

1. **Gating claim is partial.** Catalogued list reads (`mail.messages`, `gmail.messages`, `drive.files`, `calendar.events`) return `Some(false)` from `operation_is_write`, so they are NOT approval-gated under ask today. Only uncatalogued operations (`*.messages.get`, `*.file.content`, `profile`, `sharepoint.*`) return `None`, and `None` is gated (`permission/mod.rs:215`). Tests pin both sides: `integration_tools.rs:1009-1015` (Some(false)) and `1051-1056` (None).
2. **Wrong citation.** `source_validation.rs:11-23` holds scope-notice constants, not tool classes. ORG_CLASS_TOOLS is at `tool_loop.rs:4228-4237`.
3. **Design conflict.** The card says to add the new tool to ORG_CLASS_TOOLS. The doc comment at `tool_loop.rs:4221-4227` says connected-service tools sit in neither WEB_CLASS_TOOLS nor ORG_CLASS_TOOLS. Needs a ruling.
4. **Planned tool name is absent.** Plan line 166 names the F6 tool `read_provider_action`. No such symbol exists in WT or LIVE. Existing pieces: `list_provider_actions` (read, ungated; test at `permission/mod.rs:374-375`); `execute_provider_action` (classified by arguments); client method `read_action_json` (`integration_tools.rs:494`), already used for write read-back at `postcondition.rs:873` and `920`.
5. **Not in card: audit tagging.** `data_category()` (`runtime_loop/agent.rs:314-327`) has no arm for provider tools, so `execute_provider_action` and `list_provider_actions` fall through to `public_non_personal` (line 325). Connector content is personal data. F8 must add arms. Same in LIVE.
6. **Not in card: picker bypasses action contracts.** The composer calls `browseConnection` and `fetchConnectionContent` directly (`integrations-client.ts:376-408`). `src/shared/actions/action-registry.ts` has integrations connect and sync contracts (929-951) but no browse or content contract. Root CLAUDE.md says a contract comes first.
7. **Not in card: F8 dependencies are live-only (`??`).** ZDR refusal and retention contracts exist only in LIVE: `model-gateway/src/zdr_posture.rs`, `mp-contracts/src/zdr_refusal.rs`, `mp-contracts/src/retention.rs`, `apps/Model Plane/api/contracts/zdr-refusals.json`, `session-core/src/retention_receipt.rs`, and frontend `src/shared/api/zdr-refusals.ts`.
8. **Documentation drift.** (a) `integration_tools.rs:13-22` says `execute_provider_action` is "listed in permission::is_risky_tool"; the code classifies it by argument first (`is_risky_call`, `permission/mod.rs:195-200`), and the name entry at line 260 is a fallback. (b) CATALOG text at `integration_tools.rs:102` and `111` says "No server-side query/search filter is implemented yet", but `mail_read.go` implements query, from, since, until, folder, unread and pageToken. (c) `integration_tools.rs:194-196` names `INTEGRATION_COREV2_INTERNAL_KEY`, but `from_env` reads `INGESTION_SERVICE_API_KEY` (line 214). Header wiring was not traced.
9. **LIVE drift.** Cited files are modified in LIVE: `tool_loop.rs` (M, 425 lines changed), `runtime_loop/agent.rs` (M, 207), `runtime_loop/mod.rs` (M, +18), `DashboardComposer.tsx` (M, +7), `sse_events.rs` (M, +203), `sse.rs` (M, +883). Cited lines shift +2 to +39. Keyword check on the diffs found no provider-action or connector hunk.

## 1. Citation verification (card F-t3-010)

| card id | cited path:lines | claim (short) | WT verdict | LIVE status | corrected path:lines (WT) |
|---|---|---|---|---|---|
| F-t3-010 | `apps/Ingestion Plane/integration-corev2/internal/actions/service.go:195-310` | dispatches mail, drive, calendar reads | MATCH. Default arm 194-196; Microsoft switch 203-234; Google switch 276-307 | clean | same |
| F-t3-010 | `mail_read.go:1-70` (= `internal/actions/mail_read.go`) | param set; live proof unverified | MATCH. Param doc 18-42; "not verified against a live provider yet ... task F10" at 44-50; F6 note 33-34 | clean | 18-50 (doc); 52-75 consts; 126-160 parseMailQuery |
| F-t3-010 | `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:150-200` | picker attaches extracted text | MATCH at type level. ComposerAttachmentSource 173-179; ComposerFile 181-202 (extractedText 193, sourceItemId 197, connectionId 199). Picker call sites 5323-5463 | M (+7; types at 175-204 LIVE) | 173-202 (types); 5323-5463 (call sites) |
| F-t3-010 | `execution-core/src/integration_tools.rs:1-160` | catalogue lacks search filter and .get/content ops | MATCH. CATALOG 71-150 (google 98-106, microsoft 107-115); stale text at 102 and 111; no .get or content entries | clean | CATALOG 71-150 |
| F-t3-010 | `integration_tools.rs:744-761` | operation_is_write returns None if uncatalogued | MATCH, drift +3. Fn 747-761; doc 735-746 | clean | 735-761 |
| F-t3-010 | `execution-core/src/permission/mod.rs:168-225` | unknown operation treated as risky | MATCH. EXECUTE const 166; is_risky_call 195-200; provider_action_is_risky 206-218; name fallback 253-260 | clean | 166-218 (+ fallback 253-260) |
| F-t3-010 | `model-gateway/src/tool_loop.rs:3580-3700` | inline Ask loop offers only Verevon-native reads | MATCH for dispatch arms: inbox 3586-3614; social 3615-3651; knowledge_list_documents 3652-3674; MCP denied 3675-3682; result_query 3695-3711. No provider-action names in tool_loop.rs (WT or LIVE). These are dispatch_tool arms (dispatch_tool at 2406), not the offered list (builtin_tool_defs at 4273) | M (+425 lines; arms +7 LIVE) | 3583-3711; dispatch_tool 2406 |
| F-t3-010 | `model-gateway/src/source_validation.rs:11-23` | ORG_CLASS_TOOLS "org class" | ABSENT (wrong file). Lines 11, 19, 23 are CONVERSATION, KNOWLEDGE_ONLY and WEB_ONLY scope-notice constants | clean | tool_loop.rs 4228-4237 (ORG_CLASS_TOOLS); 4246-4253 (tool_allowed_for_retrieval_scope); LIVE 4241-4250 and 4259-4266 |
| F-t3-010 | (no line) "add the tool to ORG_CLASS_TOOLS" | web-only turn excludes it | CONFLICT. Doc comment 4221-4227: connected-service tools are in neither bucket | M file | decision needed |
| F-t3-010 | (no line) "record_tool_step persists truncated output" | non-ZDR step records truncated | MATCH. record_tool_step 4361-4431. Non-ZDR branch 4394-4401 persists `truncate(output, OUTPUT_TRUNCATE)`, OUTPUT_TRUNCATE = 2000 (line 309). ZDR branch 4379-4393 persists `[content_redacted=true result=...]` only | M (agent.rs +207; body identical; LIVE 4368-4438) | 4361-4431 |
| F-t3-010 | (symbol) IntegrationActionsClient | reuse the client | MATCH. Struct 153-166; impl 193; from_env 201; list_provider_actions 257; execute_action 347; read_action_json 494 | clean | same |
| F-t3-010 | (symbol) is_risky_call | reuse the classifier | MATCH. `permission/mod.rs:195`; used by evaluate at line 65 | clean | same |
| F-t3-010 | (text) "Writes stay out of Ask mode, which has no approval gate" | Ask mode ungated | CONSISTENT. Inline loop denies MCP (tool_loop.rs 3675-3682); no approval path in dispatch_tool | M | n/a |
| F-t3-010 | (text) plan F2-F13, F6, F8, F10, G7-G10, OD10-OD12 | plan references | MATCH. Plan rows 126-129, 160-163, 188-190, 241-244; OD 462, 467, 472. Audit F12 question (1734) = plan F12 (268) | clean | same |
| F-t3-010 | (text) "Personal mailbox content must never surface in a shared-Space thread" | Space visibility rule | NOT DETERMINED. No Space-path check was performed | n/a | n/a |

### 1a. Catalogue versus implementation (input to F6 catalogue refresh)

"Implemented" = case arm in `service.go`. "Catalogued" = CATALOG entry in `integration_tools.rs`. "Result today" = `operation_is_write`. Gated under ask = result is None or true.

| provider | operation (service.go arm lines, WT) | implemented | catalogued (line) | result today | gated under ask |
|---|---|---|---|---|---|
| microsoft | mail.messages (211-213) | yes | yes (111), read | Some(false) | no |
| microsoft | mail.messages.get (214-215) | yes | no | None | yes |
| microsoft | calendar.events (208-210) | yes | yes (113), read | Some(false) | no |
| microsoft | drive.files (216-218) | yes | yes (112), read | Some(false) | no |
| microsoft | drive.file.content (219-220) | yes | no | None | yes |
| microsoft | sharepoint.sites, sharepoint.drives (221-224) | yes | no | None | yes |
| microsoft | profile (206-207) | yes | no | None | yes |
| microsoft | mail.send (225-230) | yes | yes (110), write | Some(true) | yes |
| google | gmail.messages (281-285) | yes | yes (102), read | Some(false) | no |
| google | gmail.messages.get (286-287) | yes | no | None | yes |
| google | calendar.events (294-296) | yes | yes (103), read | Some(false) | no |
| google | drive.files (297-299) | yes | yes (104), read | Some(false) | no |
| google | drive.file.content (300-303) | yes | no | None | yes |
| google | profile (279-280) | yes | no | None | yes |
| google | gmail.send (288-293) | yes | yes (101), write | Some(true) | yes |

Browse allowlist (gateway, `browse.rs`) covers the four list reads and seven content reads above and nothing else. Frontend types match it (`integrations-client.ts:323-338`).

## 2. Affected-file map (depth 2)

Live status key: clean = no change in LIVE; M = modified; ?? = untracked, LIVE only.

### 2a. Model Plane, execution-core (Rust)

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `apps/Model Plane/rust/services/execution-core/src/integration_tools.rs` | provider-action client, CATALOG, operation_is_write, token mint | 1-32 doc; 71-150 CATALOG; 153-166 client; 201-227 from_env; 257 list_provider_actions; 347 execute_action; 494 read_action_json; 542-665 token mint and resolve; 747-761 operation_is_write | clean | 19 test attrs. In-file: renders_connections_with_catalog_operations 892; operation_is_write_classifies_reads_and_writes 965; phase6_fixed_operation_strings_are_catalogued 987; mailread_operations_are_catalogued_as_reads_never_writes 1003; operation_is_write_returns_none_for_unknown_and_empty 1051; client tests 1081-1382 (mint and resolve token tests; list_provider_actions_succeeds_for_the_runs_own_user...; read_action_json_succeeds_for_the_runs_own_user...; two_concurrent_runs_...) |
| `.../execution-core/src/permission/mod.rs` | risk classifier, rung, enforcement | 65 evaluate uses is_risky_call; 100 rung_required_for; 124 check_autonomy_rung; 166 const; 181 is_restricted_context_write; 195-218 is_risky_call and provider classifier; 229-266 is_risky_tool list (260 execute_provider_action; 266 browser_agent) | clean | 20 test attrs; provider asserts 368-375 (execute_provider_action gated; list_provider_actions not gated) |
| `.../execution-core/src/runtime_loop/agent.rs` | tool defs, data_category, record_tool_step | 309 OUTPUT_TRUNCATE; 314-327 data_category; 3646-3651 discoverable-args test; 4109 offered_tool_defs; 4167 and 4172 list and execute ToolDefinitions; 4361-4431 record_tool_step; call sites 2048, 7077, 7087, 7135, 7145 | M (+207; shift +7) | offered_tool_defs_has_no_duplicate_tool_names 11577; content_redacted=true assertions at LIVE 7118 and 7170 (names not captured) |
| `.../execution-core/src/runtime_loop/mod.rs` | run-step plumbing; Z-18 `waived` flag | +18 | M (+18) | not read |
| `.../execution-core/src/postcondition.rs` | write read-back via read_action_json | 873, 920 | clean | not read |
| `.../execution-core/src/approval_delivery_worker.rs` | resumable approvals for execute_provider_action | 120 PROVIDER_ACTION_TOOL_NAME; 470 match arm | clean | test at 1424, 1534 |
| `.../execution-core/src/capability_policy.rs` | tool to capability-id mapping | 830-831 (list -> cap.tool.provider.read; execute -> cap.tool.provider.execute) | clean | not read |
| `.../execution-core/src/attestation.rs` | write-attestation signer from env | 154-166 | clean | not read |
| `.../execution-core/src/grpc.rs` | run and step RPC handlers | not range-mapped | M (+98) | not read |
| `.../execution-core/tests/autonomy_ladder_contract.rs` | contract: rung from call arguments, not catalogue | 68, 85, 108, 135 | clean | this file (4 tests) |

### 2b. Model Plane, model-gateway (Rust)

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `.../model-gateway/src/tool_loop.rs` | inline Ask loop: dispatch_tool, scope filter, builtin defs | dispatch_tool 2406; inline arms 3583-3711; WEB_CLASS_TOOLS 4220; ORG_CLASS_TOOLS 4228-4237; tool_allowed_for_retrieval_scope 4246-4253; builtin_tool_defs 4273; MCP denial 3675-3682 | M (+425) | doc comments on org and web bucket tests at LIVE 10795 and 10829 (names not captured) |
| `.../model-gateway/src/source_validation.rs` | scope-notice constants (not tool classes) | 11, 19, 23 | clean | not read |
| `.../model-gateway/src/tool_retry.rs` | retry gate mirrors is_risky_call | 16 (doc) | clean | not read |
| `.../model-gateway/src/sse_events.rs` | ChatEvent citation and source_scope_applied frames | Citation 132-191 (origin 190; source_group_id 144; number 160; claim_id 168); SourceScopeApplied 199; family map 356-359; names 405-406 | M (+203; Citation at LIVE 171, origin 229) | citation anchoring tests near LIVE 1332-1345 (names not captured) |
| `.../model-gateway/src/sse.rs` | SSE assembly; citations and grounding | not range-mapped | M (+883) | not read |
| `.../model-gateway/src/zdr_precheck.rs` | ZDR precheck | not range-mapped | M | not read |
| `.../model-gateway/src/zdr_posture.rs` | ZDR continuation gate (used by F-t3-011) | n/a | ?? | not read |

### 2c. Model Plane, contracts, session, capability, docs

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `apps/Model Plane/rust/crates/mp-contracts/src/tool_arguments.rs` | argument grounding; connection_id discoverable rule | 405 (connection_id discoverable), 815 (execute_provider_action listed) | clean | not read |
| `.../mp-contracts/src/autonomy.rs` | AutonomyRung rank and permits | 45 (rank) | clean | not read |
| `.../mp-contracts/src/zdr_refusal.rs`, `retention.rs` | ZDR refusal and retention contracts | n/a | ?? | not read |
| `.../session-core/src/approval_delivery.rs` | approval payload naming execute_provider_action | 1132 | clean | not read |
| `.../session-core/migrations/` | run and step persistence | latest tracked 0043 | clean; 0044, 0045 and `revert/` are ?? | not read |
| `.../session-core/src/retention_receipt.rs` | retention receipt | n/a | ?? | not read |
| `apps/Model Plane/go/services/capability-core/migrations/0008_execution_dispatch_capabilities.up.sql` | seeds cap.tool.provider.read and .execute | 21-22 | clean | not read |
| `apps/Model Plane/docs/decisions/ledger.md` | settled decisions | 1762-1772 (rung per call); 2240-2245 (discoverable; execute_provider_action excluded from grounding list; a test pins it); 2306-2309 (never add connection_id to grounding table) | clean | n/a |
| `apps/Model Plane/scripts/seed_tool_knowledge.sql` | org tool-memory seeds for list and execute_provider_action | 236-241 | clean | n/a |
| `apps/Model Plane/deploy/docker-compose.yml` | execution-core to integration-corev2 wiring | 1053, 1210-1211 | clean | n/a |
| `apps/Model Plane/feature.md` | doc: is_risky_call gate list | 32 | clean | n/a |
| `apps/Model Plane/docs/sse-event-taxonomy.md` | generated SSE taxonomy, CI-checked | n/a | M (+4) | CI sse-taxonomy.yml:45 |

### 2d. Ingestion Plane, integration-corev2 (Go; module `github.com/triodelab/integration-corev2`)

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `apps/Ingestion Plane/integration-corev2/internal/actions/service.go` | provider operation switch | 194-196 default; 203-234 Microsoft; 276-307 Google; 198 reconnectableError | clean | service_test.go: 44 `func Test`, e.g. TestExecuteMicrosoftMailSendTranslatesVerevonEnvelope (19), TestExecuteGoogleGmailSendTranslatesVerevonEnvelope (61) |
| `.../actions/mail_read.go` | Outlook and Gmail list and get; query parsing; page-token guard; header fan-out | 18-50 doc; 52-75 consts; 126-160 parseMailQuery; 330-374 Outlook list and get; 380-398 graphMailListURL; 480-507 graphMailPageURL; 544-584 Gmail list; 586-620 Gmail get; 665-699 header fan-out (cap 25, concurrency 5); 740-765 attachments | clean | mail_read_test.go: 29 `func Test`, e.g. TestMailReadOutlookSearchComposesKQLAndNeverCombinesFilterOrOrderby (119), TestMailReadGmailHeadersFanOutIsBoundedToFiveAtATime (813) |
| `.../actions/calendar_read.go` | Outlook calendarView and Google events windows | not read | clean | calendar_read_test.go: 19 `func Test` |
| `.../actions/drive_read.go` | OneDrive, SharePoint and Google Drive list and content | not read | clean | drive_read_test.go: 39 `func Test` |
| `.../actions/provider_auth_error.go` | typed provider credential rejection (reconnect) | not read | clean | provider_auth_error_test.go: 6 `func Test` |
| `.../internal/api/server.go` | action routes; capability and approval checks | routes 534 (`/api/v1/connections/:id/actions`), 546 (`/api/v1/actions/execute`); executeConnectionAction 3051; requireActionCapability 3312; verifyWriteAttestation 3328; requiredCapabilityForOperation 3419 (mail.messages to mail.read, sensitive; calendar.events to calendar.read, sensitive; drive.files to sharepoint.read, not sensitive); actionRequiresApproval 3621 | M (+30 LIVE; ZDR acknowledgement routes, BearerOnly wrapper, org purge audit; action routes not changed) | not enumerated (NOT DETERMINED) |
| `.../internal/api/zdr_acknowledgement.go` (+ `_test.go`) | ZDR acknowledgement routes | n/a | ?? | zdr_acknowledgement_test.go (??) |
| `.../internal/auth/middleware.go` | bearer and internal auth | n/a | M (+93, -37) | bearer_only_test.go (??) |
| `.../internal/controlplane/clients.go` | Control Plane client; org ZDR posture | n/a | M (+32) | org_zdr_posture_test.go (??) |
| `.../internal/store/zdr.go`, `postgres_zdr.go`, `memory_zdr.go` | ZDR capability record and acknowledgement store | n/a | M (+73), M, M | postgres_zdr_test.go (M); memory_zdr_lifecycle_test.go (??); storetest/zdr_lifecycle_contract.go (??) |
| `.../internal/zdr/assessor.go`; `wording.go`; `texts/` | ZDR assessment and wording | n/a | M (+10); ??; ?? | wording_test.go (??) |
| `.../internal/oauth/service.go` | OAuth token handling | n/a | M (+4) | not read |
| `.../internal/db/migrations/` | integration-corev2 Postgres migrations | 0001_init.sql to 0013_model_connection_zdr.sql (13 files) | clean | n/a |

### 2e. Frontend Plane, verevonv3 gateway (Rust)

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations.rs` | route table | browse 81-84; content 85-88 | clean | n/a |
| `.../gateway/src/domains/integrations/browse.rs` | read-only allowlists; passthrough to integration-corev2 | READ_ONLY_BROWSE_OPS 24-29; READ_ONLY_CONTENT_OPS 33-41; reject_unless_allowlisted 47; browse_connection 81; content_connection 102; upstream `/api/v1/connections/{id}/actions` 92-93 | clean | 6 tests: rejects_an_operation_outside_the_browse_allowlist_before_any_upstream_call (143); rejects_a_browse_op_on_the_content_route_and_vice_versa (154); accepts_every_allowlisted_browse_and_content_operation (173); rejects_a_missing_or_blank_operation (191); forwards_an_allowlisted_browse_operation... (201); a_rejected_operation_never_touches_the_upstream_mock (246) |

### 2f. Frontend Plane, verevonv3 src (TS and TSX)

| path | role | lines (WT) | LIVE | tests |
|---|---|---|---|---|
| `src/shared/api/integrations-client.ts` | REST client: list, browse, content | 156-163 listConnections; 323-338 operation unions; 347-354 ConnectorPickerTab; 362-374 listConnectorPickerTabs; 376-391 browseConnection; 393-408 fetchConnectionContent | clean | integrations-client.test.ts: 6 cases |
| `src/features/dashboard/home/DashboardComposer.tsx` | composer picker: tabs, search, content chips | 115-119 imports; 173-202 types; 3809 connectionId into payload; 5323-5373 content fetch and chip build; 5411; 5427 tabs resource; 5463 browseConnection | M (+7; picker lines shift +7) | DashboardComposer.connector-attach.test.tsx (clean): describe at 88 and 111, 182; it at 89, 112, 142, 153, 183, 207 |
| `src/features/chat/lib/prepare-chat-attachments.ts` | attachment preparation | not read | clean | prepare-chat-attachments.test.ts (clean) |
| `src/features/chat/components/chat-normalizers.ts` | attachment and chat normalisation | not read | M (+39) | chat-normalizers.connector-attach.test.ts (clean) |
| `src/features/chat/components/chat-types.ts` | Citation origin: web or knowledge (137); sourceGroupId (105) | 105, 137 | M (+7) | n/a |
| `src/features/chat/components/ChatMessages.tsx` | renders citations and scope badge | not read | M (+229) | n/a |
| `src/features/chat/components/ChatPage.tsx`; `use-chat-controller.ts` | chat page and controller | not read | M (+13); M (+4) | n/a |
| `src/shared/api/chat-client.ts`; `src/shared/api/sse.ts` | SSE client and parser | not read | M (+53); M (+4) | n/a |
| `src/shared/actions/action-registry.ts` | action contracts; integrations connect and sync at 929-951; no browse or content entry | 929-951 | clean | action-registry.test.ts (clean) |
| `src/shared/actions/agent-tools.ts` | agent tool metadata; kind union includes 'connector' | 9 | clean | n/a |
| `src/shared/actions/chat-source-scope.ts` | RetrievalScope: auto, knowledge_only, web_only | 18 | clean | n/a |
| `src/shared/integrations/connect-bundles.ts` | consent bundles per source (G10) | 120 connectBundlesForSources; 153 instagramInboxConnectionRequest | clean | n/a |
| `src/shared/api/zdr-refusals.ts` | ZDR refusal mapping | n/a | ?? | n/a |

### 2g. Owners, plan and frozen contract (docs)

| path | role | lines (WT) | LIVE | notes |
|---|---|---|---|---|
| `docs/actions-surface-operations.md` (repo root) | FROZEN operation and capability contract | section 3 at 84-95 (unmapped ops unguarded; writes need approvalId or approvalRef); per-provider tables from about 106 (mail.messages row 115; gmail.messages row 134) | clean | its server.go line refs (390, 402, 1054, 2299, 2337, 2355, 2371, 2535, 2603) are stale vs WT |
| `apps/master-ownership-matrix.md` | owners | 37-47 decision questions; 49-90 plane matrix (rows 81, 85, 90 relevant); 157 no direct DB crossing | clean | n/a |
| `apps/Model Plane/docs/capability-ownership-matrix.md` | one-owner rules | 3 (one owner per capability); 59, 60, 63 | clean | n/a |
| `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (path in Plan above) | plan rows and owner decisions | 126-129, 160-166, 188-190, 241-244, 268, 462-472 | clean | n/a |

## 3. Related contracts

### 3a. Proto (`apps/Model Plane/proto/model_plane/v1/`)

- `execution.proto`: RunAgentRequest 76; ExecuteStepRequest 268; ExecuteStepResponse 321; comment naming `permission::is_risky_call` at 129. Clean.
- `sessions.proto`: CompleteStepRequest 622 (step persistence). M in LIVE (+42).
- `runs.proto`: enum AutonomyRung 22 (read_only, workspace_write, danger_full_access per ledger 1762-1772). Clean.
- `inference.proto`: message ToolDefinition 178. Clean.
- `capabilities.proto`: header 10 ("connector normalization"); kind "connector" at 34 and 69. Clean.
- `verified_outcome.proto`: 61 (effect kind "provider_action:pages.post"). Clean.
- `gateway.proto`: 242 (include list containing citations). Clean.
- No proto message carries a connector-origin citation.

### 3b. SQL migrations (latest names)

- integration-corev2 `internal/db/migrations/`: latest `0013_model_connection_zdr.sql` (13 files, clean).
- session-core `migrations/` WT: latest `0043_audit_outbox_requeue.sql` (43 entries). LIVE adds `0044_run_retention_receipt.sql` (??), `0045_zdr_waived_runs_org_index.sql` (??) and `revert/` (??).
- capability-core `migrations/` WT: latest `0015_background_process_capability.up.sql` and `.down.sql`. Provider capability seeds are in `0008_execution_dispatch_capabilities.up.sql` lines 21-22.
- No connection-record table exists in Model Plane; connections live in integration-corev2 (per the execution-core module doc at `integration_tools.rs:4-11`).

### 3c. Config and env

- `INTEGRATION_COREV2_URL`: read at `integration_tools.rs:202`; default `http://host.docker.internal:3026` (line 48).
- `AUTH_CORE_URL`: read at 206; default `:3011` (line 49). Also read at `approval_delivery_worker.rs:187` and `capability_policy.rs:624`.
- `INGESTION_SERVICE_ID` (210) and `INGESTION_SERVICE_API_KEY` (214): Auth Core service identity read by `from_env`. The doc comment at 194-196 names `INTEGRATION_COREV2_INTERNAL_KEY` instead (drift).
- `EXECUTION_CORE_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY` and `..._KEY_ID`: `attestation.rs:163` and `166`. Missing key makes writes fail closed (doc at `integration_tools.rs:160-165`).
- `deploy/docker-compose.yml:1053` and `1210-1211` (execution-core uses integration-corev2 for the two provider tools).

### 3d. SSE event types (`model-gateway/src/sse_events.rs`, WT numbering)

- `citation`: variant 132-191. Origin is `web` or `knowledge` only (field 190). Grouping key `source_group_id` (144). Frontend mirror in `chat-types.ts` (origin at 137).
- `source_scope_applied`: variant 199; name at 406.
- No connector origin or connector source group exists (LIVE grep for "connector" in sse_events.rs: 0 hits).
- CI: `.github/workflows/sse-taxonomy.yml:45` runs `gen-sse-event-taxonomy.py --check` against `docs/sse-event-taxonomy.md`.

### 3e. Gateway routes

- BFF (verevonv3 gateway): POST `/api/v1/integrations/connections/{id}/browse` (`integrations.rs:81-84`) to `browse.rs:81`, which forwards to integration-corev2 `{integration_core_url}/api/v1/connections/{id}/actions` (`browse.rs:92-93`).
- POST `/api/v1/integrations/connections/{id}/content` (`integrations.rs:85-88`) to `browse.rs:102`.
- Other integration routes used by the frontend client: `/api/v1/integrations/connections` (`integrations-client.ts:157`), connect-sessions, sync, inbox-sync, sync-jobs.
- integration-corev2: POST `/api/v1/connections/:id/actions` (server.go 534 WT, 539 LIVE); POST `/api/v1/actions/execute` (546, 551).

### 3f. Action contracts (`verevonv3/src/shared/actions`)

- `action-registry.ts` 929-951: integrations connect-session, disconnect, trigger-sync, trigger-inbox-sync, extend-inbox-history input schemas. No browse, content or picker contract.
- `agent-tools.ts` 9: kind union includes `'connector'`.
- `chat-source-scope.ts` 18: RetrievalScope is auto, knowledge_only or web_only. No connector scope.

### 3g. Frozen operations contract

- `docs/actions-surface-operations.md` (repo root). Per-operation table gives capability, sensitive and approval columns. `mail.messages` maps to `mail.read` (sensitive, no approval); `gmail.messages` to `gmail.read`; `calendar.events` to `calendar.read`. Its service.go line column is stale (for example mail.messages 92 vs WT 211). Its server.go refs are stale too: requireActionCapability cited at 2332 (WT 3312), requiredCapabilityForOperation at 2371 (WT 3419), actionRequiresApproval at 2535 (WT 3621).
- Section 3 (lines 80-95): operations with no capability mapping return nil, with no capability and no approval check (section 3, item 2).
- Section 6 (lines 298-324), relevant to F6 and F8:
  - Item 3: a connection with an empty capabilities list passes all non-sensitive operations (server.go:2337 per doc). In WT, `drive.files` and `drive.file.content` map to `sharepoint.read` with sensitive=false (`server.go:3419-3436`), so an empty-capability connection can read Drive files, while mail and calendar reads (sensitive=true) cannot.
  - Item 5: the capability layer lowercases operations, but `service.go` matches case-sensitively for Microsoft and Google. Mixed case passes the capability layer and fails in the executor. Send lowercase only. The classifier `operation_is_write` is case-insensitive (`integration_tools.rs:755`), so this does not change gating.
  - Items 1, 2 and 4 concern other providers (threads, notion, shopify, meta ads) and do not change the Google or Microsoft read path.

### 3h. Capability seeds

- capability-core `0008_execution_dispatch_capabilities.up.sql` lines 21-22: `cap.tool.provider.read` for `list_provider_actions` (low, bounded); `cap.tool.provider.execute` for `execute_provider_action` (high, variable).
- No seed exists for a read-only mail, drive or calendar tool (F6 would need one).

## 4. Test and build commands

| scope | command | source |
|---|---|---|
| Rust workspace | members include `services/model-gateway`, `services/execution-core`, `crates/mp-contracts` (workspace at `apps/Model Plane/rust/Cargo.toml`) | Cargo.toml:3-19 |
| execution-core crate | `cargo test -q -p execution-core --all-targets` (package name `execution-core`) | `rust/services/execution-core/Cargo.toml:2`; Model Plane CLAUDE.md Testing |
| model-gateway crate | `cargo test -q -p model-gateway --all-targets` (package name `model-gateway`) | `rust/services/model-gateway/Cargo.toml:2` |
| mp-contracts crate | `cargo test -q -p mp-contracts` (package name `mp-contracts`) | `rust/crates/mp-contracts/Cargo.toml:2` |
| CI Rust | `cargo fmt --all -- --check` (line 99); `cargo check --workspace` (105); `cargo test --workspace --no-fail-fast` (119) | `.github/workflows/model-plane-rust-ci.yml` |
| SSE taxonomy CI | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` | `.github/workflows/sse-taxonomy.yml:45` |
| Go integration-corev2 | `cd "apps/Ingestion Plane/integration-corev2"`, then `go test ./internal/actions/... ./internal/api/... ./internal/store/... ./internal/controlplane/...` and `go vet ./...` (module `github.com/triodelab/integration-corev2`, go 1.25.0) | `go.mod:1-3`; Model Plane and root CLAUDE.md "go test ./... and go vet ./..." |
| Go CI | none. No workflow under `.github/workflows` mentions integration-corev2 | grep, 2026-10-15 |
| Ingestion Makefile | no unit-test target for integration-corev2. `test-endpoints` (line 131) is a curl readiness check | `apps/Ingestion Plane/Makefile:131` |
| Gateway Rust (verevonv3) | `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` run from `apps/Frontend Plane/verevonv3` (add filter `browse` for this card). Cargo.toml itself not opened | verevonv3 CLAUDE.md build section |
| Frontend | `pnpm test` (node --test on four gateway scripts, then `vitest run`); `pnpm typecheck` (`tsc -b`); `pnpm lint` (`eslint .`); `pnpm verify` (lint, typecheck, test, build) | `apps/Frontend Plane/verevonv3/package.json:11-18` |
| Frontend targeted | `pnpm vitest run src/features/dashboard/home/DashboardComposer.connector-attach.test.tsx src/shared/api/integrations-client.test.ts src/features/chat/components/chat-normalizers.connector-attach.test.ts` | files present, clean |
| Frontend CI | `pnpm install --frozen-lockfile` (line 180); `pnpm verify` (193); `cargo clippy --all-targets -- -D warnings` (138); `cargo test` (141) | `.github/workflows/verevonv3-ci.yml` |
| Live proof | Terra route, no Do mode, connector chain through the picker (plan F10, line 163). Manual; no automated command | CHAT_GAP plan |

User memory notes (not re-verified in this sweep): two Model Plane cargo targets fail on Windows pre-existing (use `--lib`); the verevonv3 dev server on 5173 is the main checkout (use a second server for a worktree); verevonv3 typecheck, lint and vitest have pre-existing failures (diff failing sets against HEAD); Go `-race` needs cgo, which is unavailable.

## 5. Owners

Rules:
- Master matrix section 0 (`apps/master-ownership-matrix.md:37-47`): "choose tools", "act as an agent" to Model Plane; "store, retrieve durable knowledge" to Data Plane; UX to App Shell; durable workflow, registry, policy, grants and billing to Go.
- Master matrix section 1: row 81 (tools and toolsets, Model Plane owner); row 85 (MCP registry: Model Plane owns use, capability-core-go owns registry); row 90 (own-connection ZDR: integration-corev2 owns capability record and acknowledgement ledger; Auth Core, the Control Plane TypeScript service, is the only issuer; Model Plane owns enforcement; a waived run retrieves as non-ZDR with restricted documents filtered).
- Capability ownership matrix (`apps/Model Plane/docs/capability-ownership-matrix.md`): line 3, exactly one canonical owner per capability; row 59, tool and MCP registries to capability-core (gateway registry is a cache); row 60, policy and permissions to capability-core `policy/engine.go`, with execution-core `permission/` as enforcement; row 63, provider routing to inference-core.
- Model Plane CLAUDE.md (Code Style): model-gateway `dispatch_tool` (read-only router) and execution-core `execute_step_inner` (permission pipeline) must stay separate; the refusal boundary is the authority boundary.
- Root CLAUDE.md: Control Plane owns identity, users, orgs, billing, sessions, audit, quotas and entitlements; provider actions go through integration-corev2's actions surface (frozen contract `docs/actions-surface-operations.md`); Frontend Plane owns the UI and same-origin BFF, which must not leak raw OAuth tokens or forged scoping headers; ZDR must propagate through every content-persisting boundary.

Area to owner:

| area | owner | basis |
|---|---|---|
| provider reads and writes, connections, OAuth tokens, server-side action catalogue | Ingestion Plane, integration-corev2 (Go) | root CLAUDE.md; `integration_tools.rs:4-11`; master row 90 |
| tool choice, argument handling, citation producers, scoping of the agent loop | Model Plane (execution-core for the execute loop; model-gateway for inline Ask) | master section 0 row 41; Model Plane CLAUDE.md two-loop rule |
| approval and permission enforcement for provider calls | execution-core (enforcement); policy source is capability-core | capability matrix row 60 |
| provider capability ids `cap.tool.provider.*` | capability-core (Go) | capability matrix row 59; migration 0008 |
| ZDR for connector content (capability record, acknowledgement, waiver) | integration-corev2 (record and acknowledgement); Auth Core as issuer; Model Plane enforces and writes step records in session-core | master row 90 |
| composer picker, attachments, citation chips | Frontend Plane (verevonv3) | root CLAUDE.md; action-contract rule |
| personal versus org connection ownership | NOT DETERMINED in the matrices. Card names Control Plane; root CLAUDE.md gives Control Plane identity and orgs; integration-corev2 holds the connections | n/a |
| Space-visible threads and shared inbox | Application Plane for Space projections (root CLAUDE.md); OD11 open | plan line 467 |

## 6. Uncommitted-only and live-differs (relevant files)

Source: fresh `git --no-optional-locks status` and `diff --stat` on 2026-10-15, plus snapshot `.plan-work/live-tree-status.txt`. Diffstats are from `git diff --stat`. "Keyword check" = grep of added lines for connector, provider_action, connected app or citation.

Modified in LIVE:
- `model-gateway/src/tool_loop.rs` (425 lines changed). Keyword check: no ORG_CLASS, WEB_CLASS, tool_allowed, provider_action or connector hunk. Topic not otherwise characterised.
- `execution-core/src/runtime_loop/agent.rs` (207 changed). Keyword check: no provider or connector hunk. `data_category` and the record_tool_step ZDR and non-ZDR branches are identical to WT, shifted +7.
- `execution-core/src/runtime_loop/mod.rs` (+18). Adds `waived: bool` (Z-18 connection waiver) through run-step calls.
- `execution-core/src/grpc.rs` (98 changed), `knowledge_tools.rs` (131), `auth.rs`: modified; not characterised beyond keyword check (no provider hunk in grpc.rs or knowledge_tools.rs).
- `model-gateway/src/sse_events.rs` (203 changed) and `sse.rs` (883). Keyword hits are citation test and comment lines only. No connector origin.
- `model-gateway/src/zdr_precheck.rs`: modified; not characterised.
- `apps/Model Plane/proto/model_plane/v1/sessions.proto` (+42): not characterised.
- `apps/Model Plane/docs/sse-event-taxonomy.md` (+4): generated taxonomy, changes with sse_events.
- `verevonv3/src/features/dashboard/home/DashboardComposer.tsx` (+7): imports and props for ChatWorkPeriodPicker. Connector picker code unchanged; picker lines shift +7.
- `verevonv3/src/features/chat/components/chat-normalizers.ts` (+39), `ChatMessages.tsx` (+229), `ChatPage.tsx` (+13), `chat-types.ts` (+7), `use-chat-controller.ts` (+4); `src/shared/api/chat-client.ts` (+53), `src/shared/api/sse.ts` (+4). Keyword check: zero connector or citation hunks in the first six (citation and connector hunks in the SSE and client layers are not characterised).
- `verevonv3/package.json` (+2): adds `demo:build` and `demo:probe` scripts. Test, typecheck, lint and verify scripts unchanged.
- `integration-corev2/internal/api/server.go` (+30): ZDR acknowledgement route registration, an OrgZDR dependency, a BearerOnly wrapper, acknowledgement revocation on connection removal, org-wide ZDR purge with audit event `gdpr.integration.zdr_purged`. Action routes and actionRequiresApproval not changed in the added lines.
- `integration-corev2/internal/auth/middleware.go` (+93, -37), `internal/controlplane/clients.go` (+32), `internal/store/zdr.go` (+73), `internal/zdr/assessor.go` (+10), `internal/oauth/service.go` (+4), `internal/api/codex_subscription.go` (+8), `internal/store/memory_zdr.go`, `postgres_zdr.go`, `postgres_zdr_test.go`: ZDR and auth plumbing; characterised by file name only.

Untracked in LIVE (`??`), relevant:
- `integration-corev2/internal/api/zdr_acknowledgement.go` and `_test.go`; `internal/auth/bearer_only_test.go`; `internal/controlplane/org_zdr_posture_test.go`; `internal/store/memory_zdr_lifecycle_test.go`; `internal/store/storetest/zdr_lifecycle_contract.go`; `internal/zdr/wording.go`, `wording_test.go`, `texts/`.
- `model-gateway/src/zdr_posture.rs`; `mp-contracts/src/zdr_refusal.rs`; `mp-contracts/src/retention.rs`; `apps/Model Plane/api/contracts/zdr-refusals.json` (+ `.sha256`, per snapshot).
- `session-core/src/retention_receipt.rs`; `session-core/migrations/0044_run_retention_receipt.sql`; `0045_zdr_waived_runs_org_index.sql`; `migrations/revert/`.
- `verevonv3/src/shared/api/zdr-refusals.ts`.

Clean in both trees (cited in this file): `integration_tools.rs`, `permission/mod.rs`, `postcondition.rs`, `approval_delivery_worker.rs`, `capability_policy.rs`, `tests/autonomy_ladder_contract.rs`, `source_validation.rs`, `tool_retry.rs`, `mp-contracts/src/tool_arguments.rs`, `autonomy.rs`, `session-core/src/approval_delivery.rs`; all `integration-corev2/internal/actions/*.go` and `internal/db/migrations/`; gateway `integrations.rs` and `integrations/browse.rs`; `integrations-client.ts` and its test; `action-registry.ts` and test; `agent-tools.ts`; `chat-source-scope.ts`; `connect-bundles.ts`; `DashboardComposer.connector-attach.test.tsx`; `chat-normalizers.connector-attach.test.ts`; `prepare-chat-attachments.test.ts`; capability-core migrations; `ledger.md`; `capability-ownership-matrix.md`; `master-ownership-matrix.md`; `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md`; `docs/actions-surface-operations.md`; `seed_tool_knowledge.sql`; `deploy/docker-compose.yml`; `Ingestion Plane/Makefile`.

No relevant file is deleted (D) in LIVE.

## 7. NOT DETERMINED

- Correct ORG_CLASS_TOOLS placement for a provider read tool (conflicts with the doc comment at `tool_loop.rs:4221-4227`).
- Exact set-site of `ComposerFile.extractedText` for connector picks; the picker region is 5323-5411 in WT, not read line by line.
- Owner of personal connection ownership (no matrix row).
- Whether any CI or script runs integration-corev2 Go tests (none found in workflows or Makefile).
- Which secret the integration-corev2 header carries: doc says `INTEGRATION_COREV2_INTERNAL_KEY`, code reads `INGESTION_SERVICE_API_KEY`; header wiring not traced.
- Line ranges and behaviour in `calendar_read.go`, `drive_read.go` and `provider_auth_error.go` (only test names and call sites read).
- Whether provider actions surface in the org-wide inbox (OD11) or in Space threads: no check performed.
- Whether LIVE `sse.rs` (+883) changes citation production: not characterised.
- Behaviour of the ZDR refusal path for connector reads, since `zdr_posture.rs` and `zdr_refusal.rs` are untracked and were not read.
- Whether the card's "catalogue refresh = S effort" claim holds (not evaluated).
- Frontend action-contract conformance beyond the registry grep (no lint or eslint rule check).
