# WS08-attachments-v2: code-map sweep (audit item F-t3-017)

Read-only sweep. Dated 2026-10-14. Both trees at HEAD afb83928.

## 0. Conventions and sources

- WT = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (clean at afb83928).
- LIVE = `C:/dev/CoresSystem` (same HEAD plus uncommitted work; list in `.plan-work/live-tree-status.txt`, 324 entries).
- Paths are repo-relative to the root named. "clean" means not in the live status list. Status codes: M modified, ?? untracked-only, D deleted.
- The card was written against LIVE. Most card line numbers match LIVE, not WT. Verdicts give both.
- Audit doc (read-only): `C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md`
  - Card: lines 580-594. Summary row: 85. Section 9.1 W1 header 1528, row 1541. Sequencing note 1703. Appendix B row 1834. Appendix C 2056-2061. Appendix D: no F-t3-017 line (id grep over 2111-2161 is empty).
- Plan tokens resolved. CHATGAP = `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md` (clean): H6 at :207, H9 at :208, tool_loop note at :378, imports-core note at :389, OD18 at :502-504. OD18 ledger row: `.plan-work/common/rules-ledger-adrs.md:258` and :272.

## 1. Corrections to the card (verified in code)

1. **Durable upload path.** The active route is Frontend gateway, then documents-api-go. It is not model-gateway. `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/documents.rs:43-53` says the model-gateway `/v1/chat/documents` forward is dead, because Data Plane v2 disabled `DocumentService.CreateDocument`. The 412 the card cites sits in that dead handler (WT `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:7566-7571`). The active ZDR refusal is 403 in `apps/Data Plane v2/services/documents-api-go/internal/handler/documents.go:304-305`.
2. **"Asynchronous" is contradicted.** `apps/Data Plane v2/services/documents-api-go/cmd/main.go:204-205` calls POST /v1/documents "the canonical synchronous HTTP write". Create returns 201 or 200 with the row (`documents.go:324-326`). Retrieval readiness comes later through `dataplane.documents.indexed` (`cmd/main.go:204-206`).
3. **"No production caller" holds only for the TS client.** `uploadChatDocument` has no callers in `apps/Frontend Plane/verevonv3/src`, tests included. The gateway action `chat.upload_document` (`apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts:2904-2913`) dispatches to the same handler (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs:4758-4779`, call at :4771).
4. **Extraction is server-side for office and PDF files.** `prepare-chat-attachments.ts:22-28` sends PDF, DOCX, XLSX and PPTX to `extractChatDocument`, which goes gateway to imports-core. Only text types are read in the browser (`:30-31`). The card says "extracted client-side" and gives extraction to Data Plane. The code owner is Ingestion Plane imports-core (`apps/Ingestion Plane/imports-core/app/main.py:204-212`).
5. **Vision diversion is real.** In WT `sse.rs` the vision branch runs before the agentic branch: vision at :1341-1355, `if agentic_requested` at :1381, agentic return at :1423. A Do turn with an image never reaches the agentic path.
6. **Waiver-ZDR vision refusal exists in LIVE only.** LIVE `sse.rs:971-979` refuses vision on waived turns (`select_image` at :976). It depends on untracked `model-gateway/src/zdr_posture.rs`, which is absent from WT. WT has no waiver check. WT `sse.rs:976-977` refuses any attachment on an effective-ZDR turn.
7. **Subscription route drops images silently, as the card says.** `chat-client.ts` drops `request.attachments` when `subscriptionBacked` (LIVE :1144, WT :1114). The subscription provider reports `supports_vision: false` (`apps/Model Plane/rust/services/inference-core/src/provider/codex_subscription.rs:612`).
8. **Disclosure helper is dead code.** `unsentAttachmentNames` (`chat-normalizers.ts`, WT :1098, LIVE :1137) has no callers. Its comment describes the pre-inlining behaviour (PDFs dropped), which `inlineTextAttachments` has since replaced.
9. **OD18 is not a ruling.** `.plan-work/common/rules-ledger-adrs.md:258` records "Recommended: ... raise cap to 4 MB for xlsx, pptx and pdf after H1 zip guard; 60k-character limit stays ... No ruling found in plans." The card's "owner ruling OD18 (4 MB, 60,000 characters)" overstates this.
10. **Caps in code today (not OD18).**
    - Browser: 1,000,000 bytes for every attachment, images included (`prepare-chat-attachments.ts:18`, checked before the image branch). 60,000 characters per document (:33; WT `chat-normalizers.ts:1131`). 120,000 per turn (:35; WT :1132).
    - imports-core: 1,000,000 decoded bytes (`apps/Ingestion Plane/imports-core/app/chat_extract.py:29-30`), 60,000 characters (:12, :45-46), base64 max 1,400,000 characters (:20).
    - inference-core: image 20 MB (`apps/Model Plane/rust/services/inference-core/src/grpc.rs:40`), document 50 MB (:42).
11. **No read-only staging exists.** `code_interpreter` `files_in` is model-authored `{name, content_b64}` (`apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs:190-195`), written flat into the writable workspace (:133-145). The only "read-only" text in that file is the sandbox policy doc comment (:14, :65).
12. **Phase-2 contract gap.** `RunAgentRequest` (`apps/Model Plane/proto/model_plane/v1/execution.proto:76`) has no attachment field. Its highest field number is 17 (`subscription_connection_id`, :208). `gateway.proto` already declares `InvokeRequest.content_parts = 16` (:233) and `attachments = 18` (:239), with `AttachmentRef` at :285-291. No Model Plane service or verevonv3 source references `content_parts`. The live attachment path is the Rust `InvokeAttachment` (`model-gateway/src/vision.rs:14`, `http_routes.rs:6693` WT).
13. **OCR is not evidenced.** `apps/Model Plane/rust/services/inference-core/src/provider/doc_intel.rs:1-18` is Azure Document Intelligence `prebuilt-layout` (text, tables, structure). The file never names OCR. imports-core has no OCR term (grep for ocr, OCR and tesseract finds nothing in `apps/Ingestion Plane/imports-core`). H9 is planned only (CHATGAP :208, :389).
14. **Vision flag exists in the catalogue but nothing gates on it.** `ModelInfo.capabilities` (WT `chat-client.ts:849-852`, LIVE :866-869) is filled from the `features` list (WT :2471, LIVE :2524). The server emits "vision" from `supports_vision` (`provider/mod.rs:677-679`). No verevonv3 source gates on `'vision'`. The only `vision` string is modality inference (WT :2407-2419).
15. **Attach-time checks.** `addFiles` (`DashboardComposer.tsx`) keeps every incoming file with no MIME or size check. A static `accept` list sits on the input. The MIME check happens later, at send time, and only for non-images (`prepare-chat-attachments.ts:23-32`, which throws 'unsupported').

## 2. Citation verification (card F-t3-017)

| # | Card | Cited path:lines | Claim (short) | WT verdict | LIVE status | Corrected WT / LIVE |
|---|---|---|---|---|---|---|
| C1 | F-t3-017 | apps/Frontend Plane/verevonv3/src/features/chat/lib/prepare-chat-attachments.ts:6-43 | read before send; 1 MB; 60k; 120k | MATCH (caps :18, :33, :35); office/PDF extraction is server-side (:28) | clean | same |
| C2 | F-t3-017 | chat-normalizers.ts:1150-1240 (= apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts) | docs inlined into message text; images only in stream | DRIFT | M | WT: inline 1147-1172, caps 1131-1132, toStreamAttachments 1192-1202. LIVE: inline 1186-1211, caps 1170-1171, toStreamAttachments 1231-1241. Card range = LIVE |
| C3 | F-t3-017 | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:1144 | image parts dropped on subscription route | DRIFT (WT :1114 holds the line) | M | LIVE :1144 MATCH: `attachments: subscriptionBacked ? [] : request.attachments ?? []` |
| C4 | F-t3-017 | apps/Model Plane/rust/services/model-gateway/src/sse.rs:1405-1432 | image diverts whole turn, Do included, into AnalyzeImage | DRIFT: vision cond :1341, call :1343-1354; agentic cond :1381; agentic return :1423 | M | LIVE MATCH: vision cond :1409, call :1411-1422, image-gen :1429, agentic return :1491 |
| C5 | F-t3-017 | apps/Model Plane/rust/services/model-gateway/src/vision.rs:1-60 | uses first image only | MATCH (`select_image` :52-53 uses `find`) | clean | same |
| C6 | F-t3-017 | apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:1699-1725,3419-3430 | no MIME or capability check at attach | DRIFT: addFiles :1695, attach button :3415, accept :3112 | M | LIVE MATCH: addFiles :1699-1727 (no filter), attach :3419-3433, accept :3116 |
| C7 | F-t3-017 | apps/Model Plane/rust/services/inference-core/src/provider/mod.rs:655-685 | vision and image flags emitted | MATCH (`serves_modality` :660-662, `feature_flags` :669-687) | clean | same |
| C8 | F-t3-017 | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:2081-2118 | durable upload: text-only, async, 412 under ZDR, no caller | PARTIAL: fn :2050-2065; `type: 'text'` ~:2058 MATCH; 412 is in the dead server handler; "async" CONTRADICTED; "no TS caller" MATCH | M | LIVE fn :2103-2118, doc :2096-2102 MATCH; generic 412 mapping at :2756 |
| C9 | F-t3-017 | apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs:180-260 | files_in is model-authored base64 only | MATCH (`RawFile` :190-195; `parse_request` :179-221; `validate_input_name` :227-264) | clean | same |
| C10 | F-t3-017 | apps/Model Plane/rust/services/model-gateway/src/sse.rs:960-977 | vision refused for waived ZDR turns | DIFFERS: no waiver check; :976-977 refuses any attachment on effective ZDR | M | LIVE :957-979 MATCH (`decide_for` :967; waiver plus `select_image` :971-976). Needs untracked zdr_posture.rs |
| C11 | F-t3-017 | "imports-core extract route" (no cite) | reuse for extraction | MATCH `apps/Ingestion Plane/imports-core/app/main.py:204-212` (POST /api/v1/import/extract) | clean | same; caller `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/documents.rs:14-31` |
| C12 | F-t3-017 | "inference-core AnalyzeDocument OCR" (no cite) | OCR via AnalyzeDocument | PARTIAL: Azure DI `prebuilt-layout` (doc_intel.rs:14-18); no OCR term; gRPC `analyze_document` at grpc.rs:775-798 | doc_intel clean; grpc.rs M | same |
| C13 | F-t3-017 | "ModelInfo.capabilities" (no cite) | vision flag from catalogue | EXISTS: type WT :849-852; mapping WT :2471 | M (type :866-869; mapping :2524) | not gated anywhere in src |
| C14 | F-t3-017 | "plan H6", "plan H9" (no cite) | sandbox staging; imports-core OCR | MATCH in CHATGAP :207-208, :378, :389 | clean | same |
| C15 | F-t3-017 | "owner ruling OD18 (4 MB, 60,000 characters)" | owner ruling | DRIFT: recommended default only; "No ruling found" (rules-ledger-adrs.md:258, :272; CHATGAP :502-504) | n/a | n/a |
| C16 | 9.1 row :1541 | "Data Plane upload route" | upload owner is Data Plane | MATCH: documents-api-go Create (`cmd/main.go:259`; `documents.go:257`) | clean | same |
| C17 | card row :584 | "Data Plane (blob store, extraction)" | extraction owner | DRIFT: extraction is Ingestion Plane imports-core (`main.py:204`) | clean | n/a |

## 3. Affected-file map (depth 2)

Frontend, verevonv3 (`apps/Frontend Plane/verevonv3/`):

| path under src/ unless stated | role | lines WT / LIVE | live status | existing tests (path: names) |
|---|---|---|---|---|
| features/chat/lib/prepare-chat-attachments.ts | pre-send read; 1 MB gate; extension gate; office and PDF extraction call | 6-43 / same | clean | features/chat/lib/prepare-chat-attachments.test.ts: "preserves both same-named files in order"; "rejects a partial batch when the second file cannot be read"; "rejects over-budget text instead of indexing it after the answer starts"; it.each pdf/docx/xlsx/pptx "waits for ephemeral %s extraction"; "does not accept a PDF when extraction fails"; "reuses an already-extracted connector-picker attachment"; "still extracts a PDF with no pre-populated text" |
| features/chat/components/chat-normalizers.ts | inline text documents; image-only stream split; caps; unsentAttachmentNames (no callers) | WT 1089-1202 / LIVE 1137-1241 | M (+39, ZDR presentation helpers) | features/chat/components/chat-attachment-restoration.test.ts (appendInlinedAttachments, contentWithPreparedSources); chat-normalizers.connector-attach.test.ts (inlineTextAttachments :44, appendInlinedAttachments :65); chat-normalizers.zdr-refusal.test.ts (??) |
| features/chat/components/use-chat-controller.ts | send, queue and regenerate call sites for toStreamAttachments, inlineTextAttachments, appendInlinedAttachments | WT 1894-1899, 2532, 2856-2867, 3650 / LIVE +4 | M | use-chat-controller.zdr-refusal.test.ts (??) |
| features/dashboard/home/DashboardComposer.tsx | addFiles (no filter); attach button; static accept; prepare call; connector-import extract call | addFiles WT 1695 / LIVE 1699; attach WT 3415 / LIVE 3419; accept WT 3112 / LIVE 3116; prepare call WT 2485 / LIVE 2489; extract WT 5387 / LIVE 5394 | M (+7, work-period picker wiring) | none naming addFiles, attach or accept (grep over *.test.*) |
| shared/api/chat-client.ts | extractChatDocument; ModelInfo.capabilities; listModels; subscription attachment drop; uploadChatDocument; 412 mapping | extract WT 30-35 / LIVE 31-35; ModelInfo WT 849-852 / LIVE 866-869; listModels WT 2443 / LIVE 2496; drop WT 1114 / LIVE 1144; upload WT 2050-2065 / LIVE 2103-2118; 412 WT 2703 / LIVE 2756 | M (+53) | shared/api/chat-client.test.ts (M, +22 LIVE): "routes a connected subscription without incompatible tool features" (WT 504-537; image dropped 513-524); "carries Do on a subscription turn as the agentic feature plus plan_mode..." (WT 539-570; attachments 551, 563); "maps a catalogue feature list carrying agentic onto the model capabilities" (WT 615). shared/api/chat-client.zdr-capability.test.ts. No test references uploadChatDocument |
| shared/actions/action-registry.ts | chat.upload_document; chat.extract_document; input schemas | upload 2904-2913; extract 2915-2924; chatUploadDocumentInput 1054; content_base64 max 1_400_000 at 2922 | clean | shared/actions/catalog-manifest.test.ts (manifest shape only) |
| tests/e2e/product-office-outputs.spec.ts (verevonv3 root) | Playwright: both attachments reach /chat/documents/extract | 43-66 | clean | itself |
| tests/e2e/product-readiness.spec.ts | Playwright: extract route stubbed to 503 | 13 | clean | itself |

Gateway, `apps/Frontend Plane/verevonv3/apps/gateway/src/` (crate `verevon-gateway-rs`):

| path | role | lines | live status | tests |
|---|---|---|---|---|
| domains/chat.rs | routes: POST /api/v1/chat/documents, POST /api/v1/chat/documents/extract | 65-68 | clean | none found |
| domains/chat/documents.rs | upload proxy to documents-api /v1/documents (comment 43-53); extract proxy to imports-core | upload 54-122; extract 14-31 | clean | none found (not determined) |
| domains/knowledge/shared.rs | `proxy_data_plane_json` used by the upload proxy | 73-117 | clean | none found |
| domains/actions/dispatchers.rs | `chat.upload_document` dispatcher calls upload_chat_document | 4758-4779 (call 4771) | clean | none found |
| config.rs | DOCUMENTS_API_URL default http://dpv2-documents-api:8010; IMPORTS_API_URL default http://imports-core:3025 | 213, 224 | clean | n/a |

Model Plane, `apps/Model Plane/` (crates `model-gateway`, `inference-core`, `execution-core`, `mp-contracts`, `session-core`):

| path under rust/services/ unless stated | role | lines WT / LIVE | live status | tests |
|---|---|---|---|---|
| model-gateway/src/vision.rs | InvokeAttachment :14-27; ImageInput :32-36; is_image :38-46; select_image :52-70 (first image only) | 1-150 / same | clean | selects_image_by_url :96; decodes_inline_base64_and_strips_data_url_prefix :104; infers_image_from_mime_when_kind_absent :118; ignores_non_image_and_empty :128; rejects_bad_base64 :140 |
| model-gateway/src/sse.rs | vision branch; ZDR block; agentic branch; vision_stream; image_gen_stream; agentic_run_stream | vision WT 1338-1355 / LIVE 1406-1423; ZDR WT 973-977 / LIVE 957-979; agentic WT 1381-1423 / LIVE 1491; vision_stream WT 5646 / LIVE 5778; image_gen WT 5826 / LIVE 5958; agentic_run_stream WT 9167 / LIVE 9527 | M (+883 across the file) | no fn name with vision or attach in sse.rs. e2e_invoke_chain_test.rs:5757 invoke_stream_generate_image_emits_attachment_and_persists (generated image, not vision input) |
| model-gateway/src/http_routes.rs | InvokeRequest with attachments; /v1/chat/documents route; create_document | InvokeRequest WT 6563; attachments WT 6693; route WT 199; create_document WT 7552-7589 (412 at 7566-7571) / LIVE 7665-7702 (zdr_posture gate 7679-7684); analyze_image WT 3724; image_input helper WT 3809 (role not read) | M (+237) | tests/e2e_invoke_chain_test.rs holds AnalyzeImage mock servers only (WT :603, :835, :1001). No test fn named for create_document found |
| model-gateway/src/tool_loop.rs | code_interpreter arm: bearer and Space checks; files_in read; call into tools.rs | files_in WT 2554; call WT 2555-2570; arm start LIVE 2554 | M (+425) | tool_loop.rs:10646 (WT) asserts "files_in" in parameters_json (fn name not read) |
| model-gateway/src/tools.rs | handle_code_interpreter; passes files_in JSON into tool input | 163; files_in param 180; passthrough 189-190 | clean | not determined |
| model-gateway/src/artifacts.rs | attachment_event (ChatEvent::Attachment for generated files) | 481; use 794 (WT) | clean | e2e_invoke_chain_test.rs:5757 (above) |
| model-gateway/src/sse_events.rs | ChatEvent::Attachment to SSE name "attachment" | WT 409 | M (+203) | not determined |
| model-gateway/src/chat_turn_registry.rs | per-turn registry; candidate only for a phase-2 ephemeral store (not verified) | 36 added | M (+36) | a_private_answer_is_flagged_for_the_turn_that_follows_it (live-only) |
| execution-core/src/code_interpreter.rs | files_in parse; name rules; flat staging; code run | InputFile WT 133-145; parse_request 179-221; RawFile 190-195; validate_input_name 227-264; decode_b64 266-269 | clean | unsafe_files_in_names_are_rejected_never_sanitized :825; duplicate_and_reserved_files_in_names_are_rejected :851; a_files_in_entry_is_readable_by_the_code :1077 |
| execution-core/src/runtime_loop/mod.rs | execute_code_interpreter (step executor) | WT 891 | M (+18) | not determined |
| execution-core/src/permission/mod.rs | code_interpreter ungated while shell gated | WT 415 | clean | code_interpreter_runs_ungated_while_shell_stays_gated |
| execution-core/src/grpc.rs | RunAgent handling; live adds subscription-waiver admission | n/a | M (+98) | LIVE-only: a_waived_subscription_run_is_admitted_only_on_its_own_connection |
| inference-core/src/provider/mod.rs | ModelInfo :351; ProviderCapabilities.supports_vision :421; serves_modality; feature_flags | serves_modality :660-662; feature_flags :669-687 | clean | G4 "2 tests" per capability-ownership-matrix.md:137 (names not read) |
| inference-core/src/provider/anthropic.rs | supports_vision: true | 933 | clean | not determined |
| inference-core/src/provider/codex_subscription.rs | supports_vision: false (subscription route) | 612 | clean | not determined |
| inference-core/src/provider/vision.rs | AnalyzeImage provider chain; image_url_or_data_url | fn :563; unit test near :642 | clean | name not read |
| inference-core/src/provider/doc_intel.rs | AnalyzeDocument (Azure DI prebuilt-layout) | :14-18; structs :23-34 | clean | not determined |
| inference-core/src/grpc.rs | analyze_image RPC; validate_image_input; MAX_IMAGE_BYTES; MAX_DOCUMENT_BYTES; analyze_document RPC | analyze_image WT 707; validate_image_input WT 1234-1249; MAX_IMAGE_BYTES :40; MAX_DOCUMENT_BYTES :42; analyze_document WT 775-798 | M (+131) | LIVE-only refusal-catalogue tests at LIVE 2222-2320 (REFUSING_HANDLERS, OPERATIONS, every_refused_modality_is_the_catalogue_refusal_with_its_operation_named) |

Contracts and docs:

| path | role | lines | live status | notes |
|---|---|---|---|---|
| apps/Model Plane/proto/model_plane/v1/execution.proto | RunAgentRequest; fields 1-17; no attachment field | message 76; plan_mode 143; space_id 168; highest field 17 at 208 | clean | phase-2 field would be 18 unless reserved (not checked) |
| apps/Model Plane/proto/model_plane/v1/gateway.proto | InvokeRequest (message at 182); content_parts 16 (:233, no consumer); attachments 18 (:239); ContentPart :265-274; AttachmentRef :285-291 | as listed | clean | |
| apps/Model Plane/proto/model_plane/v1/inference.proto | rpc AnalyzeImage :54; AnalyzeImageRequest :903-909; AnalyzeImageResponse :933 | as listed | clean | |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | adds CountWaivedRuns (not attachment related) | n/a | M (+42) | |
| apps/Model Plane/docs/sse-event-taxonomy.md | attachment row :22; live adds error, plan_proposed, retrieval_notice, source_scope_applied | :22 | M (+4) | |
| apps/Model Plane/docs/chat-parity-audit.md | row 9 says content_parts route to vision (:243); code grep contradicts | :243 | clean | doc claim unsupported by code |

Data Plane and Ingestion Plane:

| path | role | lines | live status | tests |
|---|---|---|---|---|
| apps/Data Plane v2/services/documents-api-go/cmd/main.go | POST /v1/documents; "synchronous" comment | comment 204-206; route 259 | clean | n/a |
| apps/Data Plane v2/services/documents-api-go/internal/handler/documents.go | Create; ZDR gate (403); status 201/200; persistentZDRReason | Create 257-333; ZDR 68-83; gate 304-305; status 324-326 | clean | internal/handler/security_test.go: TestRejectPersistentZDRContent :30, TestPersistentZDRReasonIsMonotonic :67. internal/validate/validate_test.go: TestCreateDocument_ZDRClassificationRoundTrip :29, TestCreateDocument_ContentRequired :68 |
| apps/Ingestion Plane/imports-core/app/main.py | POST /api/v1/import/extract (ephemeral, no-store); durable jobs upload at :215 (knowledge import, not chat) | 204-212; 215 | clean | tests/test_chat_extract.py: test_text_extraction_is_complete_and_ephemeral :328; test_docx_extraction :334; test_xlsx_extraction_keeps_sheet_order_and_cell_values :348; test_pptx_extraction_keeps_slide_order_and_text :357; test_unreadable_or_oversized_input_is_rejected :371 (parametrized); test_xlsx_with_absolute_style_relationship_target_still_reads_sheet :384; xlsx formula tests :449-551 |
| apps/Ingestion Plane/imports-core/app/chat_extract.py | caps: 60k :12; xlsx 55k :15; base64 max :20; 1 MB 413 :29-30; 60k 413 :45-46 | 12-50 | clean | as above |

## 4. Related contracts

- Routes. POST /api/v1/chat/documents (gateway chat.rs:65-66). POST /api/v1/chat/documents/extract (chat.rs:68). Model-gateway POST /v1/chat/documents (`http_routes.rs:199` WT, dead). POST /v1/documents (documents-api-go `cmd/main.go:259`). POST /api/v1/import/extract and POST /api/v1/import/jobs/upload (imports-core `main.py:204`, :215).
- Actions. `chat.upload_document` (action-registry.ts:2904-2913; description says "Refused under ZDR", :2906). `chat.extract_document` (:2915-2924; description says "without storing it in organization knowledge"). Frontend invocation of either action: not searched.
- SSE. `attachment` event (`sse_events.rs:409` WT; taxonomy :22). Image generation emits attachments through `artifacts.rs:481` and :794. The vision answer path is `vision_stream` (WT sse.rs:5646-5682; later events not read). Live adds `retrieval_notice`, `error` (ZdrError) and `plan_proposed` rows (taxonomy, uncommitted).
- Migrations, forward SQL only.
  - `apps/Data Plane v2/infra/postgres/migrations/`: 39 files. Latest `20261003150000_graph_extraction_state.sql`. Latest document-related `20260907120000_document_space_ref.sql` (with .down). Source-object table at `20260526090000_add_source_objects_for_quickwit.sql` (schema not read). documents-api-go has no migrations dir of its own.
  - `apps/Model Plane/rust/services/session-core/migrations/`: 43 files in WT. Latest tracked `0043_audit_outbox_requeue.sql`. LIVE-only (??): `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/`.
  - `apps/Ingestion Plane/imports-core/migrations/`: `001_init.sql` to `004_space_import_intent.sql` (latest). No attachment table by file name.
- Config and env. Gateway: `DOCUMENTS_API_URL`, `IMPORTS_API_URL` (config.rs:213, :224). Hard-coded caps in `prepare-chat-attachments.ts`, `chat-normalizers.ts`, `chat_extract.py`, `inference-core/src/grpc.rs`. No vision or attachment feature flag was found in this sweep (not exhaustive).
- Protos. See section 3 (execution.proto, gateway.proto, inference.proto).

## 5. Test and build commands

- Frontend (root CLAUDE.md "Build And Run"): `cd "apps/Frontend Plane/verevonv3" && pnpm lint && pnpm typecheck && pnpm test && pnpm build`. Vitest for `*.test.ts(x)`. User memory notes pre-existing typecheck, lint and vitest failures in this tree.
- Targeted Vitest files: `src/features/chat/lib/prepare-chat-attachments.test.ts`, `src/features/chat/components/chat-attachment-restoration.test.ts`, `src/features/chat/components/chat-normalizers.connector-attach.test.ts`, `src/shared/api/chat-client.test.ts`.
- Playwright: `tests/e2e/product-office-outputs.spec.ts`, `tests/e2e/product-readiness.spec.ts` (runner config not read).
- Gateway (root CLAUDE.md): `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (package `verevon-gateway-rs`, from `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml`).
- Model Plane (apps/Model Plane/CLAUDE.md "Testing"): `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets`, and likewise `-p execution-core`, `-p inference-core`, `-p mp-contracts`. Workspace: `cargo test --workspace` (root CLAUDE.md). Crate names read from `rust/services/*/Cargo.toml` and `rust/crates/mp-contracts/Cargo.toml`. Proto regeneration: `buf generate` (apps/Model Plane/CLAUDE.md). User memory: two cargo targets fail on Windows pre-existing, use `--lib`.
- Data Plane documents-api-go: `cd "apps/Data Plane v2/services/documents-api-go" && go test ./...` (DATA_PLANE_DEEP_DIVE.md:441). Data Plane Makefile targets: `make check-rs`, `make test-rs`, `make build-go`, `make test-integration` (root CLAUDE.md).
- imports-core (README.md:486-502): `cd "apps/Ingestion Plane/imports-core" && pytest tests/test_chat_extract.py`; full `pytest tests/`; coverage `pytest --cov=app tests/`. Ingestion root: `make test-endpoints` (root CLAUDE.md).

## 6. Owners

- Root CLAUDE.md, "Architecture Rules" (WT). Data Plane v2 owns documents, chunks, embeddings, retrieval. Ingestion Plane captures evidence and persists durable knowledge through Data Plane contracts only. Model Plane owns reasoning, sessions and runs, inference, execution loops, capabilities and sandboxes. Frontend Plane owns the UI and same-origin gateway or BFF normalization. "No direct database crossing between planes." "Zero Data Retention must propagate through any content-persisting boundary."
- `apps/master-ownership-matrix.md`: :67 Documents service, owner documents-api-go; :112 documents-api-go owns document CRUD, metadata, org-scoped lifecycle, status, internal ingest; :127 inference-core owns speech, vision and doc provider traits where native.
- `apps/Model Plane/docs/capability-ownership-matrix.md`: :21 multimodal done in inference-core `provider/{vision,video,speech,doc_intel,realtime}.rs`; :63 provider routing and multimodal owned by inference-core; :65 sandbox leases owned by sandbox-manager; :66 OS isolation owned by execution-core with sandbox-manager; :42 model-gateway ephemeral stores are caches, not systems of record (governs any phase-2 per-run store); :137 G4 provider capabilities done in inference-core.
- `apps/Model Plane/CLAUDE.md`: "Two tool-dispatch loops ... must stay separate" (code_interpreter crosses from the model-gateway tool loop into execution-core through `tools.rs:163`); "One canonical owner per capability".
- Card plane column versus code: extraction is Ingestion Plane imports-core, not Data Plane. The gateway owns the upload proxy (Frontend Plane BFF). Durable documents are Data Plane documents-api-go. Vision and code_interpreter are Model Plane.

## 7. Uncommitted-only (LIVE) files relevant to F-t3-017

Modified, with `git --no-optional-locks diff --stat` counts in LIVE:

| path | status | diff | one-line summary |
|---|---|---|---|
| features/chat/components/chat-normalizers.ts | M | +39 | adds ZDR refusal presentation helpers (zdrStreamAction, zdrStreamPresentation); attachment functions not in the identifier scan |
| features/chat/components/use-chat-controller.ts | M | +4 | passes `failure.feature` for ZDR refusals |
| features/dashboard/home/DashboardComposer.tsx | M | +7 | wires ChatWorkPeriodPicker props; no attachment change |
| shared/api/chat-client.ts | M | +53 | retrieval-notice and failure-feature typing; uploadChatDocument not in the identifier scan |
| shared/api/chat-client.test.ts | M | +22 | adds a retrieval_notice routing test |
| model-gateway/src/sse.rs | M | +883 (mixed; 16 files total +2217 / -221) | durable-turn and ZDR notice paths; live waiver refusal at :971-979 gates vision |
| model-gateway/src/http_routes.rs | M | +237 (mixed) | create_document gate moved to zdr_posture::zdr_or_refuse with DURABLE_MUTATION_FORBIDDEN (LIVE 7679-7684) |
| model-gateway/src/tool_loop.rs | M | +425 (mixed) | retrieval-notice and rerank-gate additions; code_interpreter arm kept |
| model-gateway/src/dataplane.rs | M | +194 (mixed) | retrieval proxy notices and ZDR refusal mapping |
| model-gateway/src/sse_events.rs | M | +203 (mixed) | RetentionClass; retrieval_notice payload; usage retention class |
| model-gateway/src/chat_turn_registry.rs | M | +36 | private-answer flag for the following turn |
| execution-core/src/grpc.rs | M | +98 (mixed) | waived subscription run admission, tested only in LIVE |
| execution-core/src/runtime_loop/mod.rs | M | +18 | not identified by identifier scan |
| inference-core/src/grpc.rs | M | +131 (mixed) | refusal-catalogue tests in the test module (LIVE 2222-2320); no production change identified |
| proto/model_plane/v1/sessions.proto | M | +42 | CountWaivedRuns RPC and messages; not RunAgentRequest |
| docs/sse-event-taxonomy.md | M | +4 | adds error, plan_proposed, retrieval_notice, source_scope_applied rows |
| docs/ZDR.md | M | 64 (mixed) | not inspected |

Untracked-only (??), relevant:
- Model Plane: model-gateway/src/zdr_posture.rs (decide_for and zdr_or_refuse, used by LIVE sse.rs:967 and http_routes.rs:7679); model-gateway/tests/zdr_posture_matrix.rs; model-gateway/tests/zdr_feature_table.rs; model-gateway/tests/zdr_refusal_inventory.rs; inference-core/src/grpc/zdr_waiver_route_tests.rs; mp-contracts/src/zdr_refusal.rs and retention.rs; api/contracts/zdr-refusals.json (+ .sha256); session-core/migrations/0044_run_retention_receipt.sql, 0045_zdr_waived_runs_org_index.sql, migrations/revert/; session-core/src/retention_receipt.rs.
- Frontend: src/shared/api/zdr-refusals.ts (+ .test.ts); features/chat/components/chat-normalizers.zdr-refusal.test.ts; use-chat-controller.zdr-refusal.test.ts; shared/api/chat-client.zdr-refusal.test.ts; shared/api/http.zdr-refusal.test.ts; shared/i18n/errors.zdr-refusal.test.ts.

Deleted (D) among relevant files: none. The D entries in the status list are product-demo files outside this item.

Clean (not in live status), relevant: prepare-chat-attachments.ts and its test; gateway chat.rs, chat/documents.rs, dispatchers.rs, knowledge/shared.rs, config.rs; action-registry.ts; model-gateway vision.rs, tools.rs, artifacts.rs; execution-core code_interpreter.rs and permission/mod.rs; inference-core provider/mod.rs, vision.rs, doc_intel.rs, anthropic.rs, codex_subscription.rs; gateway.proto, execution.proto, inference.proto; documents-api-go (all); imports-core main.py and chat_extract.py; CHATGAP plan; `apps/Model Plane/docs/chat-parity-audit.md`.

## 8. NOT DETERMINED

1. OD18 owner ruling: none recorded. The 4 MB figure is a recommended default (rules-ledger-adrs.md:258).
2. Runtime OCR behaviour of Azure DI `prebuilt-layout` (doc_intel.rs read only as code, not as behaviour).
3. Phase-2 ephemeral per-run store: no existing attachment store found. `chat_turn_registry.rs` (LIVE-only addition) is unverified as a candidate.
4. Whether the gateway upload proxy checks ZDR before forwarding. documents-api-go enforces at documents.go:304-305. `proxy_data_plane_json` was not read in full.
5. Whether indexing after create is asynchronous end to end. Only the comment at cmd/main.go:204-206 was read.
6. Vision support for providers other than Anthropic (true) and codex subscription (false). The catalogue route behind `listModels` was not traced.
7. Test names for: gateway documents.rs (none found); create_document (none found); vision branch in sse.rs (none found); provider/vision.rs unit test near :642; tool_loop.rs:10646; sse_events.rs; runtime_loop/mod.rs; inference-core G4 "2 tests".
8. Exact WT start line of the code_interpreter arm in tool_loop.rs (WT files_in :2554 and call :2555-2570 were read).
9. Whether the live-only `inference-core/src/grpc/zdr_waiver_route_tests.rs` covers vision refusals (not read).
10. Frontend invocation of the `chat.upload_document` and `chat.extract_document` actions (not searched).
11. Connector-import path in DashboardComposer (WT 5387 extract call) was not traced beyond the call site.
12. External reference claims in the card (claude.ai limits, DeepSeek, T3 Code, Antigravity, Manus) were not re-verified. They are outside a code-map sweep.
13. Whether any SQL migration other than `source_objects` stores attachment bytes (name check only; schemas not read).
14. Whether the reserved field range in execution.proto allows field 18 (only the highest field, 17, was checked).
