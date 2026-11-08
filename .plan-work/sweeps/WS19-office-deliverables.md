# WS19-office-deliverables: code-map sweep (audit item F-t3-014)

Read-only sweep, dated 2026-10-15. Both trees at HEAD afb83928.
- WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (clean).
- LIVE = `C:/dev/CoresSystem` (324 entries in `.plan-work/live-tree-status.txt`; M = modified, ?? = untracked-only, D = deleted).
- "clean" = not in the live status list. Paths are under `apps/` unless shown otherwise.

## 0. Sources and conventions

- Audit card: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` lines 548-563 (anchor 548; next card F-t3-015 at 564). Summary row 83. Section 9.1: F-t3-017 row 1541 (W1); W2 header 1551; F-t3-014 row 1564 (XL, was L; deps F-t3-017, OD18). Sequencing 1703 and 1720; 9.3 item 7 at 1689; Appendix B row 1832; Appendix C 2033-2038 (reference-side corrections only); Appendix D 2124 (split "roughly L + M + M").
- Evidence pack README read for structure only. No F-t3-014 evidence was traced into code.
- "Depends on WS08": WS08 = F-t3-017 (Attachments v2), sweep `.plan-work/sweeps/WS08-attachments-v2.md`. Used as a pointer only. Items marked "(WS08, not re-opened)" come from that sweep.
- The card was written against LIVE (the WS08 sweep found the same). Where WT differs, both are given.

## 1. Item framing

- Tier Priority, Partial, effort XL, Model Plane + Frontend Plane, wave W2, confidence High (card row 552).
- Three deliverables (card 554): (a) verify-and-render step inside code_interpreter with a LibreOffice + fonts image layer (OD18); (b) SKILL.md bundles in the content-addressed store with a `skill_file` read tool and read-only sandbox mount; (c) renderer registry in the artifact panel.
- Card 562 says to start with (a) because it needs only a LibreOffice and fonts image layer (owner ruling OD18) and no bundle format.
- Dependency: F-t3-017 precedes F-t3-014 (audit 1703). F-t3-014 precedes F-t3-038 (deck pipeline) and F-t3-058 (lifecycle) (1703).
- **OD18 is a recommended default, not an owner ruling.** Rules ledger `.plan-work/common/rules-ledger-adrs.md:258` and note `:272`; CHAT_GAP plan 502-504. Recommended content: standard fonts with transliteration; raise cap to 4 MB for xlsx, pptx, pdf after H1's zip guard; 60k-character limit stays; "Binary staging held in memory only after H5 and H6 prove the sandbox on Terra (G6)". OD18 blocks H3, H9, H10, H1 limits, G6, H7, H11 (plan 505).
- CHAT_GAP = `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md`. Rows: H1 :131 (M, extractors incl. zip guard); H3 :205 (L, direct PDF); H4 :206 (M, native charts in exports, dep H2); H5 :133 (S, code_interpreter on Terra: library set, import probe, live docx/pdf/pptx/xlsx/chart proof; list form :405); H6 :207 (M, stage attached docs into sandbox); H7 :269 (L, editable code cell, dep H5); H8 :249 (L, deck path v1, dep H4); H9 :208 (M, local OCR fallback, flag yes); H10 :209 (S, xlsx/pptx knowledge import, dep H1); H11 :270 (S, post-write diagnostics); G6 :240 (L, edit existing xlsx/pptx/docx in place, needs Terra proof). No H12 row found.

## 2. Citation verdicts (card F-t3-014)

Columns: # | card line | cited path:lines | claim | WT verdict | live | corrected path:lines

| # | card | cited path:lines | claim | WT verdict | live | corrected |
|---|---|---|---|---|---|---|
| 1 | 560 | model-gateway/src/tool_loop.rs:4310-4325 | code_interpreter description steers to native charts and live formulas | DRIFT. Definition is 4305-4309 (description 4307). 4310-4325 holds create_artifact, inbox_search, read_artifact | M (+370/-55) | LIVE MATCH: name 4319, description 4320, params 4321. Cited range 4310-4325 covers them on LIVE. Use WT 4305-4309 / LIVE 4319-4322 |
| 2 | 560 | execution-core/src/code_interpreter.rs:1-45 | hermetic per-call sandbox: read-only FS, no network, timeout, workspace deleted | MATCH (module doc 1-34; imports 36-48) | clean | same |
| 3 | 560 | "Dockerfile:55-80" (no path) = execution-core/Dockerfile:53-77 | runtime image carries openpyxl, xlsxwriter, python-docx, reportlab, pypdf | MATCH (apt list 61-77: bubblewrap 67; openpyxl/xlsxwriter 70; docx/reportlab/pypdf 71; pil/lxml 72). Comment 53-60 names code_interpreter. python-pptx is pip-pinned at :82, not in the card | clean | same |
| 4 | 560 | model-gateway/src/skills.rs:1-30 (short path) | skills are markdown text in an in-memory registry | MATCH (module doc 1-7; LEARNED_SOURCE_PREFIX 29; TTL const 37) | clean | apps/Model Plane/rust/services/model-gateway/src/skills.rs:1-7 |
| 5 | 560 (negative) | none | no bundle code, LibreOffice or pdftoppm in Model Plane | MATCH for LibreOffice: 0 hits for soffice/libreoffice/pdftoppm/poppler/unoconv in apps/Model Plane. QUALIFIED for bundles: ValidateSkillBundle RPC exists (capabilities.proto:24, 171-179; server.go:566-579 checks a registry row only); no `skill_file` tool; no SKILL.md reader (only proto comment gateway.proto:1021-1023 and capability-core roadmap text) | clean | same |
| 6 | 560 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx:442-455 | PDF previewed inline | MATCH (comment 441-447; iframe 448-456 when renderKind 'pdf') | clean | 448-456 |
| 7 | 560 | ChatArtifactPanel.tsx:555-570 | other binaries shown as download chip | MATCH (binary Match 457-460; ArtifactFileCard def 559, body 558-575) | clean | 457-460, 558-575 |
| 8 | 560 | "workspace_cas.rs" (no path) = execution-core/src/workspace_cas.rs:1-16 | "first S3 content-addressed client" | MATCH (doc 1-16: sha256-keyed, MinIO via aws-sigv4). Used by execution-core/src/workspace_hydrate.rs:22, 69, 103. Not used by code_interpreter.rs or tools.rs | clean | same |
| 9 | 560 | "capability-core agent_skills CRUD" (no path) = apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go | agent_skills CRUD | MATCH (SkillsHandler :46; route /api/v1/skills :65; SQL on agent_skills 147, 215, 244, 279, 316, 329). DDL is not in capability-core: session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:191 | clean | same |
| 10 | 560 | "plan tasks H5, H6, G6 and H8" (no path) = CHAT_GAP | plan rows exist | MATCH: H5 :133 (S), H6 :207 (M), G6 :240 (L), H8 :249 (L, dep H4) | clean | same |
| 11 | 562; 586 | "owner ruling OD18" (562); "owner ruling OD18 (4 MB, 60,000 characters)" (586) | owner ruling | DRIFT: recommended default; ledger :258 says "No ruling found in plans"; note :272; CHAT_GAP 502-504 | n/a | n/a |
| 12 | 562 | "files_in accepts flat names only" (no cite) | flat staging | MATCH: code_interpreter.rs:179-221 (parse_request), 227-264 (validate_input_name rejects separators, "..", absolute paths; comment 223-226 says input files are written flat) | clean | same |
| 13 | 562 | "a read-only bundle mount is a new staging path" (claim of absence) | no read-only mount exists | DRIFT: a read-only host-to-mount primitive exists unused. sandbox.rs:103-112 (`LaunchOptions.extra_ro_binds`, default empty), emitted 201-208. No production caller in apps/Model Plane (only sandbox.rs and its tests 546, 579) | clean | sandbox.rs:91-113 and 201-208 |
| 14 | 562 | "low-risk cap.command.sandbox" (no cite) | capability the sandbox binds to | MATCH: code_interpreter.rs:20-22 (binds the LOW-risk cap.command.sandbox; not in risky list). Registration: go/services/capability-core/migrations/0010_sandbox_code_execution_capability.up.sql:3, 82, 92 | clean | same |
| 15 | 562 ([INF]) | "ZDR retention class" for bundle content and thumbnails | retention class concept | ABSENT in WT: 0 hits for RetentionClass/retention_class in model-gateway/src. LIVE-only enum at sse_events.rs (M) and session-core/src/retention_receipt.rs:42 (??) | live-only | LIVE sse_events.rs (enum added); retention_receipt.rs:42 |
| 16 | 552, 562, 1541, 1564 | "Depends on F-t3-017" | dependency | cross-ref. F-t3-017 = WS08 sweep | n/a | n/a |
| 17 | 562 | "third-party bundles follow F-t3-059" | supply-chain rules | cross-ref only; F-t3-059 card not opened | n/a | n/a |

## 3. Facts the card does not state (correction candidates)

- **Read-only bind exists.** See row 13. Default is empty, so the hermetic argv is unchanged today.
- **Sandbox argv is hermetic for code_interpreter.** The policy is `WorkspaceWrite` with `network: Disabled` (code_interpreter.rs:579-581). Argv baseline: `--die-with-parent` 167; `--unshare-pid/uts/ipc` 168-170; `--new-session` 171; `--tmpfs /tmp` 174-175; `--ro-bind / /` 176-179; `--clearenv` before the allowlist (comment 182); `--unshare-net` when network is Disabled 222-224 (sandbox.rs). Env allowlist doc at code_interpreter.rs:414. Comment at code_interpreter.rs:576 says a bind is the last filesystem op in the argv, so it wins over the read-only root. Executor comment that `sandboxed: true` alone does not cut the network: executor.rs:349-351, 389.
- **Client-side Office export already exists.** `exportChatAnswer` (chat-export/index.ts:159-175) builds docx, xlsx and pptx in the browser and prints pdf. The menu is at ChatArtifactPanel.tsx:229-255 (Word 253, PowerPoint 255). No npm office library in `package.json` (grep for docx, pptx, jszip, exceljs, pdf-lib, xlsx, jspdf, pdfmake, file-saver: 0 hits).
- **Output caps differ from OD18.** code_interpreter output caps are 8 MiB per file (MAX_FILE_BYTES, code_interpreter.rs:79) and 16 MiB total (MAX_TOTAL_BYTES, :82). Over-cap files are still listed with `truncated: true` and empty content (header 31-34; test 788). OD18's 4 MB/60k figures are inbound attachment caps (WS08).
- **Generated files travel as data URIs.** `attachment_event(id, name, mime, data_uri, size)` at model-gateway/src/artifacts.rs:481. SSE name "attachment" (taxonomy `apps/Model Plane/docs/sse-event-taxonomy.md:22`; sse_events.rs:409 WT, 471 LIVE). `ChatEvent::Artifact` and `ChatEvent::Attachment` share the "artifacts" channel (sse_events.rs:360, WT).
- **Description-parity test reads the Dockerfile.** `sandbox_image_libraries()` (tool_loop.rs:10664-10666) reads `../execution-core/Dockerfile`. Test `code_interpreter_description_names_every_library_the_sandbox_image_ships` (10712) compares the "Available libraries:" text with the Dockerfile list (its required list starts at 10732; the final assertion was not read). Adding an apt package to the Dockerfile changes this test's input. Test 10757 (`..._says_how_to_make_each_office_output`) covers office-output guidance (body not read).
- **Two dispatch loops respected.** code_interpreter enters at the model-gateway tool_loop arm (WT 2528), then `tools::handle_code_interpreter` (tools.rs:163), then execution-core over gRPC (error map tools.rs:266). Rule: apps/Model Plane/CLAUDE.md, "Code Style".
- **Skill route chain.** Gateway doc at `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/agent_actions.rs:2-9` names `/v1/skills` and `/v1/capabilities` as model-gateway proxies. model-gateway http_routes.rs:320-326 (App-Plane proxy list incl. /v1/capabilities) and :371 (/v1/skills). capability-core registry_apis.go:65 (`/api/v1/skills`). Chat injection: `fetch_skill_context` (sse.rs:5530 WT, 5662 LIVE) and MatchSkills gRPC (model-gateway/src/grpc.rs:1693).
- **Skill tables are split.** The ruling says capability-core owns `agent_skills` (capability-ownership-matrix.md:114). The DDL is in session-core migration 0004:191. capability-core writes with its own SQL (registry_apis.go:147-329) and the learning sink calls `SessionCore.UpsertAgentSkill` (skillsink/sink.go:2-4; session-core/src/grpc.rs:4906). `skill_packages` and `skill_resources` (capability-core 0003:134-173) have no Go reader (grep of go/: 0 non-migration hits). gap-analysis.md:55 lists "skill-packs (SKILL.md)" as "No".
- **Renderer registry is set-based today.** `artifactRenderKind` (chat-artifacts.ts:196-209) classifies by kind sets (HTML 47, CODE 53, BINARY 65-68). docx, xlsx and pptx are BINARY and show a download card. Only pdf, when its content looks like PDF, is a viewer (204).
- **Plan H-row dependencies cite Terra.** Terra appears in H5, G6, OD18 and the e2e title `tests/e2e/product-office-outputs.spec.ts:96`. Meaning NOT DETERMINED.

## 4. Affected-file map (depth 2)

Paths are under `apps/Model Plane/rust/services/` unless shown. Live status: clean, M, ?? or D.

| path | role | lines WT / LIVE | live | tests (path: names) |
|---|---|---|---|---|
| model-gateway/src/tool_loop.rs | builtin code_interpreter definition; match arm; files_in extraction; code_interpreter_events | def 4305-4309 / 4319-4322; arm 2528 / 2554; files_in 2554 / 2580; call 2555-2570; arm line 365 / 389; events fn 479 | M (+370/-55; code_interpreter lines not in -U0 diff) | tool_loop.rs: code_interpreter_is_a_builtin_and_inline_allowed (10628; files_in assert 10646); ..._names_every_library_the_sandbox_image_ships (10712); ..._says_how_to_make_each_office_output (10757); ..._output_with_no_json_object_is_untouched (11048); ..._accepts_python_and_sh_and_refuses_everything_else (11151). LIVE assert 10891 |
| model-gateway/src/tools.rs | handle_code_interpreter: language check 131-140; zdr param 177; files_in param 180 and passthrough 189-190; tool name 202; zdr forwarded 214; gRPC error map 266 | 163-266 | clean | none found (not determined) |
| model-gateway/src/artifacts.rs | attachment_event (data URI) | 481 | clean | e2e_invoke_chain_test.rs: invoke_stream_generate_image_emits_attachment_and_persists (WT tests 5757) |
| model-gateway/src/sse_events.rs | ChatEvent::Attachment variant; artifacts channel; SSE name | Attachment 221 / 260; channel 360; name 409 / 471 | M (+202/-1; adds RetentionClass) | not determined |
| model-gateway/src/sse.rs | fetch_skill_context (skill injection); vision_stream; agentic_run_stream | 5530, 5646, 9167 WT / fetch_skill_context 5662 LIVE | M (+814/-69; adds zdr_error_stream, prepared_zdr_refusal_stream, direct_infer_turn, zdr_turn_settings) | no test name with vision or attach in sse.rs (WS08, not re-opened) |
| model-gateway/src/skills.rs | in-memory skill registry; TTL; list/get; score | header 1-7; LEARNED_SOURCE_PREFIX 29; DEFAULT_SKILL_CACHE_TTL_SECS 37; skill_cache_ttl 39 (env SKILL_CACHE_TTL_SECONDS :42); list 124; get 135; score 314 | clean | 12 tests, lines 417-615: upsert_assigns_id_when_empty 418; org_loaded_tracking 425; stale_org_is_repulled_so_operator_edits_land_without_a_restart 447; replace_learned_reflects_an_edited_skill_body 463; replace_learned_forgets_a_deleted_skill 480; replace_learned_leaves_operator_pushed_skills_alone 494; replace_learned_does_not_touch_another_org 511; agent_skill_maps_into_match_cache_skill 525; format_skill_block_renders_name_and_body 572; list_filters_by_org 578; match_ranks_keyword_overlap 587; match_hides_a_private_learned_skill_from_a_non_owner_non_grantee 615 |
| model-gateway/src/grpc.rs | match_skills RPC | 1693-1726 | M (+31/-16; not inspected by fn) | not determined |
| model-gateway/src/chat_turn_registry.rs | per-turn mirror of skill matching | 182, 351-378 | M (+36) | a_private_answer_is_flagged_for_the_turn_that_follows_it (WS08; LIVE-only) |
| model-gateway/src/http_routes.rs | App-Plane proxy list; /v1/skills | 320-326; 371 | M (+237, WS08) | not determined |
| model-gateway/tests/e2e_invoke_chain_test.rs | generated-image attachment e2e; added ZDR e2e | 5757 (WT) | M (+271/-17) | invoke_stream_generate_image_emits_attachment_and_persists (WT 5757); a_zdr_* tests (LIVE-only) |
| execution-core/src/code_interpreter.rs | hermetic runner: contract doc; InputFile; CodeRequest; parse; flat-name rules; mime map; caps; policy; run | doc 1-34; InputFile 133-138; CodeRequest 140-146; parse_request 179-221; validate_input_name 227-264; mime_for 274-283; MAX_FILE_BYTES 79; MAX_TOTAL_BYTES 82; run_with_timeout 501; policy 579-581 | clean | unsafe_files_in_names_are_rejected_never_sanitized (825); duplicate_and_reserved_files_in_names_are_rejected (851); mime_is_inferred_from_the_extension (894); a_files_in_entry_is_readable_by_the_code (1077); an_oversized_file_is_listed_with_its_real_size_and_no_content (1144) |
| execution-core/src/runtime_loop/mod.rs | execute_code_interpreter (step executor) | 891 / 905 | M (+18; six hunks at WT 318-471, none inside the function) | not determined |
| execution-core/src/permission/mod.rs | code_interpreter not a risky tool (ungated) | 415-423 | clean | code_interpreter_runs_ungated_while_shell_stays_gated (415) |
| execution-core/src/sandbox.rs | bwrap argv builder: LaunchOptions; read-only root; writable binds; extra read-only binds; unshare-net | LaunchOptions 91-113 (extra_ro_binds 103-112); baseline 167-182; writable binds 193-199; extra ro 201-208; unshare-net 222-224 | clean | extra_ro_binds_are_emitted_after_the_writable_roots (546); extra_ro_binds_defaults_to_empty_and_changes_nothing (579) |
| execution-core/src/executor.rs | EXECUTION_CODE_TIMEOUT_SECS (timeout_from_env); network caveats | 44; 84-88; 349-351; 389 | clean | not determined |
| execution-core/src/workspace_cas.rs | sha256-keyed MinIO/S3 CAS client; from_env | doc 1-16; from_env 70-84; test 300-304 | clean | from_env_is_none_without_configuration (300) |
| execution-core/src/workspace_hydrate.rs | the CAS client's current caller (durable workspace) | 22, 69, 103 | clean | not determined |
| execution-core/Dockerfile | runtime image: bubblewrap, python3 libraries (apt), python-pptx 1.0.2 (pip) | comment 53-60; apt 61-77; bubblewrap 67; libs 70-72; pip 82 | clean | parity test reads this file (tool_loop.rs 10664-10666) |
| execution-core/src/health_attest.rs | probe of code_interpreter libraries and sandbox capability | SANDBOX_CAPABILITY 58; library list 167-179; probe test near 1508 | clean | probe test near 1508 (not read in full) |
| execution-core/src/grpc.rs | execution-core gRPC (RunAgent path) | n/a | M (+76/-22; not inspected by fn) | not determined |
| session-core/src/grpc.rs | UpsertAgentSkill handler (learned skill persistence) | 4906-4963 | M (+817/-44; adds start_run_inner, SessionRetentionGrpc, count_waived_runs) | not determined |
| session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql | agent_skills DDL | 188-209 (CREATE 191) | clean | n/a |
| session-core/migrations/0006_agent_skills_origin.sql | provenance column on agent_skills | 11 | clean | n/a |
| go/services/capability-core/internal/api/registry_apis.go | SkillsHandler CRUD on agent_skills; route /api/v1/skills | 46, 65, 110, 147, 215, 244, 279, 316, 329 | clean | skills_ownership_test.go (POST/GET /api/v1/skills at 47, 70, 95, 128; names not read) |
| go/services/capability-core/internal/server/server.go | ValidateSkillBundle (registry check only) | 566-579 | clean | server_test.go call sites 563, 612, 895 (names not read) |
| go/services/capability-core/internal/skillsink/sink.go | learned skill candidates to session-core UpsertAgentSkill | 1-5 | clean | sink_test.go (not read) |
| go/services/capability-core/migrations/0003_capabilities_registry.up.sql | skill_packages (134-164), skill_resources (171-173); no Go reader | as listed | clean | none |
| go/services/capability-core/migrations/0010_sandbox_code_execution_capability.up.sql | registers cap.command.sandbox | 3, 82, 92 | clean | none |
| go/services/capability-core/internal/roadmap/data.go; go/services/capability-core/docs/gap-analysis.md | skill-packs (SKILL.md) listed "No" | data.go:180; gap-analysis.md:55 | clean | none |
| go/services/orchestrator-core/cmd/workflows/skill_promotion.go; cmd/activities/activities.go | promotion workflow calls ValidateSkillBundle | skill_promotion.go:91; activities.go:825-832 | clean | skill_promotion_test.go; activities integration_test.go (TestValidateSkillBundleActivity at 236) |
| proto/model_plane/v1/execution.proto | RunAgentRequest; highest field 17 | 76; 208 | clean | none |
| proto/model_plane/v1/gateway.proto | InvokeRequest content_parts 16 and attachments 18; AttachmentRef; Skill (SKILL.md comment) | 233, 239, 285; 1021-1024 | clean | none |
| proto/model_plane/v1/capabilities.proto | ValidateSkillBundle rpc and messages | 24; 171-179 | clean | none |
| scripts/verify-sandbox-isolation.sh | G1 isolation check (privileged Linux, bwrap) | file exists | clean | not run |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-artifacts.ts | ArtifactRenderKind union; kind sets; artifactRenderKind; office MIME labels and extensions | union 25; sets 47, 53, 65-68; labels 98-100; extensions 117-119; RENDER_KIND_LABELS 135; RENDER_KIND_MIMES 145; artifactRenderKind 196-209; friendlyMimeLabel doc 287 | clean | chat-artifacts.test.ts: artifactRenderKind describe 153 (tests 154, 163, 169, 175); friendlyMimeLabel 240 (241: labels the office formats the backend generates); parseDataUri 197-227 |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx | renderKind Match: PDF iframe; binary card; ArtifactFileCard; Word/PowerPoint export menu | export 229-255; pdf 448-456; binary 457-460; card 558-575 | clean | ChatArtifactPanel.test.tsx: describe ArtifactsPanel 48; exports a document artifact to Word, PowerPoint or PDF under its own title (66); previews HTML in an iframe sandboxed WITHOUT allow-same-origin (115); offers a working download for a generated spreadsheet (187); says an announced artifact could not be loaded instead of showing an empty box (207) |
| apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-export/index.ts | exportChatAnswer: docx, xlsx, pptx built in browser; pdf printed | 153-175 | clean | chat-export/index.test.ts: describe exportChatAnswer 34; cases 36, 49, 50, 58 (pdf without element throws), 63 |
| apps/Frontend Plane/verevonv3/src/shared/actions/agent-tools.ts | agent tool kind union only; no skill action contract | 9 | clean | none |
| apps/Frontend Plane/verevonv3/tests/e2e/product-office-outputs.spec.ts | Playwright; titles are about H1 extraction, not outputs | 58, 96 | clean | self |
| apps/Frontend Plane/verevonv3/playwright.config.ts | runner config | root | clean | not read |
| apps/Frontend Plane/verevonv3/package.json | scripts build 9, lint 11, test 13, typecheck 17 | as listed | M (+2: demo:build, demo:probe) | n/a |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/agent_actions.rs | header documents /v1/skills and /v1/capabilities as model-gateway proxies | 2-9 | clean | not determined |

Not present (grep over apps/, code files only): verevon_office, DeckTheme, recalc (code), LibreOffice, soffice, pdftoppm, skill_file. Hits are plan docs, proto comments, generated code and roadmap text only.

## 5. Related contracts

- **Tool contract (code_interpreter).** Input `{language, code, files_in:[{name, content_b64}]}`. Output `{stdout, stderr, exit_code, files:[{name, mime, bytes, content_b64, truncated?}]}` (code_interpreter.rs:25-34). Tool schema: tool_loop.rs:4308 (WT). MIME map: code_interpreter.rs:281-283 (xlsx, docx, pptx).
- **SSE.** Event `attachment`: taxonomy `apps/Model Plane/docs/sse-event-taxonomy.md:22` (`attachment` | `artifacts` | `ChatEvent::Attachment`). Code: sse_events.rs:360, 409 (WT); 471 (LIVE). LIVE M adds 4 taxonomy rows.
- **Renderer kinds.** `ArtifactRenderKind` union, chat-artifacts.ts:25.
- **Action contracts (`src/shared/actions`).** No skill action. `agent-tools.ts:9` is a kind union only. `chat.upload_document` and `chat.extract_document` are in action-registry.ts:2904-2924 (WS08, not re-opened).
- **Protos.** execution.proto: RunAgentRequest :76; subscription_connection_id = 17 at :208 (highest field read by grep; WS08 says no attachment field in 1-17). gateway.proto: content_parts = 16 :233 (no consumer per WS08), attachments = 18 :239, AttachmentRef :285, Skill :1024. capabilities.proto: rpc ValidateSkillBundle :24; messages :171-179.
- **Routes.** model-gateway `/v1/capabilities` (http_routes.rs:325-326), `/v1/skills` (:371). capability-core `/api/v1/skills` (registry_apis.go:65). Scope calls: model-gateway grpc.rs:2135 and 2238 (`/api/v1/capabilities/scopes`).
- **Migrations (forward SQL; latest file names).**
  - capability-core: 29 files (WT). Latest `0015_background_process_capability.up.sql` and `.down.sql`.
  - session-core: 43 files (WT). Latest tracked `0043_audit_outbox_requeue.sql`. LIVE untracked: `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/` (status lines 320-322).
  - execution-core: no migrations directory.
- **Config and env.** EXECUTION_CODE_TIMEOUT_SECS (executor.rs:84-88; default not read). SKILL_CACHE_TTL_SECONDS (skills.rs:42; default 60 at :37). workspace_cas.rs:71-82: MINIO_ENDPOINT, MINIO_ACCESS_KEY, MINIO_SECRET_KEY (all required); OBJECT_STORAGE_BUCKET (default model-plane-artifacts); MINIO_REGION (default us-east-1). EXECUTION_CORE_ADDR (deploy/docker-compose.yml:485; service block not read).
- **Data Plane.** documents-api-go and imports-core are not in WS19 code paths (WS08 covers them).
- **Sandbox image libraries.** Dockerfile apt list 69-72 plus pip python-pptx==1.0.2 (:82). Parity test at tool_loop.rs:10712.

## 6. Test and build commands

- **Rust (Model Plane).** From `apps/Model Plane/rust`: `cargo test -q -p execution-core --all-targets` and `cargo test -q -p model-gateway --all-targets`. Filter example: `cargo test -p execution-core code_interpreter`. Crate names: `rust/services/execution-core/Cargo.toml:2` (`name = "execution-core"`); `rust/services/model-gateway/Cargo.toml:2` (`name = "model-gateway"`). Shape from apps/Model Plane/CLAUDE.md "Testing".
- **Workspace and CI.** `cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:119` WT). LIVE M adds a session-core `zdr_receipt` step at lines 121-170.
- **Sandbox isolation.** `apps/Model Plane/scripts/verify-sandbox-isolation.sh` (exists; not run). Per capability-ownership-matrix.md:158 it needs a privileged Linux container with bwrap 0.8.0.
- **Proto.** `buf generate` (apps/Model Plane/CLAUDE.md "Build & Run"). `buf lint` has pre-existing debt.
- **Go.** `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...` (module `github.com/triodelab/model-plane/services/capability-core`, go.mod:1). Same for orchestrator-core (module `.../services/orchestrator-core`, go.mod:1).
- **Frontend.** `cd "apps/Frontend Plane/verevonv3" && pnpm lint && pnpm typecheck && pnpm test && pnpm build` (root CLAUDE.md "Build And Run"; package.json scripts: build = `pnpm typecheck && vite build`; test = node --test gateway scripts then `vitest run`; typecheck = `tsc -b`). Targeted: `pnpm vitest run` on chat-artifacts.test.ts, ChatArtifactPanel.test.tsx, chat-export/index.test.ts.
- **Playwright.** `playwright.config.ts` (root) and `tests/e2e/product-office-outputs.spec.ts`. Command shape not verified.
- **Gateway (only if BFF routes change).** `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (root CLAUDE.md).
- **User memory (not re-verified here).** Two cargo targets fail pre-existing on Windows (use `--lib`); verevonv3 typecheck, lint and vitest have pre-existing failures in worktrees.

