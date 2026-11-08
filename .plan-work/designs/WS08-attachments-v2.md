# WS08-attachments-v2 - Attachments v2

Audit item: F-t3-017 (Priority, L, W1). Written 2026-10-15 against worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean) and the live tree `C:/dev/CoresSystem` (same HEAD plus uncommitted work). Paths are relative to the worktree unless marked `[live-only]`. `FE` = `apps/Frontend Plane/verevonv3`, `MP` = `apps/Model Plane`, `RS` = `apps/Model Plane/rust/services`.

## 1. Scope, items resolved, owner

**Item:** F-t3-017 only. Owner plane: Model Plane (model-gateway, inference-core, execution-core). Supporting: Frontend Plane (composer, BFF), Ingestion Plane imports-core and Data Plane documents-api-go for the deferred durable phase only.

**IN**
- Fail closed on every attachment the model would not actually receive (no silent drop, no silent first-image-only, no silent diversion of a Do turn).
- Server-declared attachment policy (caps, types) so UI and server cannot disagree; typed refusals.
- Images as parts of the chat turn (multi-image) on routes that can see them; stop diverting the whole turn to `AnalyzeImage`.
- Bounded, ephemeral, read-only staging of the user's original files into `code_interpreter` (Ask loop and Do loop), including images for Do.
- Raising the stacked size limits safely (three independent caps, section 2.4).

**OUT**
- Durable reference uploads, content-addressed per-org dedup, PDF page images and OCR: designed as an outline only (S8), blocked on OD18 and Data/Ingestion rulings.
- Office deliverable generation (WS19), per-thread sandbox computer (WS22), connected-app picker (WS23), typed file tools (WS24), prompt-injection screening of attachment text (WS26).
- Any change to the inlined-text path for documents (it is the working, tested path; S5 adds bytes beside it, never instead of it).
- Giving attachments to the browser/Quarry loop (Model proposes, Quarry executes; attachments are never a browser-grant input).

| Slice | Tag | Title | Effort |
|---|---|---|---|
| S1 | [FINISH] | Client fail-closed gate on the cases where media is really lost | S-M, 3-4 d |
| S2 | [BUILD] | Server attachment contract: validator, typed refusals, body limits, `attachment_policy` | M, 5-6 d |
| S3 | [BUILD] | inference-core: image parts on `InferRequest`, vision chain filter, per-model vision flag | L, 7-9 d |
| S4 | [BUILD] | model-gateway: images as parts, remove the diversion, client catalogue gate | L, 6-8 d |
| S5 | [BUILD] | Read-only staging into `code_interpreter` (Ask loop first) | L, 8-10 d |
| S6 | [BUILD] | Do loop: attachments on `RunAgentRequest`, images in the Do seed | M-L, 5-7 d |
| S7 | [BUILD] | Raise caps: Home-to-Chat handoff without sessionStorage, body and gRPC limits | M, 4-5 d |
| S8 | [BUILD, deferred] | Durable reference uploads and OCR (outline) | XL, not sized |

## 2. Verified current state

### 2.1 What exists (opened)

| Fact | Evidence |
|---|---|
| Browser reads every attachment before the draft clears: 1,000,000-byte gate runs before the image branch; PDF/DOCX/XLSX/PPTX text comes from server extraction; text types are read locally; anything else throws `unsupported`; 60,000 chars per file, 120,000 per turn | `FE/src/features/chat/lib/prepare-chat-attachments.ts:18-35` |
| Documents are inlined into the user message as `--- VEDLEGG: name ---` fences; only `image/*` survives into the stream attachments, as `{kind:'image', data_base64, mime_type}` | `FE/src/features/chat/components/chat-normalizers.ts:1147-1202` (WT; live `:1186-1241`, file is `M` in live) |
| `StreamAttachment` can only be an image | `FE/src/features/chat/components/chat-types.ts:505-509` |
| Subscription route drops every attachment silently | `FE/src/shared/api/chat-client.ts:1114` (WT; live `:1144`) `attachments: subscriptionBacked ? [] : request.attachments ?? []` |
| `ModelInfo.capabilities` is filled from the catalogue `features` list; nothing gates on `'vision'`. The only gating precedent is `modelSupportsAgentic` | `chat-client.ts:849-852`, `:2471`, `:2571`; `DashboardComposer.tsx:1136` |
| The Verevon modes (`verevon-budget/balance/genius`) are NOT catalogue entries; the server resolves them. `selectedCatalogModel()` is `undefined` for the default selection | `chat-client.ts:816-833`; `DashboardComposer.tsx:1115-1127` |
| Composer attach: `addFiles` keeps every file with no type/size/route check; static `accept` list on the input | `DashboardComposer.tsx:1695-1725` (WT), `:3112` (WT) |
| Home-to-Chat handoff serializes the whole launch (data URLs included) into `sessionStorage`, rejecting above 2,000,000 chars | `FE/src/features/chat/lib/pending-chat-launch.ts:7,66-97` |
| BFF chat stream route takes `Json<Value>`; model-gateway `/v1/invoke/stream` takes `axum::Json<InvokeRequest>`. No `DefaultBodyLimit` override exists on either (only the audio routes have one), so axum's default JSON limit applies | `FE/apps/gateway/src/domains/chat/streams.rs:18-23`; `RS/model-gateway/src/sse.rs:835-846`; grep `DefaultBodyLimit` hits only `domains/ai.rs`, `http_routes.rs:422-426` |
| tonic servers set no message-size limits (grep for `max_decoding_message_size` is empty); default decode limit 4 MiB. `MAX_IMAGE_BYTES` 20 MB in inference-core is therefore unreachable over gRPC today | `RS/inference-core/src/grpc.rs:40,1181`; `RS/execution-core/src/grpc.rs:1434` |
| Any image attachment diverts the whole turn: `select_image` (first image only) then `vision_stream` (one `AnalyzeImage` call, the user text is the prompt; no history, tools, memory, grounding). It runs before the agentic branch, so a Do turn with an image never reaches the agent | `RS/model-gateway/src/sse.rs:1341-1355` (vision), `:1381` (agentic), `:5646` (`vision_stream`); `RS/model-gateway/src/vision.rs:52-70` |
| `InvokeAttachment` has `kind`, `url`, `data_base64`, `mime_type` and no name; a `kind:"file"` is ignored by `is_image` | `RS/model-gateway/src/vision.rs:14-46` |
| ZDR turns refuse any attachment before a run exists | `sse.rs:976-977` (WT) |
| Chat completion messages are text only: `ChatMessage{role, content, name, compaction_summary}` (tags 1-4); OpenAI and Anthropic adapters build `"content": m.content`; Anthropic indexes messages after filtering out `system` | `MP/proto/model_plane/v1/inference.proto:198-211`; `RS/inference-core/src/provider/openai.rs:411-429`; `anthropic.rs:670-709` |
| The vision flag is per provider, not per model: `feature_flags()` projects `supports_vision`; OpenAI/Azure `OpenAi`/`Azure` flavors say true, `UnifiedAzureAi` false; Anthropic true; ChatGPT subscription false | `provider/mod.rs:669-687`; `openai.rs:610-650,1040-1070`; `anthropic.rs:933`; `codex_subscription.rs:612,871-885` |
| Provider chain skips providers on ZDR, privacy tier and residency before any network call, in three places | `provider/fallback.rs:1283-1297` (`infer_one_model`), `:1619-1632` (`stream_one_model`), `:1783-1798` |
| `/v1/models` returns `{models:[{id,provider,modality,streaming,features,privacy_tier,residency}]}` and the BFF relays it verbatim | `RS/model-gateway/src/http_routes.rs:7451-7527`; `FE/apps/gateway/src/domains/chat/json_handlers.rs:452-476` |
| `code_interpreter` `files_in` is model-authored `{name, content_b64}`, validated flat, written into the WRITABLE workspace root. Workspace is deleted on every path (Drop guard) | `RS/execution-core/src/code_interpreter.rs:133-145,179-264,329-345,430-565` |
| The sandbox already has a read-only layering seam, `LaunchOptions.extra_ro_binds` (emitted after the writable binds), but no caller passes it and `execute_sandboxed_in_dir` builds `LaunchOptions` without it | `RS/execution-core/src/sandbox.rs:95-112,202-210`; `executor.rs:250-263`; grep for `extra_ro_binds` outside `sandbox.rs` is empty |
| Both loops reach the same `code_interpreter` arm: gateway loop through `ExecuteStep` (`tools.rs:163-260`, arm at `tool_loop.rs:2554`), Do loop through `execute_step_inner` (21 parameters) | `RS/model-gateway/src/tools.rs:163`; `RS/execution-core/src/runtime_loop/mod.rs:455-505,596-623,891-904` |
| `ExecuteStepRequest` highest tag 11 (`space_id`); `RunAgentRequest` highest tag 17 (`subscription_connection_id`), no `reserved` | `MP/proto/model_plane/v1/execution.proto:268-318,76-209` |
| The durable text route `POST /api/v1/chat/documents` goes BFF to documents-api-go, not model-gateway (that forward is dead); ZDR refusal is 403 there; create is documented synchronous | `FE/apps/gateway/src/domains/chat/documents.rs:43-53`; `apps/Data Plane v2/services/documents-api-go/internal/handler/documents.go:304-305` |
| imports-core `POST /api/v1/import/extract` is ephemeral (no storage, no model call), 1,000,000 decoded bytes, 60,000 chars, and rejects scanned files ("Scanned files require a text version") | `apps/Ingestion Plane/imports-core/app/main.py:204-212`; `app/chat_extract.py:8-50` |
| inference-core `doc_intel.rs` is Azure Document Intelligence `prebuilt-layout`; no OCR engine named | `RS/inference-core/src/provider/doc_intel.rs:1-18` |