## 7. Owners

- **Root CLAUDE.md, "Architecture Rules".** Model Plane owns reasoning, sessions and runs, inference, execution loops, capabilities, sandboxes, browser grants and cost. Frontend Plane owns the Verevon v3 UI and same-origin gateway/BFF normalization. "Zero Data Retention must propagate through any content-persisting boundary."
- **master-ownership-matrix.md.** :125 model-gateway (public invoke, stream, ZDR and cross-plane guards). :128 execution-core (tool loop, hooks, secret scrub, subagents). :130 capability-core (tools, commands, skills, plugins, MCP, routing, safety). :112 documents-api-go (document CRUD).
- **capability-ownership-matrix.md (apps/Model Plane/docs).**
  - :21 multimodal in inference-core (provider/vision, doc_intel).
  - :22 capability-core registries incl. `sandbox/` and `artifacts/`.
  - :65 sandbox leases in sandbox-manager.
  - :66 OS isolation (bwrap, Landlock, seccomp, egress) in execution-core with sandbox-manager provisioning.
  - :68 background process lifecycle in execution-core (process_host.rs).
  - :113-114 ruling: capability-core owns the skills registry (`agent_skills`); orchestrator-core SkillPromotionWorkflow writes into it; the learning loop produces candidates.
  - :137-138 G4 and G3 done; :158 G1 isolation check via scripts.
- **apps/Model Plane/CLAUDE.md, "Code Style".** Two tool-dispatch loops stay separate: model-gateway dispatch_tool (read-only router) and execution-core execute_step_inner. code_interpreter crosses through tools.rs gRPC, which fits the rule.
- **Ownership conflicts to resolve.**
  - Skills: capability-core owns the registry by ruling, but the agent_skills DDL is in session-core and capability-core writes it with its own SQL.
  - Office export: the renderer registry belongs to Frontend; the in-browser export (chat-export) is a second Office generator outside the sandbox.
  - Extraction: card plane column says Data Plane; WS08 found Ingestion Plane imports-core owns extraction.

## 8. Uncommitted-only (live)

Modified (M), with live `git --no-optional-locks diff --stat` and one-line summary:

| path (under apps/) | live | diff | summary |
|---|---|---|---|
| Model Plane/rust/services/model-gateway/src/tool_loop.rs | M | +370/-55 | retrieval_notice_events and rerank_gate_sentence helpers; code_interpreter definition, arm and files_in lines not in the diff |
| Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs | M | +18 | six insertions in execute_step, execute_step_with_browser_grant, execute_step_with_subagent and execute_step_inner (WT 318-471); execute_code_interpreter (WT 891) untouched |
| Model Plane/rust/services/model-gateway/src/sse_events.rs | M | +202/-1 | adds `pub enum RetentionClass`; attachment event name unchanged (LIVE 471) |
| Model Plane/rust/services/model-gateway/src/sse.rs | M | +814/-69 | adds ZDR direct-answer path (zdr_error_stream, prepared_zdr_refusal_stream, direct_infer_turn, zdr_turn_settings); fetch_skill_context not among added fns |
| Model Plane/rust/services/model-gateway/src/grpc.rs | M | +31/-16 | not inspected by function |
| Model Plane/rust/services/model-gateway/src/chat_turn_registry.rs | M | +36 | private-answer flag for the following turn (WS08) |
| Model Plane/rust/services/model-gateway/src/zdr_precheck.rs | M | +58/-226 | adds `pub fn http` refusal mapper; other content removed (not inspected further) |
| Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | M | +271/-17 | adds ZDR e2e tests; generated-image test status not re-checked in LIVE |
| Model Plane/rust/services/execution-core/src/grpc.rs | M | +76/-22 | not inspected by function (WS08: waived subscription admission) |
| Model Plane/rust/services/session-core/src/grpc.rs | M | +817/-44 | adds start_run_inner, SessionRetentionGrpc, count_waived_runs; UpsertAgentSkill (WT 4906) not among added fns |
| Model Plane/rust/services/inference-core/src/grpc.rs | M | +129/-2 | refusal-catalogue tests (WS08) |
| Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs | M | 67 changed lines (mixed) | not inspected |
| Model Plane/rust/crates/mp-contracts/src/lib.rs | M | +2 | not inspected |
| Model Plane/docs/sse-event-taxonomy.md | M | +4 | adds error, plan_proposed, retrieval_notice and source_scope_applied rows |
| .github/workflows/model-plane-rust-ci.yml | M | +50 | adds CARGO_TERM_COLOR and step `cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1` (LIVE 121-170) |
| Frontend Plane/verevonv3/package.json | M | +2 | adds demo:build and demo:probe; test, lint, typecheck and build lines unchanged in diff |