### 2.2 Live-only files (uncommitted in `C:/dev/CoresSystem`) that this stream touches or depends on

| File | Why it matters | Must land first |
|---|---|---|
| `RS/model-gateway/src/zdr_posture.rs` [live-only] | `decide_for`; live `sse.rs:957-979` refuses vision on waived turns (`WAIVER_ROUTE_MISMATCH`) | WS01 commits the ZDR wave |
| `RS/model-gateway/src/zdr_features.rs` [live-only] | Table over every `InvokeRequest` field, destructured without `..`: a new `InvokeRequest` field stops the build until classified; `attachments` is `refused_feature` (`:128`, `:296-297`) | WS01 |
| `RS/model-gateway/tests/zdr_feature_table.rs`, `zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs` [live-only] | The golden cases this stream extends | WS01 |
| `MP/api/contracts/zdr-refusals.json` [live-only] (line 77 names `attachments`) | typed refusal catalogue | WS01 |
| Modified in live: `RS/model-gateway/src/sse.rs` (+883), `http_routes.rs`, `tool_loop.rs`, `sse_events.rs`, `FE/src/features/chat/components/chat-normalizers.ts`, `use-chat-controller.ts`, `DashboardComposer.tsx`, `FE/src/shared/api/chat-client.ts` | All my slices edit these. Line numbers differ (live +60 to +70 in `sse.rs` before the vision block: live vision `:1409-1423`) | WS01, then thin diffs |

### 2.3 Where the audit is stale or wrong (code wins)

1. **"Phase 1 is client-side: read the vision flag, disable image attach on routes without it" is incomplete and, as written, harmful.** Today the selected chat model does not see the image at all: `AnalyzeImage` serves it on the platform vision chain regardless of the picked model. Gating attach on the catalogue flag now would block images that work (an Azure `UnifiedAzureAi` model has no flag but its turn is still served by AnalyzeImage). The flag becomes meaningful only after S4 makes the picked model the reader. Meanwhile the real losses are: subscription route (silently dropped), Do (diverted away from the agent), more than one image (silently ignored), ZDR (refused after the user spent the send).
2. **"Send images as parts" needs a wire change the card does not name**: `InferRequest` has no image field and `ChatMessage` has no parts (S3). `gateway.proto` `InvokeRequest.content_parts = 16` and `attachments = 18` have no consumer; the live path is the Rust `InvokeAttachment`. `docs/chat-parity-audit.md:243` claim that content_parts route to vision is unsupported by code (sweep).
3. **The default model has no catalogue entry**, so a client-side catalogue gate cannot answer for `verevon-balance`. The server must declare it (`attachment_policy`, S2).
4. **Vision flag is per provider**, not per model (table above). A real per-model flag needs inference-core work (S3).
5. **OD18 is not a ruling.** `.plan-work/common/rules-ledger-adrs.md:258,272`: recommended default only (Tesseract, 4 MB for xlsx/pptx/pdf after the H1 zip guard, 60k chars stays), "No ruling found". Treat 4 MB as unratified.
6. **A 4 MB cap is not a constant change.** Three more caps stack under it: sessionStorage handoff 2,000,000 chars (`pending-chat-launch.ts:7`), axum 2 MiB JSON body (BFF and gateway), tonic 4 MiB gRPC (`ExecuteStep`, `AnalyzeImage`, `RunAgent`). A 1 MB image is 1.33 MB base64: two images already exceed the handoff and body caps today (a two-image Home send fails `too_large`, a chat-page send 413s; not reproduced, derived from the constants).
7. **Durable route facts** (card row): extraction is Ingestion Plane imports-core, not Data Plane; the active upload is BFF to documents-api-go; create is synchronous; the 412 sits in a dead handler; `uploadChatDocument` has no TS caller but the `chat.upload_document` action (`FE/src/shared/actions/action-registry.ts:2904-2913`) reaches the same handler (`dispatchers.rs:4758-4779`).
8. **A subscription-pinned request that does carry an image would be analysed on the platform chain** (server has no route check before the vision branch). Only the browser stops it. This is a data-egress gap, fixed in S2.
9. **`unsentAttachmentNames`** (`chat-normalizers.ts:1098`, WT) has no callers and describes pre-inlining behaviour; delete or repurpose in S1.
10. Two-loop boundary holds: the gateway loop already delegates `code_interpreter` to execution-core (`tools.rs:163`). Staging read-only user files adds no effect to the gateway loop.

### 2.4 Cap map (today and target)