Untracked only (??), relevant:
- Model Plane/rust/services/session-core/src/retention_receipt.rs (RetentionClass enum at :42); session-core/migrations/0044_run_retention_receipt.sql; 0045_zdr_waived_runs_org_index.sql; migrations/revert/.
- Model Plane/rust/crates/mp-contracts/src/retention.rs; zdr_refusal.rs; tests/zdr_retention_matrix.rs; tests/fixtures/.
- Model Plane/rust/services/model-gateway/src/zdr_posture.rs; zdr_features.rs; relevance_calibration.rs (+ calibration/).
- Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs (imports sse_events::RetentionClass); zdr_feature_table.rs; zdr_refusal_inventory.rs; fixtures zdr-posture-matrix.json (+ .sha256), fixtures/auth-core/.
- Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs; inference-core/src/grpc/zdr_waiver_route_tests.rs; inference-core/tests/zdr_refusal_inventory.rs.
- Model Plane/api/contracts/zdr-refusals.json (+ .sha256).

Deleted (D) among relevant files: none.

Relevant and clean in LIVE (not in status list): code_interpreter.rs, execution-core Dockerfile, sandbox.rs, executor.rs, workspace_cas.rs, workspace_hydrate.rs, tools.rs, artifacts.rs, skills.rs, permission/mod.rs, health_attest.rs, all of capability-core and orchestrator-core, the four protos, chat-artifacts.ts, ChatArtifactPanel.tsx, chat-export, agent-tools.ts, action-registry.ts, the product-office-outputs spec, playwright.config.ts. The plan and ledger files are under `.plan-work` and are not git-tracked.