| Layer | Today | Where | Target (S2/S7, values env-configurable) |
|---|---|---|---|
| Per file in browser | 1,000,000 B | `prepare-chat-attachments.ts:18` | server-declared (`attachment_policy.max_file_bytes`) |
| Handoff sessionStorage | 2,000,000 chars total | `pending-chat-launch.ts:7` | in-memory/IndexedDB handoff (S7) |
| BFF + gateway JSON body | axum default 2 MiB | `streams.rs:18`, `sse.rs:846` | `DefaultBodyLimit` 12 MiB on the two chat stream routes |
| gRPC to execution-core and inference-core | tonic 4 MiB | server builders | 16 MiB decode on those two services |
| Text per doc / per turn | 60,000 / 120,000 chars | `prepare-chat-attachments.ts:33-35`; `chat_extract.py:12` | unchanged (OD18 says stays) |
| Images per turn | 1 effective | `vision.rs:52` | 4 |
| Sandbox files per turn | n/a | | 3, total 3 MiB (until OD18 ruled) |

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | OD18 caps (4 MB, OCR engine) | (a) wait; (b) ship under today's 1 MB, caps server-declared so a ruling is a config flip | (b). Do not start S7/S8 sizing until the owner rules. | S7, S8 only. S1-S6 proceed. |
| D2 | Where the "ephemeral per-run store" lives | (a) bytes ride the requests (`ExecuteStepRequest`, `RunAgentRequest`); execution-core holds an in-memory run-scoped registry; (b) execution-core pulls from a model-gateway store | (a). No new execution-core to model-gateway edge, no cross-replica state for Ask (bytes arrive with the step), Do holds them in the process that runs the loop. Gateway keeps only request-scope memory. Honors capability matrix `docs/capability-ownership-matrix.md:42` (caches, not systems of record). Deviates from the audit wording "resolved by model-gateway from an ephemeral store": gateway validates, hashes and resolves; execution-core stages. | S5, S6 |
| D3 | Field on `RunAgentRequest` or on `RunContext` | (a) dedicated `RunAgentRequest.attachments`; (b) `RunContext.attachments = 5` (WS02 reserved 5+, `.plan-work/designs/WS02-run-context-cache.md:205,389`) | (a). Scope note asks for it, and `RunContext` is validated and flag-gated by WS02 S6; attachments must not wait for or share that flag. Integrator assigns the tag (proposal 20, after WS02=18 and WS03=19). One-line proto change if the integrator picks (b). | S6 |
| D4 | ZDR and attachments | (a) keep refusal, add pre-send explanation; (b) in-memory images on ZDR | (a). No attested provider exists yet (F-t3-005); `docs/ZDR.md:97` documents attachments as refused. Revisit only after WS01 attests a provider. No ledger entry is reversed. | S2, S4 clause |
| D5 | Fate of `AnalyzeImage` diversion | (a) delete; (b) keep as explicit fallback | (b): used only when `CHAT_VISION_AS_PARTS_ENABLED` is off or no vision-capable chat provider is configured, and never for Do/agentic. Delete after one release. | S4 |
| D6 | Capability truth for Verevon modes | (a) client guesses; (b) server declares in `/v1/models` | (b): `attachment_policy.modes.vision` computed by the gateway from whether inference-core lists at least one vision-capable provider. | S2, S4 |
| D7 | Subscription route and files | refuse all attachments (route stays narrow, matches `chat-client.ts` comment at the `agentic` block) vs allow | Refuse, typed `attachment_route_unsupported`; no silent drop. | S2 |
| D8 | Follow-up turns on the same file | (a) re-attach each time (v1); (b) keep bytes per thread | (a). Bytes per thread is the durable phase (S8). Inlined text already persists in the thread. State this limit in the UI copy. | none |
| D9 | New flags polarity | repo has six truthy vocabularies (`flags-and-config.md` section 1.2) | Security-class refusals (S2): opt-out vocabulary A (unset = ON). Capability toggles (S4, S5): opt-in vocabulary C, default off. | S2, S4, S5 |

Slices not needing any decision: S1, S2, S3. Needs ratified field numbers (integrator): S3 (`InferRequest`), S5 (`ExecuteStepRequest = 12` verified free), S6 (D3).

## 4. Slices (build order)

### S1. Client fail-closed gate on the cases where media is really lost [FINISH] (S-M, 3-4 d)

**Goal.** The user is told before send, and the send is refused locally, whenever the attachment would not reach the model: subscription route (any attachment), Do mode with an image, more than one image, ZDR/temporary chat, unsupported type, over-cap size. Nothing is silently dropped. No catalogue gate yet (section 2.3 item 1).

**Files**
- NEW `FE/src/features/chat/lib/attachment-policy.ts` - pure `evaluateAttachment(file, ctx): {ok:true} | {ok:false, reason}`; `ctx = {subscriptionBacked, planMode, zdr, currentImageCount, currentTotalBytes}`; closed reasons `unsupported_type | too_large | route_unsupported | do_images_unsupported | too_many_images | zdr_refused`; constants mirror today's caps (1,000,000 bytes, 1 image) in one place.
- `FE/src/features/dashboard/home/DashboardComposer.tsx` - `addFiles` (`:1695`) runs `evaluateAttachment` per file; rejected files do not enter `files`; a polite inline notice (i18n NO/EN via `i18n.tr`) names file and reason. `subscriptionBacked()` (`:1005`) and the Do toggle feed `ctx`; when Do is turned on with images pending, show the same notice and keep the images listed as blocked. Do not touch the `accept` string except to keep it in sync with the policy constant.
- `FE/src/shared/api/chat-client.ts` - replace the silent `subscriptionBacked ? [] : ...` (`:1114`, live `:1144`) with a thrown typed local error `attachments_not_sent` when attachments are present and the route cannot carry them (belt and braces for non-composer callers). Subscription body otherwise unchanged.
- `FE/src/features/chat/components/chat-normalizers.ts` - delete dead `unsentAttachmentNames` (`:1098` WT / `:1137` live) or reuse it for the notice; add `attachments_not_sent` to `streamFailurePresentation` (`:386`).
- `FE/src/shared/i18n/errors.ts` / `no.ts` / `en.ts` - strings for the six reasons.

**Contracts:** none on the wire. **Flag:** none (client behaviour; the repo has no VITE feature flags for server features). Rollback = revert. **ZDR clause:** nothing persisted; `zdr_refused` explains the existing server refusal before the send. Add the reason to the golden FE test that lists ZDR-refused features (`FE/src/shared/api/zdr-refusals.ts` and its `.test.ts` [live-only]; extend after WS01 lands).

**Tests**
- NEW `FE/src/features/chat/lib/attachment-policy.test.ts`: `a_subscription_turn_refuses_every_attachment`, `a_do_turn_refuses_images`, `a_second_image_is_refused_not_ignored`, `a_zdr_turn_refuses_attachments_before_send`, `an_oversize_file_names_the_file`, `an_unknown_extension_is_refused_with_its_name`.
- Extend `FE/src/shared/api/chat-client.test.ts` (existing "routes a connected subscription without incompatible tool features", WT `:504-537`): the image is no longer dropped silently, it throws `attachments_not_sent`.
- Extend `features/chat/lib/prepare-chat-attachments.test.ts` only if caps move (they do not).
- Run: `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/features/chat/lib/attachment-policy.test.ts src/shared/api/chat-client.test.ts src/features/chat/lib/prepare-chat-attachments.test.ts && pnpm typecheck && pnpm lint`. Memory notes pre-existing typecheck/lint/vitest failures in this tree: diff failing sets against HEAD, do not trust exit codes.
- Playwright (NEW, `tests/e2e/chat-attachment-gates.spec.ts`): pick the ChatGPT subscription model (stubbed catalogue), attach a PNG, assert the notice and an empty attachment list. Command: `pnpm test:e2e -- tests/e2e/chat-attachment-gates.spec.ts` (runner config not read; baseURL assumption must be confirmed).