## 9. NOT DETERMINED

1. H12: no row in the CHAT_GAP plan table. OD18 says "Skip H11 and H12".
2. Terra: meaning (environment, model route or provider) not determined.
3. Whether soffice runs under the WorkspaceWrite bwrap policy (profile directory, font cache, /tmp tmpfs). No runtime evidence. DEFAULT_CODE_TIMEOUT_SECS value not read; tool description says ~30 s (tool_loop.rs:4307).
4. Whether ZDR runs persist code_interpreter attachment data URIs or thumbnails. Transcript persistence not traced.
5. Whether the LIVE zdr_refusal_inventory.rs tests (execution-core, model-gateway, inference-core) cover code_interpreter. Not read.
6. Which service writes agent_skills at runtime, and whether capability-core's pool targets the session-core database. Compose DATABASE_URL not read.
7. Whether skill_packages and skill_resources (capability-core 0003) are the intended bundle store. No Go reader found; design intent not in code.
8. Whether the in-browser Office export (chat-export) stays in scope or is superseded.
9. How code_interpreter_events (tool_loop.rs:479 WT) maps outputs to ChatArtifact entries. Body not read. Frontend attachment-to-artifact normalizer not traced.
10. Which compose service builds the sandbox image. Only the EXECUTION_CORE_ADDR env reference (compose:485) was read.
11. LIVE line numbers for sse.rs vision_stream and agentic_run_stream (WS08 gave 5778 and 9527; not re-opened).
12. Parser rules of the description-parity test (tool_loop.rs:10718-10728). Whether a LibreOffice apt entry would pass unchanged.
13. Whether model-gateway/src/grpc.rs and execution-core/src/grpc.rs changes affect the code_interpreter or skill path. Not inspected by function.
14. Reference-side claims (card 556-558, Appendix C) were not re-verified; out of scope for a code-map sweep.
15. F-t3-059, F-t3-038, F-t3-058 and F-t3-084 anchors not opened. Cross-references only.
16. No tests were run and no build was attempted (read-only sweep).