**DoD:** none of the six cases reaches the wire; each shows a localized explanation; `unsentAttachmentNames` gone or used. **Evidence:** a second Vite dev server over the worktree (memory: :5173 is the main checkout and needs a restart for bind-mounted edits); screenshots of each of the six refusals and a network capture showing no `/api/v1/chat/stream` request was made. **Deps:** none; WS01 landing of `chat-normalizers.ts`/`DashboardComposer.tsx` first (live-modified). **Risks:** UI copy collisions with the ZDR presentation helpers already added live (`zdrStreamPresentation`); keep reasons separate.

### S2. Server attachment contract [BUILD] (M, 5-6 d)

**Goal.** One server-owned validator and one server-declared policy. The server stops being trusting: a subscription-pinned turn with attachments is refused (closing the platform-chain egress, 2.3 item 8), oversize/unknown/too-many are typed refusals, and the chat routes can carry the declared sizes.

**Files**
- NEW `RS/model-gateway/src/attachment_policy.rs` (declare in `lib.rs`) - `AttachmentPolicy::from_env()` (consts `CHAT_ATTACHMENT_POLICY_ENABLED` opt-out, `CHAT_ATTACHMENT_MAX_IMAGES`=4, `_MAX_IMAGE_BYTES`=1_000_000, `_MAX_FILES`=3, `_MAX_FILE_BYTES`=1_000_000, `_MAX_TOTAL_BYTES`=3_145_728 as `pub const ..._ENV: &str`, per the neighbour pattern `execution-core/src/subagent/research_fan_out.rs:42`); `validate(&InvokeRequest, route) -> Result<ValidatedAttachments, AttachmentRefusal>`; closed codes `attachment_media_unsupported`, `attachment_too_large`, `attachment_count_exceeded`, `attachment_route_unsupported`, `attachment_encoding_invalid`; allowed image mimes `image/png|jpeg|webp|gif`; sanitized names via the same rules as `code_interpreter` `validate_input_name`.
- `RS/model-gateway/src/vision.rs` - `InvokeAttachment` += `name: Option<String>` (serde default); `kind:"file"` recognized (not yet consumed). Existing `select_image` stays.
- `RS/model-gateway/src/sse.rs` - call `validate` early in `invoke_stream_sse` (before run creation; live `:957` region, after the posture decision `decide_for` so ZDR/waiver refusals keep priority and their typed catalogue). Refusal returns `error_stream(code, message, retryable=false)`.
- `RS/model-gateway/src/http_routes.rs` - `ListModelsHttpResponse` (`:7451`) += `attachment_policy` sibling of `models`: `{image_mimes[], max_images, max_image_bytes, max_files, max_file_bytes, max_total_bytes, modes:{vision:bool, sandbox_files:bool}}` computed from env and, for `modes.vision`, from whether inference-core lists a provider with the `vision` feature (D6). Additive; BFF relays verbatim. Add `DefaultBodyLimit::max(CHAT_STREAM_BODY_LIMIT_BYTES)` (12 MiB, env `CHAT_STREAM_BODY_LIMIT_BYTES`) to the `/v1/invoke/stream` route (`:137`).
- `FE/apps/gateway/src/domains/chat.rs:17` and `streams.rs:18` - same body limit layer on `/api/v1/chat/stream` (precedent `domains/ai.rs:35-56`). BFF stays allowlist-free and byte-verbatim (audit section 8).
- `FE/src/shared/api/chat-client.ts` - `listModels` (`:2443`) additionally exposes `getAttachmentPolicy()` (parses the sibling; absent = today's constants); S1's `attachment-policy.ts` reads it instead of constants.
- `FE/src/features/chat/components/chat-normalizers.ts` - `streamFailurePresentation` cases for the five codes.
- `MP/docs/sse-event-taxonomy.md` - error-code rows (no new event variant); run `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (CI `sse-taxonomy.yml`).
- `RS/model-gateway/src/zdr_features.rs` [live-only] - `attachments` stays `refused_feature`; no new `InvokeRequest` field is added in S2 (only a field on the nested `InvokeAttachment`), so the table does not change. If the table test enumerates nested fields, classify `name` as inert-under-ZDR.
- `deploy/docker-compose.yml` (`MP/deploy/docker-compose.yml`, model-gateway block `:191-711`) - one pass-through line per env var, empty default.

**Flag:** `CHAT_ATTACHMENT_POLICY_ENABLED` (opt-out, default ON; unset enforces). Rollback: set `0`. The body-limit raise has no flag; rollback by image.
**ZDR clause:** ZDR/waived decisions stay first; nothing new is persisted; validator logs counts, mimes and sizes only, never names or bytes. Add table rows to `zdr_feature_table.rs` [live-only]: (attachment image, ZDR) and (attachment file, waived) both refuse with the existing typed codes, and a new row (attachment, subscription, non-ZDR) expects `attachment_route_unsupported`.
**Tests**
- NEW unit tests in `attachment_policy.rs`: `subscription_route_with_any_attachment_is_refused`, `fifth_image_is_refused`, `oversize_image_names_limit`, `unknown_kind_is_refused_not_ignored`, `bad_base64_is_refused_not_skipped` (today `select_image` returns `None` and proceeds as plain chat: behaviour change, document it), `policy_off_restores_current_behaviour`.
- NEW `RS/model-gateway/tests/attachment_policy_contract.rs`: body of 5 MiB reaches the handler (not 413); 13 MiB gets 413.
- BFF: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` with a NEW test in `domains/chat/streams.rs` tests asserting the limit layer.
- Run: `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets` (memory: two cargo targets fail on Windows pre-existing, use `--lib` when needed).
**DoD:** all refusals typed and localized; `/v1/models` carries `attachment_policy`; a 5 MiB body passes both hops. **Evidence:** rebuild `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build model-gateway` and the BFF image (Frontend gateway recipe per memory); curl the stream route with (a) a subscription provider plus an image, expect `attachment_route_unsupported` and no run row in session-core, (b) a 5 MiB body, expect 200; capture `/api/v1/models` JSON.
**Effort/deps:** WS01 (live files), WS10 shares the `/v1/models` response (rule: WS10 edits the `models` array, WS08 only the sibling key). **Risks:** raising body limits widens memory per request (12 MiB cap, bounded concurrency is the existing gateway limit, not verified); the `attachment_encoding_invalid` change turns silent text-only fallback into an error (intended).

### S3. inference-core: image parts, vision filter, per-model flag [BUILD] (L, 7-9 d)

**Goal.** `InferRequest` can carry images for specific messages; only vision-capable providers/models serve such a request, fail closed otherwise; the catalogue says which models see images.

**Files**
- `MP/proto/model_plane/v1/inference.proto` - `InferRequest` (`:100`, tags 1-16 used; 17 WS11, 18-19 WS02 per `WS02...:109`) += `repeated InferImagePart image_parts = 20;` (integrator confirms); NEW `message InferImagePart { uint32 message_index = 1; bytes data = 2; string mime_type = 3; }`. Side channel rather than `ChatMessage` parts: `ChatMessage` literal sites number ~156 (WS02 sweep) and it avoids touching all of them. `message_index` indexes the ORIGINAL `messages` list (Anthropic filters `system` out; adapters must map before filtering). Bytes only, no URL (no provider-side fetch of client URLs).
- `RS/inference-core/src/provider/mod.rs:120-165` (internal `InferRequest`) += `image_parts: Vec<ImagePart>`; `grpc.rs` maps it; validate: at most 4, each <= `MAX_IMAGE_BYTES`, mimes allowlisted, `message_index` in range and role `user`.
- `provider/openai.rs:411-429` (`build_request_body`) - for a message with images emit `content: [{type:"text",text}, {type:"image_url", image_url:{url:"data:<mime>;base64,<...>"}}...]`; unchanged when no parts. `provider/anthropic.rs:670-709` - `content: [{type:"image", source:{type:"base64", media_type, data}}, {type:"text", text}]`, and compaction/summary branch untouched.
- `provider/fallback.rs:1283,1619,1783` - gate: `if !req.image_parts.is_empty() && !model_accepts_images(&caps, model) { continue }`; when no provider was attempted, fail closed with a typed `FAILED_PRECONDITION` reason `vision_unsupported` (pattern `tier_unavailable_error`, `:1222-1223`). Subscription provider (`codex_subscription.rs:612`) is excluded by its flag.
- `provider/mod.rs:669-687` + `openai.rs:1040-1070` - per-model vision: `model_accepts_images(caps, model_id)`: provider flag AND NOT in a conservative denylist (`deepseek*`, `o1-mini`, `gpt-3.5*`, embedding/rerank ids); `list_models` emits `vision` per model from it. Unknown ids on a vision provider stay true (same as today's provider-level truth); the denylist is the only new knowledge and lives in one function with a test table.
- Server builder `grpc.rs:1181` (`InferenceCoreServer::new(...)`) - `.max_decoding_message_size(16 MiB)`.
- Go bindings regenerate (`buf generate`); regenerate `proto/baseline.binpb` only through the repo's breaking-change flow. Additive only.

**Flag:** none; inert until a caller sets the field. **ZDR clause:** providers are still skipped when `zdr` and not attested; image bytes are request-scope only: confirm no response cache or request logging includes `image_parts` (inference-core logs request ids; add a test that the `Debug` of the internal request redacts `data`, as the ZDR body-cache tests do for Anthropic `cache_control`, `anthropic.rs` test "zdr_request_must_not_send_cache_control_anywhere_in_the_body"). Golden case: `zdr_waiver_route_tests.rs` [live-only] gains (waived, image parts) refused.
**Tests:** `cargo test -q -p inference-core --lib`: `openai_body_emits_image_url_part_for_indexed_message`, `anthropic_body_indexes_before_system_filter`, `image_request_skips_non_vision_provider`, `image_request_with_no_vision_provider_fails_closed_typed`, `no_image_parts_is_byte_identical_to_today` (existing body tests must pass unchanged), `denylisted_model_does_not_advertise_vision`; `buf lint` has pre-existing debt, run `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb`.
**DoD:** a unit-level two-image request is served by the right adapter shape and refused where it cannot be. **Evidence:** rebuild inference-core; `grpcurl`-style call (or the gateway e2e harness `tests/e2e_invoke_chain_test.rs`, mock servers at `:603,:835,:1001`) with two images against a live Azure/OpenAI vision deployment; capture the provider-bound body shape from a debug test, never bytes in logs. **Deps:** WS11/WS02 tag allocation (D8 in WS02). **Risks:** provider vision payload quirks per Azure api-version; denylist drift (mitigate: per-model override env `INFERENCE_VISION_MODEL_DENYLIST_EXTRA`).

### S4. model-gateway: images as parts, remove the diversion, client catalogue gate [BUILD] (L, 6-8 d)

**Goal.** An Ask turn with images is an ordinary chat turn on the picked model: history, tools, memory, grounding, citations all apply; every image (up to the cap) is seen; `AnalyzeImage` is only the fallback.

**Files**
- NEW `RS/model-gateway/src/vision_parts.rs` - `ImageParts::from_validated(&ValidatedAttachments)`; `attach(&mut InferRequest, &ImageParts, user_index)`; helper that records the current user turn's index in the finalized `history`.
- `RS/model-gateway/src/sse.rs:1341-1355` (live `:1409-1423`) - when `CHAT_VISION_AS_PARTS_ENABLED` and not agentic and a vision-capable route exists: do NOT call `vision_stream`; carry `ImageParts` down. Else existing `vision_stream`. For a Do turn with images: refuse with `attachment_do_unsupported` (typed) until S6. At the final answer `InferRequest` (`:2546`, struct literal with `..Default::default()`) set `image_parts`.
- `RS/model-gateway/src/tool_loop.rs:6334` (`run_tool_rounds_for_model`) - new parameter `images: &ImageParts`; every round `InferRequest` (`:6648`, `:6938`) carries it. Invariant: rounds only append to `messages`, so `user_index` stays valid; add `debug_assert!` on the prefix length plus a regression test.
- `RS/model-gateway/src/vision.rs` - unchanged `select_image` remains for the fallback; mark with a comment pointing at D5.
- model catalogue gate (FE): `FE/src/shared/api/chat-client.ts` += `export const VISION_CAPABILITY = 'vision'` and `modelSupportsVision(model|undefined, policy)` next to `modelSupportsAgentic` (`:2571`): explicit catalogue model => its flag; Verevon mode => `attachment_policy.modes.vision`; unknown => false while the catalogue has not answered (same stance as `zeroRetentionAvailable`, `DashboardComposer.tsx:1105-1110`, `undefined` while loading).
- `FE/src/features/dashboard/home/DashboardComposer.tsx` - feed `modelSupportsVision(selectedCatalogModel(), policy)` into `evaluateAttachment` (new reason `model_no_vision`); the attach button stays enabled for documents; images are refused with "this model cannot read images; pick X" naming one vision model from the catalogue.
- `FE/src/features/chat/components/chat-normalizers.ts` `toStreamAttachments` (`:1192`) - multi-image with per-turn byte budget; `chat-types.ts:505` `StreamAttachment` stays image-only in S4.
- `MP/docs/sse-event-taxonomy.md` - error-code rows; `usage` frame unchanged.

**Flag:** `CHAT_VISION_AS_PARTS_ENABLED` (opt-in, default off; compose pass-through line). Rollout: staging on, verify, flip default in compose. Rollback: unset; the fallback path is today's behaviour. **ZDR clause:** unchanged refusal (`sse.rs:976-977`; live waived refusal at `:971-979`); images are request-scope memory only, never in session-core, events, cache or artifacts; the persisted user message still carries text only (image does not survive reload; thread follow-ups cannot re-read it: D8 copy). Golden cases: add `(Ask, image, ZDR)`, `(Ask, image, waived)`, `(Ask, image, persistent)` rows to `zdr_posture_matrix.rs` [live-only] and `api/contracts/zdr-posture-matrix.json` if that golden enumerates features (not opened; `zdr-contract-sync.test.sh` is the check).
**Tests**
- NEW `RS/model-gateway/tests/vision_parts_chain_test.rs` (pattern `e2e_invoke_chain_test.rs`): two images produce an `InferRequest` with two `image_parts` at the user index; tool round appends do not move the index; history and tools still reach the model; flag off keeps `vision_stream`.
- `vision.rs` tests (`selects_image_by_url :96`, `decodes_inline_base64_and_strips_data_url_prefix :104`, etc., per sweep) stay green.
- FE: extend `attachment-policy.test.ts` (`a_non_vision_catalogue_model_refuses_images`, `a_verevon_mode_follows_the_declared_policy`, `an_unloaded_catalogue_blocks_images`).
- Run: `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --all-targets`; `bash scripts/tests/zdr-contract-sync.test.sh`; FE vitest as S1.
**DoD:** Balance + two images + a follow-on tool question returns one grounded answer; a Do turn with an image is refused with the typed code; the picker explains non-vision models. **Evidence:** rebuild model-gateway, inference-core and the BFF; in the running stack send a 2-image Ask turn on `verevon-balance` and on one explicit Anthropic model; capture the SSE (chunk/usage/done), the persisted user message (text only), session-core run row (non-ZDR), and the 4xx for the Do case. Repeat with Temporary chat (ZDR) and capture the typed refusal.
**Deps:** S2, S3. **Risks:** `sse.rs` and `tool_loop.rs` are live-modified and hot (WS01/WS02/WS03/WS27): new module plus thin diffs; token cost of images (WS27 meter must count them; vision tokens arrive in `usage`); an image without text.

### S5. Read-only staging into `code_interpreter`, Ask loop [BUILD] (L, 8-10 d)

**Goal.** "Analyse this spreadsheet" works: the original bytes (not only the extracted text) appear read-only at `attachments/<name>` inside the hermetic sandbox, with no network, and are gone after the call.

**Files**
- `MP/proto/model_plane/v1/execution.proto` - NEW `message RunAttachment { string id = 1; string name = 2; string mime = 3; uint64 size_bytes = 4; string sha256 = 5; bytes content = 6; RunAttachmentKind kind = 7; }`, `enum RunAttachmentKind { RUN_ATTACHMENT_KIND_UNSPECIFIED = 0; RUN_ATTACHMENT_KIND_FILE = 1; RUN_ATTACHMENT_KIND_IMAGE = 2; }`; `ExecuteStepRequest` (`:268-318`, verified highest tag 11) += `repeated RunAttachment attachments = 12;`.
- NEW `RS/crates/mp-contracts/src/attachments.rs` (+ `pub mod attachments;` in `lib.rs`) - shared pure contract: caps consts, `validate_name`, `sandbox_note(&[RunAttachment]) -> String` ("Files attached, read-only, in code_interpreter at attachments/NAME ..."), sha256 check. Pure, used by both loops; no dispatch shared (two-loop rule).
- NEW `RS/model-gateway/src/turn_attachments.rs` - holds `ValidatedAttachments` for the turn (request-scope), computes sha256, builds `RunAttachment`s; the gateway composes the sandbox note server-side from validated names (never from client text).
- `RS/model-gateway/src/tool_loop.rs:2554` (arm) and `tools.rs:163-260` (`handle_code_interpreter`) - pass `&[RunAttachment]` into `ExecuteStepRequest.attachments`; the model cannot add or alter them (its `files_in` stays as is). The `code_interpreter` tool description gains one line about `attachments/`; builtin tool def location not opened (`tool_loop::builtin_tool_defs` per `FE chat-client.ts` comment).
- NEW `RS/execution-core/src/run_inputs.rs` - `RunInputs` in-memory registry keyed `(org_id, run_id)`: bounded (runs, bytes), TTL sweep, refcounted RAII guard so parallel steps share and the last drop zeroes and removes. `ExecuteStep` handler registers for the call duration; the Do loop registers for the run (S6). No signature change to `execute_step_inner`: the `code_interpreter` arm reads the registry by `run_id` (`runtime_loop/mod.rs:596-623`). Nothing in tool_input can name them.
- `RS/execution-core/src/code_interpreter.rs` - `CodeRequest` += `ro_inputs: Vec<InputFile>`; `run_in_root` (`:430`) creates `<root>/attachments/`, writes files mode 0444 inside a 0555 dir, and passes the `(host, mount)` pair through `executor::execute_sandboxed_in_dir` into `LaunchOptions.extra_ro_binds` (`sandbox.rs:112`, emitted after the writable bind, `:202-210`); `attachments` becomes a reserved name in `validate_input_name` (`:227`); output collection (`:555`) already returns only top-level regular files so the directory is never returned. If the host has no bwrap (passthrough), refuse staging rather than weaken the claim: `require_requested_isolation` pattern (`executor.rs:219-226`). Persistent Space root: the empty mountpoint directory may remain in the CAS snapshot, harmless.
- `RS/execution-core/src/executor.rs:250` - `execute_sandboxed_in_dir` gains `ro_binds: &[(PathBuf,PathBuf)]`; update its callers (`code_interpreter.rs` only per grep) .
- `RS/execution-core/src/grpc.rs:1434` - `.max_decoding_message_size(16 MiB)`.
- Capability binding unchanged: `code_interpreter` stays `cap.command.sandbox` (low risk, `code_interpreter.rs:14-18`); no new capability, no gate bypass (WS05 consumes the declaration).
- FE: `chat-types.ts:505` `StreamAttachment` += `{kind:'file', name, data_base64, mime_type}`; `chat-normalizers.ts` `toStreamAttachments` emits file bytes for xlsx/csv/pdf/json/docx/pptx only when `attachment_policy.modes.sandbox_files`; the inlined text is still sent (never instead). `use-chat-controller.ts` call sites (`:1894`, `:2856`, WT lines) unchanged apart from the type. A "Brukes av kodekjøring (skrivebeskyttet)" chip on the attachment.
- `RS/model-gateway/src/http_routes.rs:6693` consumers - `attachment_policy.modes.sandbox_files` true when flag on.

**Flag:** `CHAT_SANDBOX_ATTACHMENTS_ENABLED` (opt-in, default off) on model-gateway; execution-core honours the proto field unconditionally (inert when empty). Rollback: unset. **ZDR clause:** refused on ZDR turns by the existing table (`attachments` refused); the registry never holds bytes for a ZDR step because the step cannot carry them (`enforce_persistence_free_execution`, `grpc.rs:1237`, rejects); workspace deleted by the Drop guard; bytes never logged (log sha256, size, count); never written to session-core, events, artifacts or the audit outbox; sandbox output is still scrubbed. Golden case: `(code_interpreter staging, ZDR)` refused in `zdr_feature_table.rs` [live-only].
**Tests (unit, in crate)**
- `execution-core` `code_interpreter.rs` module: `an_attachment_is_readable_by_the_code`, `writing_into_the_attachments_directory_fails_read_only` (extends the pattern at `:1077` "a_files_in_entry_is_readable_by_the_code"), `attachments_name_is_reserved_for_files_in`, `unsafe_attachment_names_are_rejected_never_sanitized` (pattern `:825`), `workspace_is_removed_after_the_call`; `sandbox.rs`: `ro_inputs_are_bound_after_the_writable_root` (pattern `:546`); `run_inputs.rs`: `last_guard_drop_removes_and_zeroes`, `parallel_steps_share_one_registration`, `ttl_sweep_evicts_orphans`, `byte_cap_rejects_registration`.
- `mp-contracts`: `sandbox_note_never_contains_client_text_beyond_validated_names`.
- model-gateway: `attachments_reach_execute_step_and_model_cannot_override` in `tests/` with a fake `ExecutionCore` server.
- Cross-service: extend `RS/execution-core/tests/cross_service_loop_contract.rs` (pattern named in `MP/CLAUDE.md`): the caps constant in `mp-contracts` is the one both services use; mutation-tested.
- Run: `cd "apps/Model Plane/rust" && cargo test -q -p execution-core --all-targets` (Windows: bwrap tests are Linux-gated; run in the `rust:1` container per memory), `cargo test -q -p mp-contracts`, `-p model-gateway`; `bash "apps/Model Plane/scripts/verify-sandbox-isolation.sh"` exists (not in CI) and is the live isolation proof.
**DoD:** a 500 KB xlsx + "chart revenue by month" produces a chart artifact; code that writes into `attachments/` gets EROFS; a follow-up turn without re-attach has no file. **Evidence:** rebuild execution-core and model-gateway; live flow with flag on: capture (1) SSE with `tool_call code_interpreter` and `tool_result`, (2) the output `attachment` event (`artifacts.rs:481`), (3) execution-core log line with the bwrap argv containing `--ro-bind` for the staging dir, (4) a second call whose code attempts `open('attachments/x','w')` and returns `Read-only file system`, (5) host `ls` of the temp dir proving no `verevon-code-*` remains.
**Deps:** S2 (validator and policy), D2; WS05 (tool classes) only consumes. **Risks:** persistent Space workspace interplay (S3.3, complete): the reserved `attachments` name must be free in Space roots (assumption); gRPC size growth; bwrap absent on a dev host.

### S6. Do loop: attachments on `RunAgentRequest`, images in the Do seed [BUILD] (M-L, 5-7 d)

**Goal.** A Do run receives only what model-gateway resolved and validated: files staged read-only for its `code_interpreter` steps for the whole run, images in the seed user message, all in memory in the process that runs the loop.

**Files**
- `MP/proto/model_plane/v1/execution.proto` - `RunAgentRequest` (`:76-209`, highest 17, no `reserved`) += `repeated RunAttachment attachments = <integrator, proposal 20>;` (fallback D3-b: `RunContext.attachments = 5`).
- `RS/model-gateway/src/sse.rs` - `build_run_agent_request` (`:8758-8835` per WS02) copies `ValidatedAttachments`; the `sse.rs` early return for agentic stays after validation (Do + image is refused until this slice flips); remove the `attachment_do_unsupported` guard behind the flag.
- `RS/execution-core/src/grpc.rs:1237` - after `enforce_persistence_free_execution`, `mp_contracts::attachments::validate(&req.attachments, req.zdr)` (refuses any attachment under ZDR, over-cap, bad names); `runtime_loop/agent.rs` - register `RunInputs` guard for the run; seed builder (WS02 `seed_messages(ctx, goal)`, `agent.rs:~1181-1221`) appends `sandbox_note`; images become `image_parts` on the first and every `InferRequest` of the run (`agent.rs:1331-1381`, full struct literal: add the field there). Subagent children (`agent.rs:3109-3140`) do NOT inherit attachments (least privilege; WS25 may extend).
- Struct-literal fixes: the 27 `RunAgentRequest` literal sites enumerated by WS02 (execution-core `agent_route.rs`, `grpc.rs`, `agent.rs`, model-gateway `sse.rs` tests, `tests/e2e_invoke_chain_test.rs`): use `..Default::default()`; coordinate so only one stream does the mechanical conversion (WS02 does it first).
- `FE`: composer allows images with Do once `modes.vision` and `attachment_policy.modes.do_attachments` are true; remove `do_images_unsupported`.

**Flag:** `CHAT_DO_ATTACHMENTS_ENABLED` (opt-in, default off). **ZDR clause:** `validate` refuses all attachments when `zdr`; Do already refused under ZDR (`sse.rs:976-977` lists `agentic`); checkpoints must never contain attachment bytes: add the field to WS03's checkpoint scrub (`scrub_checkpoint_test.rs`, named in WS02). Golden: `(Do, attachment, ZDR)` row.
**Tests:** `a_do_run_with_attachments_under_zdr_is_refused_at_the_boundary`, `children_do_not_inherit_attachments`, `images_ride_every_round_of_the_run`, `run_inputs_released_on_terminal_and_on_cancel`; wire-parity golden for `RunAgentRequest` (WS02 adds the harness). Run `cargo test -q -p execution-core --all-targets -p model-gateway`.
**DoD / Evidence:** live Do run "analyse attached sales.xlsx and write a summary" with approvals; capture the plan, the `code_interpreter` steps, the run record, and that cancel mid-run drops the registry (execution-core metric/log). **Deps:** S4, S5, WS02 S4/S7 (field and seed), WS03 (checkpoint scrub). **Risks:** merge pressure on `execution.proto`, `agent.rs`, `sse.rs` (section 5 rule); a resumed run (WS03) must not lose bytes: v1 states that a resumed Do run re-requests attachments (refusal reason `attachments_expired`) rather than persisting them.

### S7. Raise caps safely [BUILD] (M, 4-5 d; blocked on D1 for the numbers)

**Goal.** Make the OD18 number deliverable once ruled, without breaking any hop.

**Files**
- `FE/src/features/chat/lib/pending-chat-launch.ts:7,66-97` - hand attachments off through an in-memory module store (`temporaryLaunch` already exists for ZDR, `:92-96`) plus IndexedDB for reload survival (non-ZDR only), instead of serializing data URLs into `sessionStorage`; keep the 2,000,000-char cap for the text envelope.
- `FE/src/features/chat/lib/prepare-chat-attachments.ts:18` - read the limit from `attachment_policy`.
- `FE` gateway + model-gateway body limits (S2 consts) and tonic limits (S3, S5) re-derived from `max_total_bytes`; `apps/Ingestion Plane/imports-core/app/chat_extract.py:20,29-30` raise base64 and byte caps in step (keep the 60,000 char limit; H1 zip-bomb guard `OfficeArchiveTooLargeError` is the precondition named by OD18).
**Flag:** none beyond S2 env. **ZDR:** ZDR launches stay in memory only (existing). **Tests:** `pending-chat-launch.test.ts` extended (two 3 MB files survive handoff; ZDR still never touches storage); imports-core `pytest tests/test_chat_extract.py` new oversize cases; Playwright `product-office-outputs.spec.ts:43-66` re-run. **Evidence:** a Home-launched send with two 3 MB files end to end through all four hops. **Deps:** D1, S2. **Risks:** browser memory; IndexedDB blocked in private windows (fall back to refusal, never to truncation).

### S8. Durable reference uploads and OCR [BUILD, deferred] (XL, not sized)

Outline only, so nothing built now blocks it. Preconditions: D1 ruled, Data/Ingestion owners agree the route, per-org opt-in policy (WS25 registry), Space session's imports-core ingress (section 6).
- Gateway `chat.upload_document` / `chat.extract_document` actions (`FE/src/shared/actions/action-registry.ts:2904-2924`) already exist; the durable route is documents-api-go (ZDR 403 at `documents.go:304-305`), not model-gateway. Do not wire the text route as-is (asynchronous indexing was measured to miss the first turn, see `chat-normalizers.ts` inlining comment).
- Content hash per org (never global: cross-tenant dedup leaks existence), deletion cascade to derived chunks (DPA erasure), ZDR refuse.
- OCR: OD18 recommends local Tesseract in imports-core (`chat_extract.py` currently rejects scanned input); inference-core `doc_intel.rs` is layout extraction, not a verified OCR path. PDF page images need S3.
- Reference-based attachments reuse `RunAttachment` with `content` empty and a signed `url`/`id` resolved by execution-core through a Data Plane API, never a database.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| WS19 office deliverables (F-t3-014 depends on F-t3-017) | `attachments/` read-only mount inside `code_interpreter` (S5/S6); `mp-contracts::attachments` caps, `RunAttachment`; WS19 adds its LibreOffice layer and bundle mount on the same `extra_ro_binds` seam (S5 passes a vector, so a second pair is additive) |
| WS22 sandbox computer / WS24 file tools | Read-only input mount semantics and reserved name `attachments`; the future `SandboxBackend` seam must preserve ro binds (`External` policy applies no bwrap: refuse staging there) |
| WS10 model entitlements | `/v1/models` gains sibling `attachment_policy`; WS10 owns the `models` array and per-model filtering; `features` keeps `vision` |
| WS27 cost meter | image parts count as input tokens; `usage` frame unchanged, provider usage must include them |
| WS26 content safety | attachment text is user-authored, already inlined; sandbox outputs stay `unscreened` provenance (`tool_loop.rs:2570-2590` pattern); extension point `RunAttachment.sha256` for scanners |
| WS12 CI gates | Vitest/ cargo tests named above; FE gate test as an architecture guard (no silent `[]` drop) |
| WS03/WS02 | Do attachments registry lifetime (release on terminal, cancel, park) |

**CONSUMES**

| From | Contract |
|---|---|
| WS01 | ZDR wave committed: `zdr_posture.rs`, `zdr_features.rs`, tests, `sse.rs`/`http_routes.rs`/`tool_loop.rs`/FE files |
| WS02 | `RunAgentRequest` tag 18 and `seed_messages` (S6); D8 field allocation; 27-site literal conversion |
| WS11 | `InferRequest` tag 17 |
| WS03 | checkpoint scrub excludes attachment bytes |
| WS05 | tool-class declaration for `code_interpreter` unchanged (low-risk sandbox); no gate bypass |
| Integrator | proto tags: `InferRequest.image_parts` (proposal 20), `RunAgentRequest.attachments` (proposal 20), `ExecuteStepRequest.attachments = 12` |

**Conflict hot spots and edit order**

| File | Others | Rule |
|---|---|---|
| `model-gateway/src/sse.rs`, `tool_loop.rs`, `http_routes.rs` (live-modified) | WS01, WS02, WS03, WS04, WS27, WS10 | WS01 commits the live wave first; WS08 adds new modules and thin diffs; S2's validator call merges before S4 |
| `proto/.../execution.proto`, `inference.proto` | WS02, WS03, WS11, WS24 | integrator assigns tags; WS02 first; add-only; `buf breaking` in CI (`.github/workflows/buf.yml`) |
| `execution-core/.../agent.rs`, `runtime_loop/mod.rs` | WS02, WS03, WS05, WS24, WS25 | WS08 touches only the `code_interpreter` arm and the seed note; WS03 owns checkpoints |
| `FE .../chat-normalizers.ts`, `DashboardComposer.tsx`, `chat-client.ts`, `use-chat-controller.ts` | WS09, WS11, WS14, WS17 | WS08 adds new files (`attachment-policy.ts`) and minimal hooks; WS09 transcript refactor edits first on the controller |
| `/v1/models` response | WS10 | WS10 edits `models`, WS08 only `attachment_policy` |

## 6. Cross-session interfaces

- **Space session (a):** overlap-space row 13 (`.plan-work/common/overlap-space.md:30`): "Durable upload path differs (imports-core Space ingress vs ephemeral model-gateway store)". Ownership: durable and Space-bound uploads belong to the Space session and Data/Ingestion Planes (S8 defers to them); this stream owns only the ephemeral Ask/Do path. Assumed from (a): a Space composer that attaches files calls the same `attachment_policy` and `toStreamAttachments`; durable Space files reach sandboxes through the S3.3 workspace, not this mount; nothing else uses the name `attachments` in a Space workspace root.
- **Quarry/browser session (b):** attachments are never passed to Quarry or `browser_agent`; a browser upload-a-file action is a separate Quarry-owned contract. Assumed from (b): none. Shared watch item: both sessions edit `execution.proto` and `agent.rs`; follow section 5.

## 7. Regression guards (audit section 8)

| Strength | How this stream touches it | Gate |
|---|---|---|
| ZDR by construction | attachment refusal precedes everything; staging refuses ZDR; images never persisted | `zdr_feature_table.rs`, `zdr_posture_matrix.rs` [live-only] new rows; `bash scripts/tests/zdr-contract-sync.test.sh`; `execution-core` boundary test |
| Two loops separate | only a pure contracts crate is shared; gateway loop still reaches execution only via `ExecuteStep` | `cross_service_loop_contract.rs` extended; review rule: no `dispatch_tool` side effects added |
| Kernel-enforced sandbox | ro bind is additive, after the writable bind; unsandboxed host refuses staging | `sandbox.rs` argv tests; `verify-sandbox-isolation.sh` live |
| Honest outcomes | never silent drop; invalid base64 now errors instead of falling back to plain chat | FE `attachment-policy.test.ts`; `attachment_policy.rs` tests |
| Typed event taxonomy, byte-verbatim BFF | no new event variants; error codes documented; BFF only gains a body limit | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; BFF `cargo test` |
| Credential isolation | sandbox env cleared, no network; bytes never logged, sha256 only | `child_env` tests (existing), new log-redaction test |
| Intent routing with provenance | vision filter is another skip-before-network gate; served-by/privacy tier unchanged | `fallback.rs` tests |
| Multi-tenant runtime | bytes never cross plane databases; durable path stays Data Plane API | review rule, S8 |

## 8. Effort and risks

**Total:** S1-S6 about 34-44 engineer-days; S7 +4-5; S8 not sized. **Critical path:** S2 (validator and tags) then S3 then S4; S5 runs in parallel after S2; S6 last (needs WS02 S4/S7). A releasable "no silent loss" increment ships after S1 and S2 (about 8-10 days) with no proto change.

**Top risks**
1. **Hot-file merge pressure** (`sse.rs`, `tool_loop.rs`, `agent.rs`, both protos). Mitigation: WS01 commits live work first, new modules, add-only protos, integrator-assigned tags.
2. **Stacked limits** (handoff, 2 MiB body, 4 MiB gRPC) make cap raises fail in the wrong hop. Mitigation: caps server-declared (S2), limits derived from one number (S7), a four-hop test.
3. **Per-model vision truth.** Provider-level flag can over-advertise. Mitigation: denylist function with a test table; fail closed with `vision_unsupported` server-side regardless of client belief.
4. **Weakening the sandbox claim.** Read-only guarantee holds only where bwrap runs. Mitigation: refuse staging on a passthrough host; live negative-write proof in evidence.
5. **Bytes in requests** (memory, logs, checkpoints). Mitigation: request-scope only, RAII zeroing registry, sha256-only logging, checkpoint scrub test with WS03, ZDR refusal at the boundary.
